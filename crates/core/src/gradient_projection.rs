//! Path-based user-equilibrium assignment by gradient projection.
//!
//! Every OD pair of every traffic class keeps a small set of paths. A sweep visits the
//! origins one at a time (Gauss-Seidel). For each origin and each class it finds every
//! destination's current cheapest path, adds it to the set if new, and shifts flow from
//! the other paths to the cheapest one with a Newton step. After every single shift, the
//! cost and derivative of the links and turns whose flow changed are refreshed, so the
//! next shift sees the new costs.
//!
//! A path costs the sum of its link costs and turn costs. A link costs its travel time,
//! which depends on the total PCE-weighted volume of all classes, plus the class's own
//! extra cost on that link. A turn costs its fixed penalty from the turn table plus a delay
//! that depends on the turn's own volume (`turn_vdf`), so each turn behaves like a
//! pseudo-link. Interacting junction models (delay depending on other flows) are not
//! supported.
//!
//! With several classes of different PCE the equilibrium problem has no single objective
//! (it is asymmetric), so convergence is judged by the relative gap only.

use std::time::Instant;

use crate::order::{OriginOrder, OriginSequencer};
use crate::path_store::{OriginPaths, PathRef, PathStore};
use rayon::prelude::*;

use crate::{AssignmentError, Network, NoDelay, OdDemand, SolverError, TurnSearch, TurnTable, Vdf};

#[derive(Debug, Clone, Copy)]
pub struct GradientProjectionOptions {
    /// Maximum number of sweeps over all origins.
    pub max_iterations: usize,
    /// Stop when the relative gap is at or below this.
    pub gap_tolerance: f64,
    pub origin_order: OriginOrder,
    /// How many origins have their shortest paths searched in parallel before their flow
    /// shifts are applied, one origin at a time. The searches of a chunk use the costs from
    /// the start of the chunk, which can only make a newly found path slightly stale: it is
    /// still a valid path, and the shifts themselves always use current costs. Larger chunks
    /// use more cores but need a few more sweeps, and the effect depends on how big the chunk
    /// is compared with the number of origins. `1` is the plain sequential sweep and `0`
    /// (the default) chooses automatically: about 1/32 of the origins, between 1 and 64.
    /// Results do not depend on the number of threads.
    pub chunk_size: usize,
}

impl Default for GradientProjectionOptions {
    fn default() -> Self {
        GradientProjectionOptions {
            max_iterations: 200,
            gap_tolerance: 1e-6,
            origin_order: OriginOrder::Fixed,
            chunk_size: 0,
        }
    }
}

/// Where the solver spent its time, in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhaseTimes {
    /// Shortest-path searches during sweeps, including finding which paths are new
    /// (the parallel part of a sweep).
    pub shortest_paths: f64,
    /// Adding shortest paths to the path sets and shifting flow between paths.
    pub flow_updates: f64,
    /// Rebuilding volumes from path flows and refreshing every cost, once per sweep.
    pub sweep_rebuild: f64,
    /// Exact gap checks (one shortest-path search per origin, plus a pass over all paths).
    pub gap_checks: f64,
    /// Building the starting solution.
    pub initial_assignment: f64,
}

/// One traffic class: its own demand and its own view of the network.
#[derive(Debug, Clone, Copy)]
pub struct ClassSpec<'a> {
    pub demand: &'a OdDemand,
    /// How many passenger-car units one vehicle of this class loads on the network
    /// (e.g. 1.0 for cars, 2.5 for trucks). Must be positive.
    pub pce: f64,
    /// Extra generalised cost of using each link, in the same units as travel time
    /// (a toll divided by the class's value of time, a per-length charge, ...).
    /// `None` means no extra cost. Must be non-negative.
    pub extra_link_cost: Option<&'a [f64]>,
    /// Which links this class may use. `None` means all. A turn is usable only if both
    /// of its links are.
    pub allowed_links: Option<&'a [bool]>,
}

impl<'a> ClassSpec<'a> {
    /// A plain class: PCE 1, no extra costs, every link allowed.
    pub fn new(demand: &'a OdDemand) -> ClassSpec<'a> {
        ClassSpec {
            demand,
            pce: 1.0,
            extra_link_cost: None,
            allowed_links: None,
        }
    }

    /// Cost of `link` for this class, given the link's travel time.
    fn link_cost(&self, link: usize, time: f64) -> f64 {
        if self.allowed_links.is_some_and(|allowed| !allowed[link]) {
            return f64::INFINITY; // the shortest-path search treats this as "cannot use"
        }
        time + self.extra_link_cost.map_or(0.0, |extra| extra[link])
    }
}

/// What one class ended up with.
#[derive(Debug)]
pub struct ClassResult {
    /// Vehicles of this class on each link.
    pub link_flow: Vec<f64>,
    /// Vehicles of this class on each turn.
    pub turn_flow: Vec<f64>,
    pub paths: PathStore,
}

#[derive(Debug)]
pub struct MultiClassResult {
    pub classes: Vec<ClassResult>,
    /// PCE-weighted volume on each link: what the travel time depends on.
    pub link_volume: Vec<f64>,
    /// PCE-weighted volume on each turn.
    pub turn_volume: Vec<f64>,
    /// Number of sweeps performed.
    pub iterations: usize,
    /// Exact relative gap, from shortest paths at one consistent cost snapshot.
    pub relative_gap: f64,
    pub converged: bool,
    /// Cheap per-sweep gap estimate, measured while sweeping (costs change during a sweep).
    pub sweep_gaps: Vec<f64>,
    pub times: PhaseTimes,
}

#[derive(Debug)]
pub struct GradientProjectionResult {
    pub link_flow: Vec<f64>,
    pub turn_flow: Vec<f64>,
    pub paths: PathStore,
    /// Number of sweeps performed.
    pub iterations: usize,
    /// Exact relative gap, from shortest paths at one consistent cost snapshot.
    pub relative_gap: f64,
    /// Beckmann objective: link integrals, turn delay integrals and fixed turn penalties.
    pub objective: f64,
    pub converged: bool,
    /// Cheap per-sweep gap estimate, measured while sweeping (costs change during a sweep).
    pub sweep_gaps: Vec<f64>,
    pub times: PhaseTimes,
}

/// Flows of one kind of element (links or turns) with their current costs and cost derivatives.
struct Elements {
    flow: Vec<f64>,
    cost: Vec<f64>,
    derivative: Vec<f64>,
}

impl Elements {
    fn new(len: usize) -> Elements {
        Elements {
            flow: vec![0.0; len],
            cost: vec![0.0; len],
            derivative: vec![0.0; len],
        }
    }
}

/// Links and turns together, the functions that price them, and each class's link costs.
struct State<'a, V: Vdf, T: Vdf> {
    /// `flow` is the PCE-weighted volume and `cost` the travel time.
    links: Elements,
    turns: Elements,
    /// Per class: travel time plus the class's extra cost; infinite where the class may not go.
    class_link_cost: Vec<Vec<f64>>,
    classes: &'a [ClassSpec<'a>],
    vdf: &'a V,
    turn_vdf: &'a T,
    table: &'a TurnTable,
}

impl<V: Vdf, T: Vdf> State<'_, V, T> {
    fn refresh_link(&mut self, link: usize) {
        // Repeated small shifts can leave a flow a hair below zero.
        let flow = self.links.flow[link].max(0.0);
        let (time, derivative) = self.vdf.cost_and_derivative_at(link, flow);
        self.links.cost[link] = time;
        self.links.derivative[link] = derivative;
        for (k, class) in self.classes.iter().enumerate() {
            self.class_link_cost[k][link] = class.link_cost(link, time);
        }
    }

    fn refresh_turn(&mut self, turn: usize) {
        let flow = self.turns.flow[turn].max(0.0);
        let (delay, derivative) = self.turn_vdf.cost_and_derivative_at(turn, flow);
        self.turns.cost[turn] = self.table.penalty(turn) + delay;
        self.turns.derivative[turn] = derivative;
    }

    fn refresh_all(&mut self) {
        for link in 0..self.links.flow.len() {
            self.refresh_link(link);
        }
        for turn in 0..self.turns.flow.len() {
            self.refresh_turn(turn);
        }
    }

    /// Cost of a stretch of links and turns for `class`.
    fn segment_cost(&self, class: usize, links: &[u32], turns: &[u32]) -> f64 {
        let link_cost: f64 = links
            .iter()
            .map(|&a| self.class_link_cost[class][a as usize])
            .sum();
        let turn_cost: f64 = turns.iter().map(|&t| self.turns.cost[t as usize]).sum();
        link_cost + turn_cost
    }
}

/// Drops the longest common prefix and the longest common suffix of `a` and `b`, returning
/// what is left of each. Two paths of one OD pair usually share both ends, so what remains
/// is the stretch where they actually differ.
fn trim_common<'a>(a: &'a [u32], b: &'a [u32]) -> (&'a [u32], &'a [u32]) {
    let n = a.len().min(b.len());
    let mut prefix = 0;
    while prefix < n && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < n - prefix && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix] {
        suffix += 1;
    }
    (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix])
}

/// A set of ids that can be cleared in O(1) by bumping a stamp.
struct Marker {
    stamp: Vec<u32>,
    current: u32,
}

impl Marker {
    fn new(len: usize) -> Marker {
        Marker {
            stamp: vec![0; len],
            current: 0,
        }
    }

    fn clear(&mut self) {
        if self.current == u32::MAX {
            self.stamp.fill(0);
            self.current = 0;
        }
        self.current += 1;
    }

    fn insert(&mut self, id: u32) {
        self.stamp[id as usize] = self.current;
    }

    fn contains(&self, id: u32) -> bool {
        self.stamp[id as usize] == self.current
    }

    /// Replaces the contents with `ids`.
    fn reset_to(&mut self, ids: &[u32]) {
        self.clear();
        for &id in ids {
            self.insert(id);
        }
    }
}

/// Elements whose flow changed since the last refresh, each listed once.
struct Touched {
    seen: Marker,
    list: Vec<u32>,
}

impl Touched {
    fn new(len: usize) -> Touched {
        Touched {
            seen: Marker::new(len),
            list: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.seen.clear();
        self.list.clear();
    }

    fn add(&mut self, id: u32) {
        if !self.seen.contains(id) {
            self.seen.insert(id);
            self.list.push(id);
        }
    }
}

/// Scratch space for one kind of element, reused across OD pairs.
struct KindWork {
    in_cheapest: Marker,
    in_other: Marker,
    touched: Touched,
}

impl KindWork {
    fn new(len: usize) -> KindWork {
        KindWork {
            in_cheapest: Marker::new(len),
            in_other: Marker::new(len),
            touched: Touched::new(len),
        }
    }
}

struct Work {
    path_cost: Vec<f64>,
    links: KindWork,
    turns: KindWork,
}

/// Sum of `derivative` over the ids that are not in `shared`.
fn slope_outside(ids: &[u32], shared: &Marker, derivative: &[f64]) -> f64 {
    ids.iter()
        .filter(|&&a| !shared.contains(a))
        .map(|&a| derivative[a as usize])
        .sum()
}

/// Adds `amount` to the volume of every id not in `shared`, remembering which changed.
fn shift(ids: &[u32], shared: &Marker, volume: &mut [f64], touched: &mut Touched, amount: f64) {
    for &a in ids {
        if !shared.contains(a) {
            volume[a as usize] += amount;
            touched.add(a);
        }
    }
}

/// Recomputes cost and derivative of everything whose flow changed, then forgets the list.
fn refresh_touched<V: Vdf, T: Vdf>(state: &mut State<V, T>, work: &mut Work) {
    for &a in &work.links.touched.list {
        state.refresh_link(a as usize);
    }
    for &t in &work.turns.touched.list {
        state.refresh_turn(t as usize);
    }
    work.links.touched.clear();
    work.turns.touched.clear();
}

/// Shifts flow among the paths of one OD pair of `class` and returns its `excess`: the cost
/// of its flow above the cheapest path's, measured before the shifts.
///
/// `reference` is the index of the path the shortest-path search just found. It is almost
/// always the cheapest, so path costs are taken relative to it, and only on the stretches
/// where paths differ: a stretch shared by two paths cancels out of every comparison and of
/// every shift.
fn equilibrate_od<V: Vdf, T: Vdf>(
    block: &mut OriginPaths,
    od: usize,
    class: usize,
    reference: usize,
    state: &mut State<V, T>,
    work: &mut Work,
) -> f64 {
    let pce = state.classes[class].pce;
    let OriginPaths {
        links: link_arena,
        turns: turn_arena,
        ods,
        ..
    } = block;
    let paths = &mut ods[od].paths;
    if paths.len() == 1 {
        return 0.0;
    }
    let links_of = |p: &PathRef| &link_arena[p.start as usize..(p.start + p.len) as usize];
    let turns_of = |p: &PathRef| {
        let end = p.turn_start + p.len.saturating_sub(1);
        &turn_arena[p.turn_start as usize..end as usize]
    };

    // Cost of each path minus the reference path's, from the differing stretches only.
    let (ref_links, ref_turns) = (links_of(&paths[reference]), turns_of(&paths[reference]));
    work.path_cost.clear();
    for (i, p) in paths.iter().enumerate() {
        if i == reference {
            work.path_cost.push(0.0);
            continue;
        }
        let (links, ref_l) = trim_common(links_of(p), ref_links);
        let (turns, ref_t) = trim_common(turns_of(p), ref_turns);
        work.path_cost.push(
            state.segment_cost(class, links, turns) - state.segment_cost(class, ref_l, ref_t),
        );
    }
    let cheapest = (0..paths.len())
        .min_by(|&a, &b| work.path_cost[a].total_cmp(&work.path_cost[b]))
        .expect("an OD pair always has a path");
    let base = work.path_cost[cheapest];
    let excess: f64 = paths
        .iter()
        .zip(&work.path_cost)
        .map(|(p, &cost)| p.flow * (cost - base))
        .sum();

    let (cheap_links, cheap_turns) = (links_of(&paths[cheapest]), turns_of(&paths[cheapest]));
    work.links.touched.clear();
    work.turns.touched.clear();

    let mut moved = 0.0;
    let mut costs_unchanged = true;
    for (i, path) in paths.iter_mut().enumerate() {
        if i == cheapest || path.flow <= 0.0 {
            continue;
        }
        // Only the stretches where this path and the cheapest differ matter.
        let (links, cheap_l) = trim_common(links_of(path), cheap_links);
        let (turns, cheap_t) = trim_common(turns_of(path), cheap_turns);
        // Until the first shift, costs are exactly what the pricing pass above saw, so its
        // result is reused. After a shift, price both stretches afresh: shifting several
        // paths from one cost snapshot overshoots when they load the same links.
        let gap = if costs_unchanged {
            work.path_cost[i] - base
        } else {
            state.segment_cost(class, links, turns) - state.segment_cost(class, cheap_l, cheap_t)
        };
        if gap <= 0.0 {
            continue;
        }
        work.links.in_cheapest.reset_to(cheap_l);
        work.links.in_other.reset_to(links);
        work.turns.in_cheapest.reset_to(cheap_t);
        work.turns.in_other.reset_to(turns);

        // Newton step: the cost gap over how fast it changes per vehicle shifted. One
        // vehicle of this class changes a volume by `pce`, so the slope of the links and
        // turns the two paths do not share is scaled by it.
        let slope = pce
            * (slope_outside(links, &work.links.in_cheapest, &state.links.derivative)
                + slope_outside(cheap_l, &work.links.in_other, &state.links.derivative)
                + slope_outside(turns, &work.turns.in_cheapest, &state.turns.derivative)
                + slope_outside(cheap_t, &work.turns.in_other, &state.turns.derivative));
        let flow = path.flow;
        let delta = if slope > 0.0 {
            (gap / slope).min(flow)
        } else {
            flow
        };

        let amount = pce * delta;
        let w = &mut work.links;
        shift(
            links,
            &w.in_cheapest,
            &mut state.links.flow,
            &mut w.touched,
            -amount,
        );
        shift(
            cheap_l,
            &w.in_other,
            &mut state.links.flow,
            &mut w.touched,
            amount,
        );
        let w = &mut work.turns;
        shift(
            turns,
            &w.in_cheapest,
            &mut state.turns.flow,
            &mut w.touched,
            -amount,
        );
        shift(
            cheap_t,
            &w.in_other,
            &mut state.turns.flow,
            &mut w.touched,
            amount,
        );

        costs_unchanged = false;
        path.flow -= delta;
        moved += delta;
        refresh_touched(state, work);
    }
    paths[cheapest].flow += moved;
    block.remove_empty_paths(od);
    excess
}

/// Vehicle flows of one class on links and turns.
struct ClassFlows {
    link: Vec<f64>,
    turn: Vec<f64>,
}

/// Rebuilds every class's vehicle flows and the PCE-weighted volumes from path flows, which
/// removes accumulated rounding drift.
fn recompute_flows(
    stores: &[PathStore],
    classes: &[ClassSpec],
    class_flows: &mut [ClassFlows],
    links: &mut [f64],
    turns: &mut [f64],
) {
    links.fill(0.0);
    turns.fill(0.0);
    for ((store, class), flows) in stores.iter().zip(classes).zip(class_flows.iter_mut()) {
        flows.link.fill(0.0);
        flows.turn.fill(0.0);
        for (_, _, path_links, path_turns, path_flow) in store.iter_with_turns() {
            for &a in path_links {
                flows.link[a as usize] += path_flow;
            }
            for &t in path_turns {
                flows.turn[t as usize] += path_flow;
            }
        }
        for (volume, &flow) in links.iter_mut().zip(&flows.link) {
            *volume += class.pce * flow;
        }
        for (volume, &flow) in turns.iter_mut().zip(&flows.turn) {
            *volume += class.pce * flow;
        }
    }
}

/// Total cost of all vehicles at the current costs: what the relative gap is measured against.
fn total_cost<V: Vdf, T: Vdf>(state: &State<V, T>, class_flows: &[ClassFlows]) -> f64 {
    let mut total = 0.0;
    for (k, flows) in class_flows.iter().enumerate() {
        for (&flow, &cost) in flows.link.iter().zip(&state.class_link_cost[k]) {
            if flow > 0.0 {
                total += flow * cost;
            }
        }
        for (&flow, &cost) in flows.turn.iter().zip(&state.turns.cost) {
            total += flow * cost;
        }
    }
    total
}

/// Relative gap from fresh shortest paths for every OD pair of every class, all at the
/// current costs: `1 - (cost of shortest paths) / (cost of current flows)`.
fn exact_gap<V: Vdf, T: Vdf>(
    network: &Network,
    table: &TurnTable,
    state: &State<V, T>,
    stores: &[PathStore],
) -> f64 {
    // One pair of partial sums per origin, added up in order, so the result does not depend
    // on how the work was split across threads.
    let class_costs = &state.class_link_cost;
    let turn_cost = &state.turns.cost;
    let per_origin: Vec<(f64, f64)> = (0..network.num_nodes())
        .into_par_iter()
        .map_init(
            || TurnSearch::new(network),
            |search, origin| {
                let (mut current, mut shortest) = (0.0, 0.0);
                for (k, store) in stores.iter().enumerate() {
                    let block = &store.origins[origin];
                    if block.ods.is_empty() {
                        continue;
                    }
                    for od in &block.ods {
                        for path in &od.paths {
                            let link_cost: f64 = block
                                .path_links(path)
                                .iter()
                                .map(|&a| class_costs[k][a as usize])
                                .sum();
                            let turn_part: f64 = block
                                .path_turns(path)
                                .iter()
                                .map(|&t| turn_cost[t as usize])
                                .sum();
                            current += path.flow * (link_cost + turn_part);
                        }
                    }
                    search.run(network, table, &class_costs[k], turn_cost, origin as u32);
                    for od in &block.ods {
                        shortest += od.demand * search.node_dist(od.dest);
                    }
                }
                (current, shortest)
            },
        )
        .collect();
    let current: f64 = per_origin.iter().map(|&(c, _)| c).sum();
    let shortest: f64 = per_origin.iter().map(|&(_, s)| s).sum();
    if current <= 0.0 {
        return 0.0;
    }
    ((current - shortest) / current).max(0.0)
}

/// The starting paths of one origin: every OD pair's whole demand on its zero-flow shortest
/// path. Returns one block per class.
fn initial_origin(
    network: &Network,
    table: &TurnTable,
    classes: &[ClassSpec],
    class_costs: &[Vec<f64>],
    turn_cost: &[f64],
    origin: u32,
    search: &mut TurnSearch,
) -> Result<Vec<OriginPaths>, SolverError> {
    let (mut path, mut path_turns) = (Vec::new(), Vec::new());
    let mut blocks = Vec::with_capacity(classes.len());
    for (k, class) in classes.iter().enumerate() {
        let mut block = OriginPaths::default();
        let (destinations, quantities) = class.demand.from_origin(origin);
        let mut searched = false;
        for (&dest, &q) in destinations.iter().zip(quantities) {
            if dest == origin || q == 0.0 {
                continue;
            }
            if !searched {
                search.run(network, table, &class_costs[k], turn_cost, origin);
                searched = true;
            }
            if !search.path_into(dest, &mut path) {
                return Err(SolverError::ClassUnreachable {
                    class: k,
                    origin,
                    destination: dest,
                });
            }
            assert!(
                table.path_turns_into(&path, &mut path_turns),
                "search paths follow table turns"
            );
            let od = block.add_od(dest, q);
            let p = block.add_path(od, &path, &path_turns);
            block.ods[od].paths[p].flow = q;
        }
        blocks.push(block);
    }
    Ok(blocks)
}

/// Marks "this OD pair's shortest path is not in its path set yet".
const NEW_PATH: u32 = u32::MAX;

/// The shortest path found for one OD pair: either already in the path set (at `existing`)
/// or new, with its ids in the owning [`Discovery`]'s buffers.
struct Found {
    class: u32,
    od: u32,
    existing: u32,
    link_start: u32,
    len: u32,
    turn_start: u32,
}

/// What a shortest-path search from one origin found, for every OD pair of every class,
/// in (class, OD index) order.
#[derive(Default)]
struct Discovery {
    links: Vec<u32>,
    turns: Vec<u32>,
    entries: Vec<Found>,
}

/// Per-thread scratch space for [`discover`].
struct Searcher {
    search: TurnSearch,
    path: Vec<u32>,
    path_turns: Vec<u32>,
}

impl Searcher {
    fn new(network: &Network) -> Searcher {
        Searcher {
            search: TurnSearch::new(network),
            path: Vec::new(),
            path_turns: Vec::new(),
        }
    }
}

/// Searches from `origin` for every class and notes each destination's shortest path, and
/// whether its OD pair's path set already has it. Reads the stores; changes nothing.
fn discover(
    network: &Network,
    table: &TurnTable,
    class_costs: &[Vec<f64>],
    turn_cost: &[f64],
    stores: &[PathStore],
    origin: u32,
    searcher: &mut Searcher,
) -> Result<Discovery, SolverError> {
    let Searcher {
        search,
        path,
        path_turns,
    } = searcher;
    let mut found = Discovery::default();
    for (k, store) in stores.iter().enumerate() {
        let block = &store.origins[origin as usize];
        if block.ods.is_empty() {
            continue;
        }
        search.run(network, table, &class_costs[k], turn_cost, origin);
        for (od, pair) in block.ods.iter().enumerate() {
            if !search.path_into(pair.dest, path) {
                return Err(SolverError::ClassUnreachable {
                    class: k,
                    origin,
                    destination: pair.dest,
                });
            }
            let mut entry = Found {
                class: k as u32,
                od: od as u32,
                existing: NEW_PATH,
                link_start: 0,
                len: 0,
                turn_start: 0,
            };
            match block.find_path(od, path) {
                Some(index) => entry.existing = index as u32,
                None => {
                    assert!(
                        table.path_turns_into(path, path_turns),
                        "search paths follow table turns"
                    );
                    entry.link_start = found.links.len() as u32;
                    entry.len = path.len() as u32;
                    entry.turn_start = found.turns.len() as u32;
                    found.links.extend_from_slice(path);
                    found.turns.extend_from_slice(path_turns);
                }
            }
            found.entries.push(entry);
        }
    }
    Ok(found)
}

fn validate_classes(network: &Network, classes: &[ClassSpec]) -> Result<(), SolverError> {
    let (nl, nn) = (network.num_links(), network.num_nodes());
    let bad = |class, reason| Err(SolverError::InvalidClass { class, reason });
    for (k, c) in classes.iter().enumerate() {
        if !(c.pce.is_finite() && c.pce > 0.0) {
            return bad(k, "pce must be positive and finite");
        }
        if c.demand.num_nodes() != nn {
            return bad(k, "demand was built for a different number of nodes");
        }
        if let Some(extra) = c.extra_link_cost {
            if extra.len() != nl {
                return bad(k, "extra_link_cost needs one entry per link");
            }
            if extra.iter().any(|x| !x.is_finite() || *x < 0.0) {
                return bad(k, "extra_link_cost must be finite and non-negative");
            }
        }
        if c.allowed_links.is_some_and(|a| a.len() != nl) {
            return bad(k, "allowed_links needs one entry per link");
        }
    }
    Ok(())
}

/// Gradient projection with link delays and fixed turn penalties only, one class.
pub fn gradient_projection<V: Vdf>(
    network: &Network,
    turns: &TurnTable,
    vdf: &V,
    demand: &OdDemand,
    options: GradientProjectionOptions,
) -> Result<GradientProjectionResult, SolverError> {
    let no_delay = NoDelay::new(turns.num_turns());
    gradient_projection_with_turn_delays(network, turns, vdf, &no_delay, demand, options)
}

/// Gradient projection, one class, where turns also have a flow-dependent delay
/// `turn_vdf`, indexed by turn id.
pub fn gradient_projection_with_turn_delays<V: Vdf, T: Vdf>(
    network: &Network,
    turns: &TurnTable,
    vdf: &V,
    turn_vdf: &T,
    demand: &OdDemand,
    options: GradientProjectionOptions,
) -> Result<GradientProjectionResult, SolverError> {
    let classes = [ClassSpec::new(demand)];
    let mut multi = gradient_projection_multiclass(
        network, turns, vdf, turn_vdf, &classes, options,
    )
    .map_err(|e| match e {
        SolverError::ClassUnreachable {
            origin,
            destination,
            ..
        } => SolverError::Assignment(AssignmentError::Unreachable {
            origin,
            destination,
        }),
        other => other,
    })?;
    // One class with PCE 1: volumes are the vehicle flows, so the Beckmann objective applies.
    let penalties: f64 = (0..turns.num_turns())
        .map(|t| turns.penalty(t) * multi.turn_volume[t])
        .sum();
    let objective =
        vdf.integral(&multi.link_volume) + turn_vdf.integral(&multi.turn_volume) + penalties;
    let class = multi.classes.pop().expect("one class");
    Ok(GradientProjectionResult {
        link_flow: class.link_flow,
        turn_flow: class.turn_flow,
        paths: class.paths,
        iterations: multi.iterations,
        relative_gap: multi.relative_gap,
        objective,
        converged: multi.converged,
        sweep_gaps: multi.sweep_gaps,
        times: multi.times,
    })
}

/// Gradient projection for several traffic classes sharing the network.
///
/// Link and turn delays depend on the PCE-weighted total volume. Each class has its own
/// demand, extra link costs and allowed links, and chooses its own paths.
pub fn gradient_projection_multiclass<V: Vdf, T: Vdf>(
    network: &Network,
    turns: &TurnTable,
    vdf: &V,
    turn_vdf: &T,
    classes: &[ClassSpec],
    options: GradientProjectionOptions,
) -> Result<MultiClassResult, SolverError> {
    let (nl, nt, nn) = (network.num_links(), turns.num_turns(), network.num_nodes());
    if vdf.len() != nl {
        return Err(SolverError::WrongNumberOfLinks {
            expected: nl,
            found: vdf.len(),
        });
    }
    if turn_vdf.len() != nt {
        return Err(SolverError::WrongNumberOfTurns {
            expected: nt,
            found: turn_vdf.len(),
        });
    }
    validate_classes(network, classes)?;

    let mut state = State {
        links: Elements::new(nl),
        turns: Elements::new(nt),
        class_link_cost: vec![vec![0.0; nl]; classes.len()],
        classes,
        vdf,
        turn_vdf,
        table: turns,
    };
    state.refresh_all();

    let started = Instant::now();
    let mut times = PhaseTimes::default();

    // Start with every OD pair's whole demand on its zero-flow shortest path.
    let mut stores: Vec<PathStore> = classes.iter().map(|_| PathStore::new(nn)).collect();
    let mut origin_total = vec![0.0; nn];
    let initial: Vec<Result<Vec<OriginPaths>, SolverError>> = (0..nn as u32)
        .into_par_iter()
        .map_init(
            || TurnSearch::new(network),
            |search, origin| {
                initial_origin(
                    network,
                    turns,
                    classes,
                    &state.class_link_cost,
                    &state.turns.cost,
                    origin,
                    search,
                )
            },
        )
        .collect();
    for (origin, blocks) in initial.into_iter().enumerate() {
        for (k, block) in blocks?.into_iter().enumerate() {
            origin_total[origin] += block.ods.iter().map(|od| od.demand).sum::<f64>();
            stores[k].origins[origin] = block;
        }
    }
    let mut class_flows: Vec<ClassFlows> = classes
        .iter()
        .map(|_| ClassFlows {
            link: vec![0.0; nl],
            turn: vec![0.0; nt],
        })
        .collect();
    recompute_flows(
        &stores,
        classes,
        &mut class_flows,
        &mut state.links.flow,
        &mut state.turns.flow,
    );
    state.refresh_all();
    times.initial_assignment = started.elapsed().as_secs_f64();

    let origin_demand: Vec<(u32, f64)> = (0..nn as u32)
        .filter(|&o| origin_total[o as usize] > 0.0)
        .map(|o| (o, origin_total[o as usize]))
        .collect();
    let has_demand = !origin_demand.is_empty();
    let num_origins = origin_demand.len();
    let mut sequencer = OriginSequencer::new(options.origin_order, nn, origin_demand);
    let mut work = Work {
        path_cost: Vec::new(),
        links: KindWork::new(nl),
        turns: KindWork::new(nt),
    };
    let mut sweep_gaps = Vec::new();
    let mut iterations = 0;
    let chunk_size = match options.chunk_size {
        0 => (num_origins / 32).clamp(1, 64),
        n => n,
    };
    let mut main_searcher = Searcher::new(network);
    // How much the cheap per-sweep estimate has understated the exact gap so far. An exact
    // check costs about a sweep's worth of shortest-path searches, so it is only run once the
    // estimate, scaled by this, could pass.
    let mut understatement = 1.0_f64;

    let finish = |state: State<V, T>,
                  stores: Vec<PathStore>,
                  class_flows: Vec<ClassFlows>,
                  iterations,
                  gap: f64,
                  sweep_gaps,
                  times| {
        MultiClassResult {
            classes: stores
                .into_iter()
                .zip(class_flows)
                .map(|(paths, flows)| ClassResult {
                    link_flow: flows.link,
                    turn_flow: flows.turn,
                    paths,
                })
                .collect(),
            link_volume: state.links.flow,
            turn_volume: state.turns.flow,
            iterations,
            relative_gap: gap,
            converged: gap <= options.gap_tolerance,
            sweep_gaps,
            times,
        }
    };

    if !has_demand {
        return Ok(finish(
            state,
            stores,
            class_flows,
            0,
            0.0,
            sweep_gaps,
            times,
        ));
    }

    loop {
        if iterations >= options.max_iterations {
            let checked = Instant::now();
            let gap = exact_gap(network, turns, &state, &stores);
            times.gap_checks += checked.elapsed().as_secs_f64();
            return Ok(finish(
                state,
                stores,
                class_flows,
                iterations,
                gap,
                sweep_gaps,
                times,
            ));
        }

        let mut excess = 0.0;
        let order = sequencer.next_sweep();
        for chunk in order.chunks(chunk_size) {
            // Searches only read costs and path sets, so a chunk's origins can be searched
            // at the same time. The shifts below use the costs as they are at that moment.
            let searched = Instant::now();
            let found: Vec<Result<Discovery, SolverError>> = if chunk.len() > 1 {
                chunk
                    .par_iter()
                    .map_init(
                        || Searcher::new(network),
                        |searcher, &origin| {
                            discover(
                                network,
                                turns,
                                &state.class_link_cost,
                                &state.turns.cost,
                                &stores,
                                origin,
                                searcher,
                            )
                        },
                    )
                    .collect()
            } else {
                chunk
                    .iter()
                    .map(|&origin| {
                        discover(
                            network,
                            turns,
                            &state.class_link_cost,
                            &state.turns.cost,
                            &stores,
                            origin,
                            &mut main_searcher,
                        )
                    })
                    .collect()
            };
            times.shortest_paths += searched.elapsed().as_secs_f64();

            for (&origin, found) in chunk.iter().zip(found) {
                let found = found?;
                let updating = Instant::now();
                let mut origin_excess = 0.0;
                for entry in &found.entries {
                    let k = entry.class as usize;
                    let od = entry.od as usize;
                    let block = &mut stores[k].origins[origin as usize];
                    let reference = if entry.existing == NEW_PATH {
                        let links = &found.links
                            [entry.link_start as usize..(entry.link_start + entry.len) as usize];
                        let turn_ids = &found.turns[entry.turn_start as usize
                            ..(entry.turn_start + entry.len.saturating_sub(1)) as usize];
                        block.add_path(od, links, turn_ids)
                    } else {
                        entry.existing as usize
                    };
                    origin_excess += equilibrate_od(block, od, k, reference, &mut state, &mut work);
                }
                for store in stores.iter_mut() {
                    store.origins[origin as usize].compact_if_needed();
                }
                excess += origin_excess;
                sequencer.record_gap(origin, origin_excess);
                times.flow_updates += updating.elapsed().as_secs_f64();
            }
        }
        iterations += 1;
        let rebuilding = Instant::now();
        recompute_flows(
            &stores,
            classes,
            &mut class_flows,
            &mut state.links.flow,
            &mut state.turns.flow,
        );
        state.refresh_all();
        let total = total_cost(&state, &class_flows);
        times.sweep_rebuild += rebuilding.elapsed().as_secs_f64();
        sweep_gaps.push(if total > 0.0 { excess / total } else { 0.0 });

        let estimate = *sweep_gaps.last().unwrap();
        if estimate * understatement <= options.gap_tolerance {
            // The sweep estimate mixes cost snapshots; confirm with a consistent one.
            let checked = Instant::now();
            let gap = exact_gap(network, turns, &state, &stores);
            times.gap_checks += checked.elapsed().as_secs_f64();
            if estimate > 0.0 {
                understatement = understatement.max(gap / estimate);
            }
            if gap <= options.gap_tolerance {
                return Ok(finish(
                    state,
                    stores,
                    class_flows,
                    iterations,
                    gap,
                    sweep_gaps,
                    times,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Bpr, Edge, FrankWolfeOptions, TurnException, TurnOptions, TurnRule, frank_wolfe,
        frank_wolfe_with_turn_delays,
    };
    use std::collections::HashMap;

    fn bpr(t0: &[f64], cap: &[f64], power: f64) -> Bpr {
        let n = t0.len();
        Bpr::new(t0.to_vec(), cap.to_vec(), vec![1.0; n], vec![power; n]).unwrap()
    }

    fn setup(
        num_nodes: usize,
        edges: &[(u32, u32)],
        od: &[(u32, u32, f64)],
    ) -> (Network, TurnTable, OdDemand) {
        let edges: Vec<Edge> = edges
            .iter()
            .enumerate()
            .map(|(i, &(from, to))| Edge {
                link_id: i as u32,
                from,
                to,
            })
            .collect();
        let net = Network::from_edges(num_nodes, &edges).unwrap();
        let turns =
            TurnTable::build(&net, &vec![false; num_nodes], &[], TurnOptions::default()).unwrap();
        let (o, d, q): (Vec<u32>, Vec<u32>, Vec<f64>) = (
            od.iter().map(|x| x.0).collect(),
            od.iter().map(|x| x.1).collect(),
            od.iter().map(|x| x.2).collect(),
        );
        let demand = OdDemand::from_triples(num_nodes, &o, &d, &q).unwrap();
        (net, turns, demand)
    }

    fn tight(max_iterations: usize) -> GradientProjectionOptions {
        GradientProjectionOptions {
            max_iterations,
            gap_tolerance: 1e-10,
            origin_order: OriginOrder::Fixed,
            ..Default::default()
        }
    }

    /// Path flows add up to each OD's demand, are positive, follow real turns, and
    /// rebuild both the link flows and the turn flows.
    fn check_consistency(
        result: &GradientProjectionResult,
        net: &Network,
        turns: &TurnTable,
        demand: &OdDemand,
    ) {
        let mut per_od: HashMap<(u32, u32), f64> = HashMap::new();
        let mut link_flow = vec![0.0; net.num_links()];
        let mut turn_flow = vec![0.0; turns.num_turns()];
        let mut seen = std::collections::HashSet::new();
        for (origin, dest, links, turn_ids, flow) in result.paths.iter_with_turns() {
            assert!(flow > 0.0, "empty path kept");
            *per_od.entry((origin, dest)).or_default() += flow;
            assert!(
                seen.insert((origin, dest, links.to_vec())),
                "duplicate path"
            );
            assert_eq!(net.from_node(links[0]), origin);
            assert_eq!(net.to_node(*links.last().unwrap()), dest);
            assert_eq!(turn_ids.len(), links.len() - 1);
            for (i, &t) in turn_ids.iter().enumerate() {
                assert_eq!(turns.from_link(t as usize), links[i]);
                assert_eq!(turns.to_link(t as usize), links[i + 1]);
                turn_flow[t as usize] += flow;
            }
            for &a in links {
                link_flow[a as usize] += flow;
            }
        }
        for origin in 0..net.num_nodes() as u32 {
            let (dests, qs) = demand.from_origin(origin);
            let mut wanted: HashMap<u32, f64> = HashMap::new();
            for (&d, &q) in dests.iter().zip(qs) {
                if d != origin && q > 0.0 {
                    *wanted.entry(d).or_default() += q;
                }
            }
            for (d, q) in wanted {
                let got = per_od.get(&(origin, d)).copied().unwrap_or(0.0);
                assert!(
                    (got - q).abs() < 1e-8 * q.max(1.0),
                    "OD {origin}->{d}: {got} vs {q}"
                );
            }
        }
        for (a, b) in link_flow.iter().zip(&result.link_flow) {
            assert!((a - b).abs() < 1e-8 * b.abs().max(1.0), "link {a} vs {b}");
        }
        for (a, b) in turn_flow.iter().zip(&result.turn_flow) {
            assert!((a - b).abs() < 1e-8 * b.abs().max(1.0), "turn {a} vs {b}");
        }
    }

    #[test]
    fn linear_costs_have_a_closed_form_solution() {
        // c1 = 10 + v1, c2 = 20 + v2, demand 30: equal at v1 = 20, v2 = 10.
        let (net, turns, demand) = setup(2, &[(0, 1), (0, 1)], &[(0, 1, 30.0)]);
        let vdf = bpr(&[10.0, 20.0], &[10.0, 20.0], 1.0);
        let r = gradient_projection(&net, &turns, &vdf, &demand, tight(50)).unwrap();
        assert!(r.converged);
        assert!((r.link_flow[0] - 20.0).abs() < 1e-6, "{:?}", r.link_flow);
        assert!((r.link_flow[1] - 10.0).abs() < 1e-6);
        check_consistency(&r, &net, &turns, &demand);
    }

    #[test]
    fn shared_links_are_left_alone_and_result_matches_frank_wolfe() {
        // Two routes 0-1-3 and 0-2-3 that share the tail 3->4, and a second OD pair on the
        // same links: path overlaps and several ODs per origin are exercised.
        let edges = [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)];
        let od = [(0, 4, 40.0), (0, 3, 15.0)];
        let (net, turns, demand) = setup(5, &edges, &od);
        let vdf = bpr(
            &[8.0, 12.0, 9.0, 7.0, 5.0],
            &[20.0, 30.0, 25.0, 15.0, 40.0],
            4.0,
        );

        let gp = gradient_projection(&net, &turns, &vdf, &demand, tight(500)).unwrap();
        assert!(gp.converged);
        check_consistency(&gp, &net, &turns, &demand);

        let fw_opts = FrankWolfeOptions {
            max_iterations: 20_000,
            gap_tolerance: 1e-9,
        };
        let fw = frank_wolfe(&net, &turns, &vdf, &demand, fw_opts).unwrap();
        for (a, b) in gp.link_flow.iter().zip(&fw.link_flow) {
            assert!(
                (a - b).abs() < 1e-3,
                "{:?} vs {:?}",
                gp.link_flow,
                fw.link_flow
            );
        }
        // The tail carries all of OD 0->4, whatever the split before it.
        assert!(gp.link_flow[4] >= 40.0 - 1e-9);
    }

    #[test]
    fn a_banned_turn_forces_the_longer_route_and_no_demand_converges_at_once() {
        // 0: A->J, 1: A->X, 2: X->J, 3: J->K, with A->J->K banned.
        let (net, _, demand) = setup(4, &[(0, 2), (0, 1), (1, 2), (2, 3)], &[(0, 3, 10.0)]);
        let ban = TurnException {
            from_link: 0,
            to_link: 3,
            rule: TurnRule::Ban,
        };
        let turns = TurnTable::build(&net, &[false; 4], &[ban], TurnOptions::default()).unwrap();
        let vdf = bpr(&[5.0, 3.0, 3.0, 1.0], &[10.0; 4], 4.0);
        let r = gradient_projection(&net, &turns, &vdf, &demand, tight(10)).unwrap();
        assert!(r.converged);
        assert_eq!(r.paths.num_paths(), 1);
        assert!((r.link_flow[0]).abs() < 1e-12);
        assert!((r.link_flow[3] - 10.0).abs() < 1e-9);

        let nothing = OdDemand::from_triples(4, &[], &[], &[]).unwrap();
        let r = gradient_projection(&net, &turns, &vdf, &nothing, tight(10)).unwrap();
        assert!(r.converged);
        assert_eq!((r.iterations, r.link_flow), (0, vec![0.0; 4]));
    }

    #[test]
    fn rejects_bad_inputs() {
        let (net, turns, demand) = setup(2, &[(0, 1), (0, 1)], &[(0, 1, 1.0)]);
        let small = Bpr::new(vec![1.0], vec![1.0], vec![0.15], vec![4.0]).unwrap();
        assert_eq!(
            gradient_projection(&net, &turns, &small, &demand, tight(1)).unwrap_err(),
            SolverError::WrongNumberOfLinks {
                expected: 2,
                found: 1
            }
        );
        let vdf = bpr(&[1.0, 1.0], &[1.0, 1.0], 4.0);
        assert_eq!(
            gradient_projection_with_turn_delays(
                &net,
                &turns,
                &vdf,
                &NoDelay::new(3),
                &demand,
                tight(1)
            )
            .unwrap_err(),
            SolverError::WrongNumberOfTurns {
                expected: 0,
                found: 3
            }
        );

        // Node 2 cannot be reached from node 0.
        let (net, turns, d) = setup(3, &[(0, 1)], &[(0, 2, 1.0)]);
        let vdf = bpr(&[1.0], &[1.0], 4.0);
        assert_eq!(
            gradient_projection(&net, &turns, &vdf, &d, tight(1)).unwrap_err(),
            SolverError::Assignment(AssignmentError::Unreachable {
                origin: 0,
                destination: 2
            })
        );
    }

    #[test]
    fn zero_iterations_returns_the_all_or_nothing_start() {
        let (net, turns, demand) = setup(2, &[(0, 1), (0, 1)], &[(0, 1, 30.0)]);
        let vdf = bpr(&[10.0, 20.0], &[10.0, 20.0], 1.0);
        let r = gradient_projection(&net, &turns, &vdf, &demand, tight(0)).unwrap();
        assert_eq!(r.iterations, 0);
        assert_eq!(r.link_flow, [30.0, 0.0]);
        assert!(r.relative_gap > 0.0 && !r.converged);
    }

    // Routes 0->1->3 (links 0, 1) and 0->2->3 (links 2, 3); links cost 3 each.
    // Turn (0,1) is turn 0 with delay 2 + 0.2 v; turn (2,3) is turn 1 with a constant delay of 5.
    // Equal costs when 6 + 2 + 0.2 vA = 6 + 5, so vA = 15.
    fn two_turn_routes(penalty: Option<f64>) -> (Network, TurnTable, Bpr, Bpr, OdDemand) {
        let (net, _, demand) = setup(4, &[(0, 1), (1, 3), (0, 2), (2, 3)], &[(0, 3, 30.0)]);
        let exceptions: Vec<TurnException> = penalty
            .map(|p| TurnException {
                from_link: 2,
                to_link: 3,
                rule: TurnRule::Penalty(p),
            })
            .into_iter()
            .collect();
        let turns =
            TurnTable::build(&net, &[false; 4], &exceptions, TurnOptions::default()).unwrap();
        let links = Bpr::new(vec![3.0; 4], vec![1.0; 4], vec![0.0; 4], vec![1.0; 4]).unwrap();
        let turn_vdf = Bpr::new(
            vec![2.0, 5.0],
            vec![10.0, 1.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
        )
        .unwrap();
        (net, turns, links, turn_vdf, demand)
    }

    #[test]
    fn turn_delays_have_a_closed_form_equilibrium() {
        let (net, turns, links, turn_vdf, demand) = two_turn_routes(None);
        let r = gradient_projection_with_turn_delays(
            &net,
            &turns,
            &links,
            &turn_vdf,
            &demand,
            tight(100),
        )
        .unwrap();
        assert!(r.converged);
        assert!((r.turn_flow[0] - 15.0).abs() < 1e-6, "{:?}", r.turn_flow);
        assert!((r.turn_flow[1] - 15.0).abs() < 1e-6);
        check_consistency(&r, &net, &turns, &demand);
    }

    #[test]
    fn fixed_turn_penalties_change_the_equilibrium() {
        // A penalty of 2 on turn (2,3): 8 + 0.2 vA = 13, so vA = 25 and vB = 5.
        let (net, turns, links, turn_vdf, demand) = two_turn_routes(Some(2.0));
        let r = gradient_projection_with_turn_delays(
            &net,
            &turns,
            &links,
            &turn_vdf,
            &demand,
            tight(100),
        )
        .unwrap();
        assert!(r.converged);
        assert!((r.turn_flow[0] - 25.0).abs() < 1e-6, "{:?}", r.turn_flow);
        assert!((r.turn_flow[1] - 5.0).abs() < 1e-6);
        // Penalties alone (no delay function) are also honoured: with only penalty 10 on
        // turn (0,1)'s competitor, everyone takes the other route.
        let (net, turns, links, _, demand) = two_turn_routes(Some(10.0));
        let r = gradient_projection(&net, &turns, &links, &demand, tight(100)).unwrap();
        assert!(r.converged);
        // Equal link costs and no other delay: route A (turn cost 0) wins outright.
        assert!((r.turn_flow[0] - 30.0).abs() < 1e-9 && r.turn_flow[1].abs() < 1e-9);
    }

    #[test]
    fn turn_costs_match_frank_wolfe_on_overlapping_paths() {
        // The shared-tail network again, now with a delay on each of its four turns.
        let edges = [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)];
        let od = [(0, 4, 40.0), (0, 3, 15.0)];
        let (net, turns, demand) = setup(5, &edges, &od);
        assert_eq!(turns.num_turns(), 4);
        let vdf = bpr(
            &[8.0, 12.0, 9.0, 7.0, 5.0],
            &[20.0, 30.0, 25.0, 15.0, 40.0],
            4.0,
        );
        let turn_vdf = Bpr::new(
            vec![1.0, 3.0, 2.0, 4.0],
            vec![15.0, 10.0, 30.0, 20.0],
            vec![2.0, 1.0, 1.5, 1.0],
            vec![2.0, 3.0, 2.0, 1.0],
        )
        .unwrap();

        let gp = gradient_projection_with_turn_delays(
            &net,
            &turns,
            &vdf,
            &turn_vdf,
            &demand,
            tight(500),
        )
        .unwrap();
        assert!(gp.converged);
        check_consistency(&gp, &net, &turns, &demand);

        // Frank-Wolfe's slow tail leaves it near a 5e-5 gap after 20 000 iterations on this
        // network, while GP reaches machine precision. So GP is the more accurate side: its
        // objective must not exceed Frank-Wolfe's, and the flows agree to Frank-Wolfe's accuracy.
        let fw_opts = FrankWolfeOptions {
            max_iterations: 20_000,
            gap_tolerance: 1e-6,
        };
        let fw =
            frank_wolfe_with_turn_delays(&net, &turns, &vdf, &turn_vdf, &demand, fw_opts).unwrap();
        assert!(
            gp.objective <= fw.objective + 1e-9,
            "{} vs {}",
            gp.objective,
            fw.objective
        );
        assert!((fw.objective - gp.objective) / gp.objective < 1e-4);
        for (a, b) in gp.link_flow.iter().zip(&fw.link_flow) {
            assert!(
                (a - b).abs() < 0.05,
                "links {:?} vs {:?}",
                gp.link_flow,
                fw.link_flow
            );
        }
        for (a, b) in gp.turn_flow.iter().zip(&fw.turn_flow) {
            assert!(
                (a - b).abs() < 0.5,
                "turns {:?} vs {:?}",
                gp.turn_flow,
                fw.turn_flow
            );
        }
    }

    // ---- multi-class ----

    fn od(num_nodes: usize, pairs: &[(u32, u32, f64)]) -> OdDemand {
        let o: Vec<u32> = pairs.iter().map(|x| x.0).collect();
        let d: Vec<u32> = pairs.iter().map(|x| x.1).collect();
        let q: Vec<f64> = pairs.iter().map(|x| x.2).collect();
        OdDemand::from_triples(num_nodes, &o, &d, &q).unwrap()
    }

    fn two_links() -> (Network, TurnTable, Bpr) {
        // c1 = 10 + v1 and c2 = 20 + v2.
        let (net, turns, _) = setup(2, &[(0, 1), (0, 1)], &[(0, 1, 1.0)]);
        (net, turns, bpr(&[10.0, 20.0], &[10.0, 20.0], 1.0))
    }

    fn multiclass(
        net: &Network,
        turns: &TurnTable,
        vdf: &Bpr,
        classes: &[ClassSpec],
    ) -> MultiClassResult {
        let no_delay = NoDelay::new(turns.num_turns());
        gradient_projection_multiclass(net, turns, vdf, &no_delay, classes, tight(500)).unwrap()
    }

    #[test]
    fn a_toll_pushes_one_class_off_the_tolled_link() {
        // A: 10 vehicles, no toll. B: 10 vehicles, +5 on link 0.
        // Equilibrium: A all on link 0 (10, 0); B splits (2.5, 7.5); volumes (12.5, 7.5).
        let (net, turns, vdf) = two_links();
        let (a_demand, b_demand) = (od(2, &[(0, 1, 10.0)]), od(2, &[(0, 1, 10.0)]));
        let toll = [5.0, 0.0];
        let a = ClassSpec::new(&a_demand);
        let b = ClassSpec {
            extra_link_cost: Some(&toll),
            ..ClassSpec::new(&b_demand)
        };
        let r = multiclass(&net, &turns, &vdf, &[a, b]);
        assert!(r.converged);
        let (fa, fb) = (&r.classes[0].link_flow, &r.classes[1].link_flow);
        assert!(
            (fa[0] - 10.0).abs() < 1e-6 && fa[1].abs() < 1e-6,
            "A {fa:?}"
        );
        assert!(
            (fb[0] - 2.5).abs() < 1e-6 && (fb[1] - 7.5).abs() < 1e-6,
            "B {fb:?}"
        );
        assert!((r.link_volume[0] - 12.5).abs() < 1e-6 && (r.link_volume[1] - 7.5).abs() < 1e-6);
    }

    #[test]
    fn pce_scales_the_load_a_vehicle_puts_on_the_network() {
        // 15 trucks of PCE 2 load 30 units, the same as 30 cars: volumes (20, 10).
        let (net, turns, vdf) = two_links();
        let demand = od(2, &[(0, 1, 15.0)]);
        let trucks = ClassSpec {
            pce: 2.0,
            ..ClassSpec::new(&demand)
        };
        let r = multiclass(&net, &turns, &vdf, &[trucks]);
        assert!(r.converged);
        assert!((r.link_volume[0] - 20.0).abs() < 1e-6 && (r.link_volume[1] - 10.0).abs() < 1e-6);
        let flow = &r.classes[0].link_flow;
        assert!(
            (flow[0] - 10.0).abs() < 1e-6 && (flow[1] - 5.0).abs() < 1e-6,
            "{flow:?}"
        );
    }

    #[test]
    fn identical_classes_behave_like_one_class_with_their_combined_demand() {
        let (net, turns, vdf) = two_links();
        let (d1, d2) = (od(2, &[(0, 1, 12.0)]), od(2, &[(0, 1, 18.0)]));
        let r = multiclass(
            &net,
            &turns,
            &vdf,
            &[ClassSpec::new(&d1), ClassSpec::new(&d2)],
        );
        assert!(r.converged);
        assert!((r.link_volume[0] - 20.0).abs() < 1e-6 && (r.link_volume[1] - 10.0).abs() < 1e-6);
        for (k, want) in [12.0, 18.0].into_iter().enumerate() {
            let total: f64 = r.classes[k].link_flow.iter().sum();
            assert!((total - want).abs() < 1e-8, "class {k} carries {total}");
        }
    }

    #[test]
    fn a_class_cannot_use_links_it_is_banned_from() {
        // B may only use link 1. A: 25 cars. B: 10 trucks of PCE 1.
        // v2 = 10 + a2: c1 = 10 + a1, c2 = 30 + a2, so a1 = 22.5, a2 = 2.5.
        let (net, turns, vdf) = two_links();
        let (a_demand, b_demand) = (od(2, &[(0, 1, 25.0)]), od(2, &[(0, 1, 10.0)]));
        let allowed = [false, true];
        let a = ClassSpec::new(&a_demand);
        let b = ClassSpec {
            allowed_links: Some(&allowed),
            ..ClassSpec::new(&b_demand)
        };
        let r = multiclass(&net, &turns, &vdf, &[a, b]);
        assert!(r.converged);
        assert_eq!(r.classes[1].link_flow[0], 0.0);
        assert!((r.classes[1].link_flow[1] - 10.0).abs() < 1e-9);
        assert!(
            (r.classes[0].link_flow[0] - 22.5).abs() < 1e-6,
            "{:?}",
            r.classes[0].link_flow
        );
        assert!((r.classes[0].link_flow[1] - 2.5).abs() < 1e-6);
    }

    #[test]
    fn single_class_through_the_multiclass_api_matches_the_wrapper() {
        let edges = [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)];
        let (net, turns, demand) = setup(5, &edges, &[(0, 4, 40.0), (0, 3, 15.0)]);
        let vdf = bpr(
            &[8.0, 12.0, 9.0, 7.0, 5.0],
            &[20.0, 30.0, 25.0, 15.0, 40.0],
            4.0,
        );
        let single = gradient_projection(&net, &turns, &vdf, &demand, tight(500)).unwrap();
        let multi = multiclass(&net, &turns, &vdf, &[ClassSpec::new(&demand)]);
        assert_eq!(single.link_flow, multi.classes[0].link_flow);
        assert_eq!(single.iterations, multi.iterations);
    }

    #[test]
    fn rejects_bad_classes() {
        let (net, turns, vdf) = two_links();
        let demand = od(2, &[(0, 1, 5.0)]);
        let no_delay = NoDelay::new(0);
        let run = |class: ClassSpec| {
            gradient_projection_multiclass(&net, &turns, &vdf, &no_delay, &[class], tight(5))
                .unwrap_err()
        };
        let invalid = |reason| SolverError::InvalidClass { class: 0, reason };

        let zero_pce = ClassSpec {
            pce: 0.0,
            ..ClassSpec::new(&demand)
        };
        assert_eq!(run(zero_pce), invalid("pce must be positive and finite"));
        let short = [1.0];
        let extra = ClassSpec {
            extra_link_cost: Some(&short),
            ..ClassSpec::new(&demand)
        };
        assert_eq!(
            run(extra),
            invalid("extra_link_cost needs one entry per link")
        );
        let negative = [1.0, -1.0];
        let extra = ClassSpec {
            extra_link_cost: Some(&negative),
            ..ClassSpec::new(&demand)
        };
        assert_eq!(
            run(extra),
            invalid("extra_link_cost must be finite and non-negative")
        );
        let mask = [true];
        let allowed = ClassSpec {
            allowed_links: Some(&mask),
            ..ClassSpec::new(&demand)
        };
        assert_eq!(
            run(allowed),
            invalid("allowed_links needs one entry per link")
        );
        let other_size = od(3, &[(0, 1, 5.0)]);
        assert_eq!(
            run(ClassSpec::new(&other_size)),
            invalid("demand was built for a different number of nodes")
        );

        // Both links banned for a class that has demand.
        let none = [false, false];
        let stuck = ClassSpec {
            allowed_links: Some(&none),
            ..ClassSpec::new(&demand)
        };
        assert_eq!(
            run(stuck),
            SolverError::ClassUnreachable {
                class: 0,
                origin: 0,
                destination: 1
            }
        );
    }
}
