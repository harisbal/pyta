//! Python bindings for PyTA. The Rust types here are thin wrappers; the friendly
//! Python API (DataFrame handling, docstrings) lives in `python/pyta/__init__.py`.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;

use numpy::{IntoPyArray, PyArray1, PyReadonlyArray1};
use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use pyta_core as core;

fn err(e: impl Debug) -> PyErr {
    PyValueError::new_err(format!("{e:?}"))
}

fn msg(text: impl Into<String>) -> PyErr {
    PyValueError::new_err(text.into())
}

/// Warns that the argument `old` is deprecated in favour of `new`.
fn deprecated_argument(py: Python<'_>, old: &str, new: &str) -> PyResult<()> {
    let category = py.get_type::<pyo3::exceptions::PyDeprecationWarning>();
    PyErr::warn(
        py,
        &category,
        &std::ffi::CString::new(format!("{old} is deprecated, use {new}")).expect("no NUL"),
        2,
    )
}

fn to_u32(values: &PyReadonlyArray1<'_, i64>, what: &str) -> PyResult<Vec<u32>> {
    values
        .as_array()
        .iter()
        .map(|&v| u32::try_from(v).map_err(|_| msg(format!("{what}: {v} is not a valid id"))))
        .collect()
}

// ---------------------------------------------------------------- Network

/// A road network with its link delay functions and turn table.
#[pyclass(module = "pyta._pyta", frozen)]
struct Network {
    network: core::Network,
    turns: core::TurnTable,
    link_vdf: core::Bpr,
    turn_vdf: core::Bpr,
    /// Link capacities as given (the delay function keeps only their inverse).
    capacity: Vec<f64>,
    is_endpoint: Vec<bool>,
    /// Set by `from_gmns`: the file's ids, and the nodes of each zone.
    gmns: Option<GmnsMeta>,
}

struct GmnsMeta {
    node_ids: Vec<String>,
    link_ids: Vec<String>,
    zone_nodes: HashMap<String, Vec<u32>>,
}

/// Turn rows given by the user, one entry per row. NaN means "not set" for the floats.
struct TurnRows {
    from_link: Vec<u32>,
    to_link: Vec<u32>,
    penalty: Vec<f64>,
    banned: Vec<bool>,
    /// free-flow delay, capacity, b, power
    delay: [Vec<f64>; 4],
}

fn optional_column<'py, T: numpy::Element + Copy>(
    dict: &Bound<'py, PyDict>,
    key: &str,
    len: usize,
    default: T,
) -> PyResult<Vec<T>> {
    match dict.get_item(key)? {
        None => Ok(vec![default; len]),
        Some(value) => {
            let array: PyReadonlyArray1<'py, T> = value.extract()?;
            let values = array.as_array().to_vec();
            if values.len() != len {
                return Err(msg(format!(
                    "turns['{key}'] has {} rows, expected {len}",
                    values.len()
                )));
            }
            Ok(values)
        }
    }
}

fn read_turn_rows(dict: &Bound<'_, PyDict>) -> PyResult<TurnRows> {
    let from_link: PyReadonlyArray1<i64> = dict
        .get_item("from_link")?
        .ok_or_else(|| msg("turns needs a 'from_link' column"))?
        .extract()?;
    let to_link: PyReadonlyArray1<i64> = dict
        .get_item("to_link")?
        .ok_or_else(|| msg("turns needs a 'to_link' column"))?
        .extract()?;
    let from_link = to_u32(&from_link, "turns['from_link']")?;
    let to_link = to_u32(&to_link, "turns['to_link']")?;
    if from_link.len() != to_link.len() {
        return Err(msg(
            "turns['from_link'] and turns['to_link'] differ in length",
        ));
    }
    let n = from_link.len();
    Ok(TurnRows {
        penalty: optional_column(dict, "penalty", n, f64::NAN)?,
        banned: optional_column(dict, "banned", n, false)?,
        delay: [
            optional_column(dict, "delay_free_flow", n, f64::NAN)?,
            optional_column(dict, "delay_capacity", n, f64::NAN)?,
            optional_column(dict, "delay_b", n, f64::NAN)?,
            optional_column(dict, "delay_power", n, f64::NAN)?,
        ],
        from_link,
        to_link,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_network(
    num_nodes: usize,
    from_node: &[u32],
    to_node: &[u32],
    free_flow_time: Vec<f64>,
    capacity: Vec<f64>,
    b: Option<Vec<f64>>,
    power: Option<Vec<f64>>,
    is_endpoint: Option<Vec<bool>>,
    turn_rows: Option<TurnRows>,
    allow_u_turns: bool,
) -> PyResult<Network> {
    let n = from_node.len();
    if to_node.len() != n {
        return Err(msg("from_node and to_node differ in length"));
    }
    let edges: Vec<core::Edge> = (0..n)
        .map(|i| core::Edge {
            link_id: i as u32,
            from: from_node[i],
            to: to_node[i],
        })
        .collect();
    let network = core::Network::from_edges(num_nodes, &edges).map_err(err)?;

    let b = b.unwrap_or_else(|| vec![0.15; n]);
    let power = power.unwrap_or_else(|| vec![4.0; n]);
    let link_vdf = core::Bpr::new(free_flow_time, capacity.clone(), b, power).map_err(err)?;
    let is_endpoint = is_endpoint.unwrap_or_else(|| vec![false; num_nodes]);
    if is_endpoint.len() != num_nodes {
        return Err(msg(format!(
            "is_centroid has {} entries, expected {num_nodes}",
            is_endpoint.len()
        )));
    }

    // Bans and penalties go into the table; delay functions are filled in afterwards.
    let mut exceptions = Vec::new();
    if let Some(rows) = &turn_rows {
        for i in 0..rows.from_link.len() {
            let (from_link, to_link) = (rows.from_link[i], rows.to_link[i]);
            let has_penalty = !rows.penalty[i].is_nan();
            let rule = match (rows.banned[i], has_penalty) {
                (true, true) => {
                    return Err(msg(format!(
                        "turn {from_link}->{to_link} is both banned and penalised"
                    )));
                }
                (true, false) => Some(core::TurnRule::Ban),
                (false, true) => Some(core::TurnRule::Penalty(rows.penalty[i])),
                (false, false) => None,
            };
            if let Some(rule) = rule {
                exceptions.push(core::TurnException {
                    from_link,
                    to_link,
                    rule,
                });
            }
        }
    }
    let options = core::TurnOptions { allow_u_turns };
    let turns =
        core::TurnTable::build(&network, &is_endpoint, &exceptions, options).map_err(err)?;

    let nt = turns.num_turns();
    let (mut t0, mut cap, mut tb, mut tp) =
        (vec![0.0; nt], vec![1.0; nt], vec![0.0; nt], vec![1.0; nt]);
    if let Some(rows) = &turn_rows {
        for i in 0..rows.from_link.len() {
            let delay = [
                rows.delay[0][i],
                rows.delay[1][i],
                rows.delay[2][i],
                rows.delay[3][i],
            ];
            let set = delay.iter().filter(|v| !v.is_nan()).count();
            if set == 0 {
                continue;
            }
            let (from_link, to_link) = (rows.from_link[i], rows.to_link[i]);
            if set != 4 {
                return Err(msg(format!(
                    "turn {from_link}->{to_link}: give all of delay_free_flow, delay_capacity, delay_b and delay_power, or none"
                )));
            }
            let id = turns.turn_between(from_link, to_link).ok_or_else(|| {
                msg(format!(
                    "turn {from_link}->{to_link} has a delay function but is not an allowed turn"
                ))
            })?;
            (t0[id], cap[id], tb[id], tp[id]) = (delay[0], delay[1], delay[2], delay[3]);
        }
    }
    let turn_vdf = core::Bpr::new(t0, cap, tb, tp).map_err(err)?;
    Ok(Network {
        network,
        turns,
        link_vdf,
        turn_vdf,
        capacity,
        is_endpoint,
        gmns: None,
    })
}

#[pymethods]
impl Network {
    /// Links are numbered by their position in the arrays; node ids are 0..num_nodes.
    #[new]
    #[pyo3(signature = (num_nodes, from_node, to_node, free_flow_time, capacity, b=None, power=None, is_centroid=None, turns=None, allow_u_turns=false, is_endpoint=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        num_nodes: usize,
        from_node: PyReadonlyArray1<'_, i64>,
        to_node: PyReadonlyArray1<'_, i64>,
        free_flow_time: PyReadonlyArray1<'_, f64>,
        capacity: PyReadonlyArray1<'_, f64>,
        b: Option<PyReadonlyArray1<'_, f64>>,
        power: Option<PyReadonlyArray1<'_, f64>>,
        is_centroid: Option<PyReadonlyArray1<'_, bool>>,
        turns: Option<&Bound<'_, PyDict>>,
        allow_u_turns: bool,
        is_endpoint: Option<PyReadonlyArray1<'_, bool>>,
    ) -> PyResult<Network> {
        let is_centroid = match (is_centroid, is_endpoint) {
            (None, Some(old)) => {
                deprecated_argument(py, "is_endpoint", "is_centroid")?;
                Some(old)
            }
            (given, _) => given,
        };
        let rows = turns.map(read_turn_rows).transpose()?;
        build_network(
            num_nodes,
            &to_u32(&from_node, "from_node")?,
            &to_u32(&to_node, "to_node")?,
            free_flow_time.as_array().to_vec(),
            capacity.as_array().to_vec(),
            b.map(|v| v.as_array().to_vec()),
            power.map(|v| v.as_array().to_vec()),
            is_centroid.map(|v| v.as_array().to_vec()),
            rows,
            allow_u_turns,
        )
    }

    /// Reads a TNTP `*_net.tntp` file (node ids become 0-based, links keep file order).
    #[staticmethod]
    fn from_tntp(path: &str) -> PyResult<Network> {
        let net = pyta_io::tntp::read_net(path).map_err(|e| msg(e.to_string()))?;
        let n = net.network.num_links();
        let from: Vec<u32> = (0..n as u32).map(|l| net.network.from_node(l)).collect();
        let to: Vec<u32> = (0..n as u32).map(|l| net.network.to_node(l)).collect();
        build_network(
            net.network.num_nodes(),
            &from,
            &to,
            net.free_flow_time,
            net.capacity,
            Some(net.b),
            Some(net.power),
            Some(net.is_endpoint),
            None,
            false,
        )
    }

    /// Reads a GMNS dataset directory (`node.csv`, `link.csv`, optional `movement.csv`
    /// and `config.csv`). Times are in minutes. See ADR 0009 for the rules.
    #[staticmethod]
    #[pyo3(signature = (path, length_unit=None, speed_unit=None, zone_nodes_are_centroids=false, zone_nodes_are_endpoints=None))]
    fn from_gmns(
        py: Python<'_>,
        path: &str,
        length_unit: Option<String>,
        speed_unit: Option<String>,
        zone_nodes_are_centroids: bool,
        zone_nodes_are_endpoints: Option<bool>,
    ) -> PyResult<Network> {
        let zone_nodes_are_endpoints = match zone_nodes_are_endpoints {
            Some(old) => {
                deprecated_argument(py, "zone_nodes_are_endpoints", "zone_nodes_are_centroids")?;
                old
            }
            None => zone_nodes_are_centroids,
        };
        let options = pyta_io::gmns::GmnsOptions {
            length_unit,
            speed_unit,
            zone_nodes_are_endpoints,
            ..Default::default()
        };
        let g = pyta_io::gmns::read_network(path, &options).map_err(|e| msg(e.to_string()))?;
        let n = g.network.num_links();
        let from: Vec<u32> = (0..n as u32).map(|l| g.network.from_node(l)).collect();
        let to: Vec<u32> = (0..n as u32).map(|l| g.network.to_node(l)).collect();
        // Bans and penalties from movement.csv, as the turn rows `build_network` takes.
        let rows = g.turn_exceptions.iter().fold(
            TurnRows {
                from_link: Vec::new(),
                to_link: Vec::new(),
                penalty: Vec::new(),
                banned: Vec::new(),
                delay: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            },
            |mut rows, e| {
                rows.from_link.push(e.from_link);
                rows.to_link.push(e.to_link);
                let (banned, penalty) = match e.rule {
                    core::TurnRule::Ban => (true, f64::NAN),
                    core::TurnRule::Penalty(p) => (false, p),
                };
                rows.banned.push(banned);
                rows.penalty.push(penalty);
                for d in &mut rows.delay {
                    d.push(f64::NAN);
                }
                rows
            },
        );
        let mut net = build_network(
            g.network.num_nodes(),
            &from,
            &to,
            g.free_flow_time,
            g.capacity,
            Some(g.b),
            Some(g.power),
            Some(g.is_endpoint),
            Some(rows),
            false,
        )?;
        net.gmns = Some(GmnsMeta {
            node_ids: g.node_ids,
            link_ids: g.link_ids,
            zone_nodes: g.zone_nodes,
        });
        Ok(net)
    }

    /// The `node_id` of every node in the GMNS files, or None for other networks.
    #[getter]
    fn node_ids(&self) -> Option<Vec<String>> {
        self.gmns.as_ref().map(|g| g.node_ids.clone())
    }

    /// The `link_id` of every link (an undirected link appears twice), or None.
    #[getter]
    fn link_ids(&self) -> Option<Vec<String>> {
        self.gmns.as_ref().map(|g| g.link_ids.clone())
    }

    #[getter]
    fn num_nodes(&self) -> usize {
        self.network.num_nodes()
    }

    #[getter]
    fn num_links(&self) -> usize {
        self.network.num_links()
    }

    #[getter]
    fn num_turns(&self) -> usize {
        self.turns.num_turns()
    }

    /// Capacity per link.
    #[getter]
    fn capacity<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.capacity.clone().into_pyarray(py)
    }

    #[getter]
    fn is_centroid<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<bool>> {
        self.is_endpoint.clone().into_pyarray(py)
    }

    /// Per link: from-node and to-node ids.
    fn link_nodes<'py>(
        &self,
        py: Python<'py>,
    ) -> (Bound<'py, PyArray1<u32>>, Bound<'py, PyArray1<u32>>) {
        let n = self.network.num_links() as u32;
        let from = (0..n)
            .map(|l| self.network.from_node(l))
            .collect::<Vec<_>>();
        let to = (0..n).map(|l| self.network.to_node(l)).collect::<Vec<_>>();
        (from.into_pyarray(py), to.into_pyarray(py))
    }

    /// Per turn id: the incoming and outgoing link. Use this to see which turns exist.
    fn turn_links<'py>(
        &self,
        py: Python<'py>,
    ) -> (Bound<'py, PyArray1<u32>>, Bound<'py, PyArray1<u32>>) {
        let n = self.turns.num_turns();
        let from = (0..n).map(|t| self.turns.from_link(t)).collect::<Vec<_>>();
        let to = (0..n).map(|t| self.turns.to_link(t)).collect::<Vec<_>>();
        (from.into_pyarray(py), to.into_pyarray(py))
    }

    fn __repr__(&self) -> String {
        format!(
            "Network(nodes={}, links={}, turns={})",
            self.network.num_nodes(),
            self.network.num_links(),
            self.turns.num_turns()
        )
    }
}

// ----------------------------------------------------------------- Demand

/// Trips between nodes (zone = node), in long format.
#[pyclass(module = "pyta._pyta", frozen)]
struct Demand {
    od: core::OdDemand,
    num_nodes: usize,
    /// Trips dropped because their zone has no node (GMNS only).
    skipped_flow: f64,
}

#[pymethods]
impl Demand {
    #[new]
    fn new(
        num_nodes: usize,
        origins: PyReadonlyArray1<'_, i64>,
        destinations: PyReadonlyArray1<'_, i64>,
        flows: PyReadonlyArray1<'_, f64>,
    ) -> PyResult<Demand> {
        let od = core::OdDemand::from_triples(
            num_nodes,
            &to_u32(&origins, "origins")?,
            &to_u32(&destinations, "destinations")?,
            flows.as_slice()?,
        )
        .map_err(err)?;
        Ok(Demand {
            od,
            num_nodes,
            skipped_flow: 0.0,
        })
    }

    /// Reads a TNTP `*_trips.tntp` file for `network`.
    #[staticmethod]
    fn from_tntp(network: &Network, path: &str) -> PyResult<Demand> {
        let trips = pyta_io::tntp::read_trips(path).map_err(|e| msg(e.to_string()))?;
        let num_nodes = network.network.num_nodes();
        let od = core::OdDemand::from_triples(
            num_nodes,
            &trips.origins,
            &trips.destinations,
            &trips.flows,
        )
        .map_err(err)?;
        Ok(Demand {
            od,
            num_nodes,
            skipped_flow: 0.0,
        })
    }

    /// Reads a GMNS-style demand file (one row per zone pair) for a network read with
    /// `Network.from_gmns`. Zone trips are split equally over the zone's nodes.
    #[staticmethod]
    #[pyo3(signature = (network, path, origin="o_zone_id", destination="d_zone_id", flow="volume", skip_unknown_zones=false))]
    fn from_gmns(
        network: &Network,
        path: &str,
        origin: &str,
        destination: &str,
        flow: &str,
        skip_unknown_zones: bool,
    ) -> PyResult<Demand> {
        let meta = network
            .gmns
            .as_ref()
            .ok_or_else(|| msg("network was not read with Network.from_gmns"))?;
        let options = pyta_io::gmns::DemandOptions {
            origin: origin.to_string(),
            destination: destination.to_string(),
            flow: flow.to_string(),
            skip_unknown_zones,
        };
        let read = pyta_io::gmns::read_zone_demand(path, &meta.zone_nodes, &options)
            .map_err(|e| msg(e.to_string()))?;
        let num_nodes = network.network.num_nodes();
        let od = core::OdDemand::from_triples(
            num_nodes,
            &read.demand.origins,
            &read.demand.destinations,
            &read.demand.flows,
        )
        .map_err(err)?;
        Ok(Demand {
            od,
            num_nodes,
            skipped_flow: read.skipped_flow,
        })
    }

    /// Trips dropped because their zone has no node (0 unless read with `from_gmns`).
    #[getter]
    fn unknown_zone_flow(&self) -> f64 {
        self.skipped_flow
    }

    #[getter]
    fn num_nodes(&self) -> usize {
        self.num_nodes
    }

    fn total(&self) -> f64 {
        self.od.total()
    }

    fn __repr__(&self) -> String {
        format!(
            "Demand(nodes={}, total={})",
            self.num_nodes,
            self.od.total()
        )
    }
}

// ----------------------------------------------------------- TrafficClass

/// One traffic class: demand plus how its vehicles load and move on the network.
#[pyclass(module = "pyta._pyta", frozen)]
struct TrafficClass {
    demand: Py<Demand>,
    /// Used to label the class in result tables; defaults to its position.
    name: Option<String>,
    pce: f64,
    extra_link_cost: Option<Vec<f64>>,
    allowed_links: Option<Vec<bool>>,
}

#[pymethods]
impl TrafficClass {
    #[new]
    #[pyo3(signature = (demand, pce=1.0, extra_link_cost=None, allowed_links=None, name=None))]
    fn new(
        demand: Py<Demand>,
        pce: f64,
        extra_link_cost: Option<PyReadonlyArray1<'_, f64>>,
        allowed_links: Option<PyReadonlyArray1<'_, bool>>,
        name: Option<String>,
    ) -> TrafficClass {
        TrafficClass {
            demand,
            name,
            pce,
            extra_link_cost: extra_link_cost.map(|v| v.as_array().to_vec()),
            allowed_links: allowed_links.map(|v| v.as_array().to_vec()),
        }
    }
}

// ----------------------------------------------------------------- Result

#[pyclass(module = "pyta._pyta", name = "_RawResult", frozen)]
struct AssignResult {
    /// The network the result belongs to, for the tables built in Python.
    network: Py<Network>,
    classes: Vec<core::ClassResult>,
    class_names: Vec<String>,
    link_volume: Vec<f64>,
    turn_volume: Vec<f64>,
    /// Travel time per link and per turn at the final volumes (preload included).
    link_time: Vec<f64>,
    turn_time: Vec<f64>,
    /// The fixed volumes passed to `assign`, if any.
    link_preload: Option<Vec<f64>>,
    turn_preload: Option<Vec<f64>>,
    iterations: usize,
    relative_gap: f64,
    converged: bool,
    sweep_gaps: Vec<f64>,
    objective: Option<f64>,
    times: core::PhaseTimes,
    /// Demand dropped by `unreachable="skip"`.
    unrouted: Vec<core::Unrouted>,
}

impl AssignResult {
    fn class(&self, index: usize) -> PyResult<&core::ClassResult> {
        self.classes.get(index).ok_or_else(|| {
            PyIndexError::new_err(format!(
                "class {index} out of range ({} classes)",
                self.classes.len()
            ))
        })
    }
}

#[pymethods]
impl AssignResult {
    /// PCE-weighted volume per link (what travel times depend on).
    #[getter]
    fn link_volume<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.link_volume.clone().into_pyarray(py)
    }

    #[getter]
    fn turn_volume<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.turn_volume.clone().into_pyarray(py)
    }

    /// Vehicles of the first class per link.
    #[getter]
    fn link_flow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<f64>>> {
        Ok(self.class(0)?.link_flow.clone().into_pyarray(py))
    }

    #[getter]
    fn turn_flow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<f64>>> {
        Ok(self.class(0)?.turn_flow.clone().into_pyarray(py))
    }

    /// Vehicles of each class per link, as a list of arrays.
    #[getter]
    fn class_link_flow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(
            py,
            self.classes
                .iter()
                .map(|c| c.link_flow.clone().into_pyarray(py)),
        )
    }

    #[getter]
    fn class_turn_flow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(
            py,
            self.classes
                .iter()
                .map(|c| c.turn_flow.clone().into_pyarray(py)),
        )
    }

    #[getter]
    fn relative_gap(&self) -> f64 {
        self.relative_gap
    }

    #[getter]
    fn iterations(&self) -> usize {
        self.iterations
    }

    #[getter]
    fn converged(&self) -> bool {
        self.converged
    }

    #[getter]
    fn sweep_gaps<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.sweep_gaps.clone().into_pyarray(py)
    }

    /// Seconds spent per phase: `shortest_paths`, `flow_updates`, `sweep_rebuild`,
    /// `gap_checks`, `initial_assignment`.
    #[getter]
    fn times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let out = PyDict::new(py);
        out.set_item("initial_assignment", self.times.initial_assignment)?;
        out.set_item("shortest_paths", self.times.shortest_paths)?;
        out.set_item("flow_updates", self.times.flow_updates)?;
        out.set_item("sweep_rebuild", self.times.sweep_rebuild)?;
        out.set_item("gap_checks", self.times.gap_checks)?;
        Ok(out)
    }

    /// The fixed volume per link passed to `assign` as `link_preload`, or None.
    #[getter]
    fn link_preload<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyArray1<f64>>> {
        self.link_preload.clone().map(|v| v.into_pyarray(py))
    }

    /// The fixed volume per turn passed to `assign` as `turn_preload`, or None.
    #[getter]
    fn turn_preload<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyArray1<f64>>> {
        self.turn_preload.clone().map(|v| v.into_pyarray(py))
    }

    /// Travel time per link at the final volumes, preload included.
    #[getter]
    fn link_time<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.link_time.clone().into_pyarray(py)
    }

    /// Travel time per turn (fixed penalty plus delay) at the final volumes.
    #[getter]
    fn turn_time<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        self.turn_time.clone().into_pyarray(py)
    }

    /// One name per class: the `TrafficClass` name, or its position.
    #[getter]
    fn class_names(&self) -> Vec<String> {
        self.class_names.clone()
    }

    /// The network that was assigned.
    #[getter]
    fn network(&self, py: Python<'_>) -> Py<Network> {
        self.network.clone_ref(py)
    }

    /// Beckmann objective; only defined for a single class with PCE 1, otherwise `None`.
    #[getter]
    fn objective(&self) -> Option<f64> {
        self.objective
    }

    /// Every path of a class as flat arrays: `origin`, `destination`, `flow` and `time`
    /// (one entry per path), `link_offsets` (one more entry) and `links` (all link ids back
    /// to back).
    #[pyo3(signature = (class_index=0))]
    fn paths<'py>(&self, py: Python<'py>, class_index: usize) -> PyResult<Bound<'py, PyDict>> {
        let class = self.class(class_index)?;
        let (mut origin, mut dest, mut flow) = (Vec::new(), Vec::new(), Vec::new());
        let mut time = Vec::new();
        let mut offsets: Vec<i64> = vec![0];
        let mut links: Vec<u32> = Vec::new();
        for (o, d, path_links, path_turns, f) in class.paths.iter_with_turns() {
            origin.push(o);
            dest.push(d);
            flow.push(f);
            time.push(
                path_links
                    .iter()
                    .map(|&l| self.link_time[l as usize])
                    .sum::<f64>()
                    + path_turns
                        .iter()
                        .map(|&t| self.turn_time[t as usize])
                        .sum::<f64>(),
            );
            links.extend_from_slice(&path_links);
            offsets.push(links.len() as i64);
        }
        let out = PyDict::new(py);
        out.set_item("origin", origin.into_pyarray(py))?;
        out.set_item("destination", dest.into_pyarray(py))?;
        out.set_item("flow", flow.into_pyarray(py))?;
        out.set_item("time", time.into_pyarray(py))?;
        out.set_item("link_offsets", offsets.into_pyarray(py))?;
        out.set_item("links", links.into_pyarray(py))?;
        Ok(out)
    }

    /// Trips per class that `unreachable="skip"` left out because no allowed path joins
    /// their origin and destination.
    #[getter]
    fn unrouted_flow(&self) -> Vec<f64> {
        let mut totals = vec![0.0; self.classes.len()];
        for u in &self.unrouted {
            totals[u.class] += u.flow;
        }
        totals
    }

    /// The dropped demand as flat arrays: `class` (index), `origin`, `destination`, `flow`.
    fn unrouted<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let out = PyDict::new(py);
        let class: Vec<i64> = self.unrouted.iter().map(|u| u.class as i64).collect();
        let origin: Vec<i64> = self.unrouted.iter().map(|u| i64::from(u.origin)).collect();
        let dest: Vec<i64> = self
            .unrouted
            .iter()
            .map(|u| i64::from(u.destination))
            .collect();
        let flow: Vec<f64> = self.unrouted.iter().map(|u| u.flow).collect();
        out.set_item("class", class.into_pyarray(py))?;
        out.set_item("origin", origin.into_pyarray(py))?;
        out.set_item("destination", dest.into_pyarray(py))?;
        out.set_item("flow", flow.into_pyarray(py))?;
        Ok(out)
    }

    fn __repr__(&self) -> String {
        format!(
            "AssignResult(converged={}, iterations={}, relative_gap={:.3e}, classes={})",
            if self.converged { "True" } else { "False" },
            self.iterations,
            self.relative_gap,
            self.classes.len()
        )
    }
}

// ----------------------------------------------------------------- assign

fn parse_order(order: &str, seed: u64) -> PyResult<core::OriginOrder> {
    Ok(match order {
        "fixed" => core::OriginOrder::Fixed,
        "shuffle" => core::OriginOrder::Shuffle { seed },
        "alternate" => core::OriginOrder::Alternate,
        "largest_gap" => core::OriginOrder::LargestGapFirst,
        "largest_demand" => core::OriginOrder::LargestDemandFirst,
        "smallest_demand" => core::OriginOrder::SmallestDemandFirst,
        other => {
            return Err(msg(format!(
                "unknown order '{other}': use fixed, shuffle, alternate, largest_gap, largest_demand or smallest_demand"
            )));
        }
    })
}

/// One class as given to `assign`: a bare `Demand` or a full `TrafficClass`.
fn extract_class(item: &Bound<'_, PyAny>) -> PyResult<Py<TrafficClass>> {
    let py = item.py();
    if let Ok(class) = item.extract::<Py<TrafficClass>>() {
        return Ok(class);
    }
    if let Ok(demand) = item.extract::<Py<Demand>>() {
        return Py::new(
            py,
            TrafficClass {
                demand,
                name: None,
                pce: 1.0,
                extra_link_cost: None,
                allowed_links: None,
            },
        );
    }
    Err(msg(
        "classes must be a Demand, a TrafficClass or a list of them",
    ))
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (network, classes, *, gap=1e-6, max_iter=200, order="fixed", seed=0, chunk_size=0, threads=None, link_preload=None, turn_preload=None, unreachable="error"))]
fn assign(
    py: Python<'_>,
    network: Py<Network>,
    classes: &Bound<'_, PyAny>,
    gap: f64,
    max_iter: usize,
    order: &str,
    seed: u64,
    chunk_size: usize,
    threads: Option<usize>,
    link_preload: Option<PyReadonlyArray1<'_, f64>>,
    turn_preload: Option<PyReadonlyArray1<'_, f64>>,
    unreachable: &str,
) -> PyResult<AssignResult> {
    let skip_unreachable = match unreachable {
        "error" => false,
        "skip" => true,
        other => {
            return Err(msg(format!(
                "unreachable must be 'error' or 'skip', not '{other}'"
            )));
        }
    };
    let items: Vec<Bound<'_, PyAny>> = match classes.cast::<PyList>() {
        Ok(list) => list.iter().collect(),
        Err(_) => vec![classes.clone()],
    };
    if items.is_empty() {
        return Err(msg("assign needs at least one class"));
    }
    let class_objects: Vec<Py<TrafficClass>> =
        items.iter().map(extract_class).collect::<PyResult<_>>()?;
    let class_refs: Vec<PyRef<'_, TrafficClass>> =
        class_objects.iter().map(|c| c.bind(py).borrow()).collect();
    let demand_refs: Vec<PyRef<'_, Demand>> = class_refs
        .iter()
        .map(|c| c.demand.bind(py).borrow())
        .collect();
    let class_names: Vec<String> = class_refs
        .iter()
        .enumerate()
        .map(|(k, c)| c.name.clone().unwrap_or_else(|| k.to_string()))
        .collect();

    let base_specs: Vec<core::ClassSpec<'_>> = class_refs
        .iter()
        .zip(&demand_refs)
        .map(|(class, demand)| core::ClassSpec {
            demand: &demand.od,
            pce: class.pce,
            extra_link_cost: class.extra_link_cost.as_deref(),
            allowed_links: class.allowed_links.as_deref(),
        })
        .collect();
    let options = build_options(gap, max_iter, order, seed, chunk_size)?;
    let pool = build_pool(threads)?;

    let net_ref = network.bind(py).borrow();
    let net: &Network = &net_ref;
    let link_preload = preload(link_preload, net.network.num_links(), "link_preload")?;
    let turn_preload = preload(turn_preload, net.turns.num_turns(), "turn_preload")?;

    let unrouted = if skip_unreachable {
        let find = || core::find_unrouted(&net.network, &net.turns, &base_specs);
        py.detach(|| match &pool {
            Some(pool) => pool.install(find),
            None => find(),
        })
        .map_err(err)?
    } else {
        Vec::new()
    };
    let routable: Vec<Option<core::OdDemand>> = (0..base_specs.len())
        .map(|k| {
            let dropped: HashSet<(u32, u32)> = unrouted
                .iter()
                .filter(|u| u.class == k)
                .map(|u| (u.origin, u.destination))
                .collect();
            (!dropped.is_empty()).then(|| {
                base_specs[k]
                    .demand
                    .retain(|o, d| !dropped.contains(&(o, d)))
            })
        })
        .collect();
    let specs: Vec<core::ClassSpec<'_>> = base_specs
        .iter()
        .zip(&routable)
        .map(|(spec, kept)| core::ClassSpec {
            demand: kept.as_ref().unwrap_or(spec.demand),
            ..*spec
        })
        .collect();

    let solved = if link_preload.is_none() && turn_preload.is_none() {
        solve(
            py,
            &pool,
            net,
            &class_names,
            &net.link_vdf,
            &net.turn_vdf,
            &specs,
            options,
        )?
    } else {
        // A missing preload is zero; adding zero leaves the delays unchanged.
        let links = link_preload
            .clone()
            .unwrap_or_else(|| vec![0.0; net.network.num_links()]);
        let turns = turn_preload
            .clone()
            .unwrap_or_else(|| vec![0.0; net.turns.num_turns()]);
        solve(
            py,
            &pool,
            net,
            &class_names,
            &core::Preloaded::new(&net.link_vdf, &links),
            &core::Preloaded::new(&net.turn_vdf, &turns),
            &specs,
            options,
        )?
    };
    drop(net_ref);
    Ok(AssignResult {
        network,
        classes: solved.result.classes,
        class_names,
        link_volume: solved.result.link_volume,
        turn_volume: solved.result.turn_volume,
        link_time: solved.link_time,
        turn_time: solved.turn_time,
        link_preload,
        turn_preload,
        iterations: solved.result.iterations,
        relative_gap: solved.result.relative_gap,
        converged: solved.result.converged,
        sweep_gaps: solved.result.sweep_gaps,
        objective: (specs.len() == 1 && specs[0].pce == 1.0).then_some(solved.objective),
        times: solved.result.times,
        unrouted,
    })
}

/// A preload argument as a vector of `expected` non-negative volumes.
fn preload(
    values: Option<PyReadonlyArray1<'_, f64>>,
    expected: usize,
    what: &str,
) -> PyResult<Option<Vec<f64>>> {
    let Some(values) = values else {
        return Ok(None);
    };
    let values = values.as_array().to_vec();
    if values.len() != expected {
        return Err(msg(format!(
            "{what} has {} entries, expected {expected}",
            values.len()
        )));
    }
    if values.iter().any(|v| !(v.is_finite() && *v >= 0.0)) {
        return Err(msg(format!("{what} must be finite and non-negative")));
    }
    Ok(Some(values))
}

struct Solved {
    result: core::MultiClassResult,
    link_time: Vec<f64>,
    turn_time: Vec<f64>,
    /// The Beckmann objective; meaningful only for a single class with PCE 1.
    objective: f64,
}

/// Runs the solver with the GIL released, then evaluates times and the objective with the
/// same delay functions.
/// Turns a solver failure into a Python error. For demand no allowed path serves, it names
/// the offending origin-destination pairs rather than only the first one found.
fn solver_error(
    error: core::SolverError,
    net: &Network,
    specs: &[core::ClassSpec<'_>],
    class_names: &[String],
) -> PyErr {
    if matches!(error, core::SolverError::ClassUnreachable { .. })
        && let Ok(all) = core::find_unrouted(&net.network, &net.turns, specs)
        && !all.is_empty()
    {
        let total: f64 = all.iter().map(|u| u.flow).sum();
        let shown: Vec<String> = all
            .iter()
            .take(5)
            .map(|u| {
                format!(
                    "{} -> {} (class {}, {} trips)",
                    u.origin, u.destination, class_names[u.class], u.flow
                )
            })
            .collect();
        let more = match all.len().saturating_sub(5) {
            0 => String::new(),
            n => format!(", and {n} more"),
        };
        return msg(format!(
            "no allowed path serves {} origin-destination pair(s) carrying {total} trips: {}{more}. \
             Pass unreachable='skip' to leave them out and report them in result.unrouted.",
            all.len(),
            shown.join(", ")
        ));
    }
    err(error)
}

#[allow(clippy::too_many_arguments)]
fn solve<V: core::Vdf + Sync, T: core::Vdf + Sync>(
    py: Python<'_>,
    pool: &Option<rayon::ThreadPool>,
    net: &Network,
    class_names: &[String],
    link_vdf: &V,
    turn_vdf: &T,
    specs: &[core::ClassSpec<'_>],
    options: core::GradientProjectionOptions,
) -> PyResult<Solved> {
    let run = || {
        core::gradient_projection_multiclass(
            &net.network,
            &net.turns,
            link_vdf,
            turn_vdf,
            specs,
            options,
        )
    };
    let result = py
        .detach(|| match pool {
            Some(pool) => pool.install(run),
            None => run(),
        })
        .map_err(|e| solver_error(e, net, specs, class_names))?;

    let link_time: Vec<f64> = (0..net.network.num_links())
        .map(|l| link_vdf.cost_at(l, result.link_volume[l]))
        .collect();
    let turn_time: Vec<f64> = (0..net.turns.num_turns())
        .map(|t| net.turns.penalty(t) + turn_vdf.cost_at(t, result.turn_volume[t]))
        .collect();
    let penalties: f64 = (0..net.turns.num_turns())
        .map(|t| net.turns.penalty(t) * result.turn_volume[t])
        .sum();
    let objective =
        link_vdf.integral(&result.link_volume) + turn_vdf.integral(&result.turn_volume) + penalties;
    Ok(Solved {
        result,
        link_time,
        turn_time,
        objective,
    })
}

fn build_options(
    gap: f64,
    max_iter: usize,
    order: &str,
    seed: u64,
    chunk_size: usize,
) -> PyResult<core::GradientProjectionOptions> {
    Ok(core::GradientProjectionOptions {
        max_iterations: max_iter,
        gap_tolerance: gap,
        origin_order: parse_order(order, seed)?,
        chunk_size,
    })
}

fn build_pool(threads: Option<usize>) -> PyResult<Option<rayon::ThreadPool>> {
    threads
        .map(|n| {
            if n == 0 {
                return Err(msg("threads must be at least 1"));
            }
            rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .map_err(err)
        })
        .transpose()
}

#[pymodule]
fn _pyta(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Network>()?;
    m.add_class::<Demand>()?;
    m.add_class::<TrafficClass>()?;
    m.add_class::<AssignResult>()?;
    m.add_function(wrap_pyfunction!(assign, m)?)?;
    Ok(())
}
