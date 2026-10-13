//! Python bindings for PyTA. The Rust types here are thin wrappers; the friendly
//! Python API (DataFrame handling, docstrings) lives in `python/pyta/__init__.py`.

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
    is_endpoint: Vec<bool>,
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
    let link_vdf = core::Bpr::new(free_flow_time, capacity, b, power).map_err(err)?;
    let is_endpoint = is_endpoint.unwrap_or_else(|| vec![false; num_nodes]);
    if is_endpoint.len() != num_nodes {
        return Err(msg(format!(
            "is_endpoint has {} entries, expected {num_nodes}",
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
        is_endpoint,
    })
}

#[pymethods]
impl Network {
    /// Links are numbered by their position in the arrays; node ids are 0..num_nodes.
    #[new]
    #[pyo3(signature = (num_nodes, from_node, to_node, free_flow_time, capacity, b=None, power=None, is_endpoint=None, turns=None, allow_u_turns=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        num_nodes: usize,
        from_node: PyReadonlyArray1<'_, i64>,
        to_node: PyReadonlyArray1<'_, i64>,
        free_flow_time: PyReadonlyArray1<'_, f64>,
        capacity: PyReadonlyArray1<'_, f64>,
        b: Option<PyReadonlyArray1<'_, f64>>,
        power: Option<PyReadonlyArray1<'_, f64>>,
        is_endpoint: Option<PyReadonlyArray1<'_, bool>>,
        turns: Option<&Bound<'_, PyDict>>,
        allow_u_turns: bool,
    ) -> PyResult<Network> {
        let rows = turns.map(read_turn_rows).transpose()?;
        build_network(
            num_nodes,
            &to_u32(&from_node, "from_node")?,
            &to_u32(&to_node, "to_node")?,
            free_flow_time.as_array().to_vec(),
            capacity.as_array().to_vec(),
            b.map(|v| v.as_array().to_vec()),
            power.map(|v| v.as_array().to_vec()),
            is_endpoint.map(|v| v.as_array().to_vec()),
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

    #[getter]
    fn is_endpoint<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<bool>> {
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
        Ok(Demand { od, num_nodes })
    }

    /// Reads a TNTP `*_trips.tntp` file for `network`.
    #[staticmethod]
    fn from_tntp(path: &str, network: &Network) -> PyResult<Demand> {
        let trips = pyta_io::tntp::read_trips(path).map_err(|e| msg(e.to_string()))?;
        let num_nodes = network.network.num_nodes();
        let od = core::OdDemand::from_triples(
            num_nodes,
            &trips.origins,
            &trips.destinations,
            &trips.flows,
        )
        .map_err(err)?;
        Ok(Demand { od, num_nodes })
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
    pce: f64,
    extra_link_cost: Option<Vec<f64>>,
    allowed_links: Option<Vec<bool>>,
}

#[pymethods]
impl TrafficClass {
    #[new]
    #[pyo3(signature = (demand, pce=1.0, extra_link_cost=None, allowed_links=None))]
    fn new(
        demand: Py<Demand>,
        pce: f64,
        extra_link_cost: Option<PyReadonlyArray1<'_, f64>>,
        allowed_links: Option<PyReadonlyArray1<'_, bool>>,
    ) -> TrafficClass {
        TrafficClass {
            demand,
            pce,
            extra_link_cost: extra_link_cost.map(|v| v.as_array().to_vec()),
            allowed_links: allowed_links.map(|v| v.as_array().to_vec()),
        }
    }
}

// ----------------------------------------------------------------- Result

#[pyclass(module = "pyta._pyta", frozen)]
struct AssignResult {
    classes: Vec<core::ClassResult>,
    link_volume: Vec<f64>,
    turn_volume: Vec<f64>,
    iterations: usize,
    relative_gap: f64,
    converged: bool,
    sweep_gaps: Vec<f64>,
    objective: Option<f64>,
    times: core::PhaseTimes,
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

    /// Beckmann objective; only defined for a single class with PCE 1, otherwise `None`.
    #[getter]
    fn objective(&self) -> Option<f64> {
        self.objective
    }

    /// Every path of a class as flat arrays: `origin`, `destination`, `flow` (one entry per
    /// path), `link_offsets` (one more entry) and `links` (all link ids back to back).
    #[pyo3(signature = (class_index=0))]
    fn paths<'py>(&self, py: Python<'py>, class_index: usize) -> PyResult<Bound<'py, PyDict>> {
        let class = self.class(class_index)?;
        let (mut origin, mut dest, mut flow) = (Vec::new(), Vec::new(), Vec::new());
        let mut offsets: Vec<i64> = vec![0];
        let mut links: Vec<u32> = Vec::new();
        for (o, d, path_links, f) in class.paths.iter() {
            origin.push(o);
            dest.push(d);
            flow.push(f);
            links.extend_from_slice(path_links);
            offsets.push(links.len() as i64);
        }
        let out = PyDict::new(py);
        out.set_item("origin", origin.into_pyarray(py))?;
        out.set_item("destination", dest.into_pyarray(py))?;
        out.set_item("flow", flow.into_pyarray(py))?;
        out.set_item("link_offsets", offsets.into_pyarray(py))?;
        out.set_item("links", links.into_pyarray(py))?;
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
#[pyo3(signature = (network, classes, gap=1e-6, max_iter=200, order="fixed", seed=0, chunk_size=0, threads=None))]
fn assign(
    py: Python<'_>,
    network: PyRef<'_, Network>,
    classes: &Bound<'_, PyAny>,
    gap: f64,
    max_iter: usize,
    order: &str,
    seed: u64,
    chunk_size: usize,
    threads: Option<usize>,
) -> PyResult<AssignResult> {
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

    let specs: Vec<core::ClassSpec<'_>> = class_refs
        .iter()
        .zip(&demand_refs)
        .map(|(class, demand)| core::ClassSpec {
            demand: &demand.od,
            pce: class.pce,
            extra_link_cost: class.extra_link_cost.as_deref(),
            allowed_links: class.allowed_links.as_deref(),
        })
        .collect();
    let options = core::GradientProjectionOptions {
        max_iterations: max_iter,
        gap_tolerance: gap,
        origin_order: parse_order(order, seed)?,
        chunk_size,
    };

    let pool = threads
        .map(|n| {
            if n == 0 {
                return Err(msg("threads must be at least 1"));
            }
            rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .map_err(err)
        })
        .transpose()?;

    let net: &Network = &network;
    let solve = || {
        core::gradient_projection_multiclass(
            &net.network,
            &net.turns,
            &net.link_vdf,
            &net.turn_vdf,
            &specs,
            options,
        )
    };
    let solved = py
        .detach(|| match &pool {
            Some(pool) => pool.install(solve),
            None => solve(),
        })
        .map_err(err)?;

    // The Beckmann objective exists only when volumes are plain vehicle flows.
    let objective = (specs.len() == 1 && specs[0].pce == 1.0).then(|| {
        use core::Vdf;
        let penalties: f64 = (0..net.turns.num_turns())
            .map(|t| net.turns.penalty(t) * solved.turn_volume[t])
            .sum();
        net.link_vdf.integral(&solved.link_volume)
            + net.turn_vdf.integral(&solved.turn_volume)
            + penalties
    });
    Ok(AssignResult {
        classes: solved.classes,
        link_volume: solved.link_volume,
        turn_volume: solved.turn_volume,
        iterations: solved.iterations,
        relative_gap: solved.relative_gap,
        converged: solved.converged,
        sweep_gaps: solved.sweep_gaps,
        objective,
        times: solved.times,
    })
}

#[pymodule]
fn _pyta(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Network>()?;
    m.add_class::<Demand>()?;
    m.add_class::<TrafficClass>()?;
    m.add_class::<AssignResult>()?;
    m.add_function(wrap_pyfunction!(assign, m)?)?;
    Ok(())
}
