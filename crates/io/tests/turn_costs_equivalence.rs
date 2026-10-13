//! Turn costs checked against a model that has no turns at all.
//!
//! Every link of the real network becomes an edge, every allowed turn becomes a
//! pseudo-edge between the two link edges, and each zone gets a zero-cost source
//! and sink edge to the links leaving and entering it. A link-only solver on this
//! expanded graph is the same equilibrium problem, so its edge flows must equal the
//! link and turn flows of the turn-aware solver on the original network.

use pyta_core::{
    Bpr, Edge, FrankWolfeOptions, GradientProjectionOptions, GradientProjectionResult, Network,
    OdDemand, OriginOrder, TurnException, TurnOptions, TurnRule, TurnTable, Vdf,
    frank_wolfe_with_turn_delays, gradient_projection, gradient_projection_with_turn_delays,
};
use pyta_io::tntp::{TntpNetwork, read_net, read_trips};

struct Case {
    net: TntpNetwork,
    demand: OdDemand,
    turns: TurnTable,
    link_vdf: Bpr,
    turn_vdf: Bpr,
}

/// Synthetic turn delays: every turn gets a BPR delay, and every 17th also a fixed penalty.
fn load(name: &str) -> Case {
    let dir = format!("{}/../../data/tntp/{name}", env!("CARGO_MANIFEST_DIR"));
    let net = read_net(format!("{dir}/{name}_net.tntp")).unwrap();
    let trips = read_trips(format!("{dir}/{name}_trips.tntp")).unwrap();
    let n = net.network.num_nodes();
    let demand =
        OdDemand::from_triples(n, &trips.origins, &trips.destinations, &trips.flows).unwrap();

    let plain =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let penalties: Vec<TurnException> = (0..plain.num_turns())
        .step_by(17)
        .map(|t| TurnException {
            from_link: plain.from_link(t),
            to_link: plain.to_link(t),
            rule: TurnRule::Penalty(1.5),
        })
        .collect();
    let turns = TurnTable::build(
        &net.network,
        &net.is_endpoint,
        &penalties,
        TurnOptions::default(),
    )
    .unwrap();
    assert_eq!(turns.num_turns(), plain.num_turns());

    let nt = turns.num_turns();
    let turn_vdf = Bpr::new(
        (0..nt).map(|t| 0.3 + 0.2 * (t % 4) as f64).collect(),
        (0..nt).map(|t| 2500.0 + 400.0 * (t % 5) as f64).collect(),
        vec![2.0; nt],
        vec![2.0; nt],
    )
    .unwrap();
    let link_vdf = Bpr::new(
        net.free_flow_time.clone(),
        net.capacity.clone(),
        net.b.clone(),
        net.power.clone(),
    )
    .unwrap();
    Case {
        net,
        demand,
        turns,
        link_vdf,
        turn_vdf,
    }
}

/// Link edges, then turn edges, then zero-cost source and sink edges.
struct ExpandedVdf<'a> {
    links: &'a Bpr,
    turns: &'a Bpr,
    table: &'a TurnTable,
    num_links: usize,
    num_turns: usize,
}

impl Vdf for ExpandedVdf<'_> {
    fn len(&self) -> usize {
        self.num_links + self.num_turns + 2 * self.num_links
    }

    fn cost_at(&self, i: usize, flow: f64) -> f64 {
        if i < self.num_links {
            self.links.cost_at(i, flow)
        } else if i < self.num_links + self.num_turns {
            let t = i - self.num_links;
            self.table.penalty(t) + self.turns.cost_at(t, flow)
        } else {
            0.0
        }
    }

    fn derivative_at(&self, i: usize, flow: f64) -> f64 {
        if i < self.num_links {
            self.links.derivative_at(i, flow)
        } else if i < self.num_links + self.num_turns {
            self.turns.derivative_at(i - self.num_links, flow)
        } else {
            0.0
        }
    }

    fn integral(&self, flows: &[f64]) -> f64 {
        let (l, t) = (self.num_links, self.num_turns);
        let penalties: f64 = (0..t).map(|k| self.table.penalty(k) * flows[l + k]).sum();
        self.links.integral(&flows[..l]) + self.turns.integral(&flows[l..l + t]) + penalties
    }
}

/// Solves the expanded model; returns the result with edge flows ordered as above.
fn solve_expanded(case: &Case, gap_tolerance: f64) -> GradientProjectionResult {
    let net = &case.net.network;
    let (l, t, n) = (net.num_links(), case.turns.num_turns(), net.num_nodes());
    // Nodes: link e has start 2e and end 2e+1; zone z has source 2l+2z and sink 2l+2z+1.
    let source = |z: u32| (2 * l) as u32 + 2 * z;
    let sink = |z: u32| source(z) + 1;
    let mut edges = Vec::new();
    let mut add = |from: u32, to: u32| {
        edges.push(Edge {
            link_id: edges.len() as u32,
            from,
            to,
        });
    };
    for e in 0..l as u32 {
        add(2 * e, 2 * e + 1);
    }
    for k in 0..t {
        add(2 * case.turns.from_link(k) + 1, 2 * case.turns.to_link(k));
    }
    for e in 0..l as u32 {
        add(source(net.from_node(e)), 2 * e);
    }
    for e in 0..l as u32 {
        add(2 * e + 1, sink(net.to_node(e)));
    }
    let expanded = Network::from_edges(2 * l + 2 * n, &edges).unwrap();
    let table = TurnTable::build(
        &expanded,
        &vec![false; expanded.num_nodes()],
        &[],
        TurnOptions::default(),
    )
    .unwrap();
    assert_eq!(table.num_turns(), table_turns_of_expanded(&expanded));

    let (mut origins, mut dests, mut flows) = (Vec::new(), Vec::new(), Vec::new());
    for o in 0..n as u32 {
        let (ds, qs) = case.demand.from_origin(o);
        for (&d, &q) in ds.iter().zip(qs) {
            origins.push(source(o));
            dests.push(sink(d));
            flows.push(q);
        }
    }
    let demand = OdDemand::from_triples(expanded.num_nodes(), &origins, &dests, &flows).unwrap();
    let vdf = ExpandedVdf {
        links: &case.link_vdf,
        turns: &case.turn_vdf,
        table: &case.turns,
        num_links: l,
        num_turns: t,
    };
    let options = GradientProjectionOptions {
        max_iterations: 2000,
        gap_tolerance,
        origin_order: OriginOrder::Fixed,
        ..Default::default()
    };
    gradient_projection(&expanded, &table, &vdf, &demand, options).unwrap()
}

/// The expanded graph has no U-turns, so every in-edge/out-edge pair at a node is a turn.
fn table_turns_of_expanded(net: &Network) -> usize {
    let mut in_degree = vec![0usize; net.num_nodes()];
    for e in 0..net.num_links() as u32 {
        in_degree[net.to_node(e) as usize] += 1;
    }
    (0..net.num_nodes() as u32)
        .map(|v| in_degree[v as usize] * net.out_links(v).len())
        .sum()
}

fn solve_original(case: &Case, gap_tolerance: f64) -> GradientProjectionResult {
    let options = GradientProjectionOptions {
        max_iterations: 2000,
        gap_tolerance,
        origin_order: OriginOrder::Fixed,
        ..Default::default()
    };
    gradient_projection_with_turn_delays(
        &case.net.network,
        &case.turns,
        &case.link_vdf,
        &case.turn_vdf,
        &case.demand,
        options,
    )
    .unwrap()
}

fn assert_same_equilibrium(case: &Case, gap_tolerance: f64, flow_tolerance: f64) {
    let direct = solve_original(case, gap_tolerance);
    let expanded = solve_expanded(case, gap_tolerance);
    assert!(direct.converged && expanded.converged);

    let l = case.net.network.num_links();
    let t = case.turns.num_turns();
    let link_diff = (0..l)
        .map(|i| (direct.link_flow[i] - expanded.link_flow[i]).abs())
        .fold(0.0, f64::max);
    let turn_diff = (0..t)
        .map(|i| (direct.turn_flow[i] - expanded.link_flow[l + i]).abs())
        .fold(0.0, f64::max);
    assert!(
        link_diff < flow_tolerance,
        "worst link flow difference {link_diff}"
    );
    assert!(
        turn_diff < flow_tolerance,
        "worst turn flow difference {turn_diff}"
    );
    let objective_diff = (direct.objective - expanded.objective).abs() / direct.objective;
    assert!(
        objective_diff < 1e-8,
        "objective difference {objective_diff}"
    );
}

#[test]
fn sioux_falls_with_turn_costs_matches_the_expanded_model() {
    assert_same_equilibrium(&load("SiouxFalls"), 1e-9, 0.05);
}

#[test]
fn anaheim_with_centroids_and_turn_costs_matches_the_expanded_model() {
    assert_same_equilibrium(&load("Anaheim"), 1e-7, 2.0);
}

#[test]
fn turn_costs_really_change_the_answer() {
    let case = load("SiouxFalls");
    let with_turns = solve_original(&case, 1e-6);
    let options = GradientProjectionOptions {
        max_iterations: 500,
        gap_tolerance: 1e-6,
        origin_order: OriginOrder::Fixed,
        ..Default::default()
    };
    let without = gradient_projection(
        &case.net.network,
        &TurnTable::build(
            &case.net.network,
            &case.net.is_endpoint,
            &[],
            TurnOptions::default(),
        )
        .unwrap(),
        &case.link_vdf,
        &case.demand,
        options,
    )
    .unwrap();
    let moved = with_turns
        .link_flow
        .iter()
        .zip(&without.link_flow)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(moved > 100.0, "turn costs only moved flows by {moved}");
}

#[test]
fn frank_wolfe_with_turn_costs_agrees_with_gradient_projection() {
    let case = load("SiouxFalls");
    let gp = solve_original(&case, 1e-8);
    let options = FrankWolfeOptions {
        max_iterations: 5000,
        gap_tolerance: 1e-4,
    };
    let fw = frank_wolfe_with_turn_delays(
        &case.net.network,
        &case.turns,
        &case.link_vdf,
        &case.turn_vdf,
        &case.demand,
        options,
    )
    .unwrap();
    assert!(fw.converged);
    let excess = (fw.objective - gp.objective) / gp.objective;
    assert!(
        excess > -1e-9 && excess < 3e-4,
        "FW objective excess {excess}"
    );
}
