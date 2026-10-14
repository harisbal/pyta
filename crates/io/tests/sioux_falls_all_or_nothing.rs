//! All-or-nothing assignment on Sioux Falls.
//!
//! Free-flow times are integers, so many shortest paths tie and the exact link
//! flows depend on tie-breaking. These tests check properties that do not:
//! flow conservation and total cost.

use pytraf_core::{OdDemand, TurnOptions, TurnTable, all_or_nothing, dijkstra_turns};
use pytraf_io::tntp::{TntpNetwork, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn load() -> (TntpNetwork, OdDemand) {
    let net = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    // TNTP zone i is node i, so zone ids are already node ids.
    let demand = OdDemand::from_triples(
        net.network.num_nodes(),
        &trips.origins,
        &trips.destinations,
        &trips.flows,
    )
    .unwrap();
    (net, demand)
}

#[test]
fn conserves_flow_and_matches_independent_total_cost() {
    let (net, demand) = load();
    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let costs = &net.free_flow_time;
    let flows = all_or_nothing(
        &net.network,
        &turns,
        costs,
        &vec![0.0; turns.num_turns()],
        &demand,
    )
    .unwrap();

    // At every node: flow out - flow in = trips produced - trips attracted.
    let n = net.network.num_nodes();
    let mut balance = vec![0.0; n];
    for link in 0..net.network.num_links() as u32 {
        balance[net.network.from_node(link) as usize] += flows.link_flow[link as usize];
        balance[net.network.to_node(link) as usize] -= flows.link_flow[link as usize];
    }
    for origin in 0..n as u32 {
        let (destinations, quantities) = demand.from_origin(origin);
        for (&d, &q) in destinations.iter().zip(quantities) {
            balance[origin as usize] -= q;
            balance[d as usize] += q;
        }
    }
    for (node, b) in balance.iter().enumerate() {
        assert!(b.abs() < 1e-6, "node {node} out of balance by {b}");
    }

    // Total trip-minutes at free flow, computed independently in Python: 3,176,000.
    let total: f64 = flows.link_flow.iter().zip(costs).map(|(f, c)| f * c).sum();
    assert!((total - 3_176_000.0).abs() < 1e-6, "total cost {total}");
}

#[test]
fn total_generalised_cost_with_turn_delays_equals_demand_times_distance() {
    let (net, demand) = load();
    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let costs = &net.free_flow_time;
    let turn_costs: Vec<f64> = (0..turns.num_turns())
        .map(|t| ((t * 7) % 9) as f64)
        .collect();
    let flows = all_or_nothing(&net.network, &turns, costs, &turn_costs, &demand).unwrap();

    let assigned: f64 = flows
        .link_flow
        .iter()
        .zip(costs)
        .map(|(f, c)| f * c)
        .sum::<f64>()
        + flows
            .turn_flow
            .iter()
            .zip(&turn_costs)
            .map(|(f, c)| f * c)
            .sum::<f64>();

    // Reference: sum of demand * shortest generalised distance, straight from the trees.
    let mut expected = 0.0;
    for origin in 0..net.network.num_nodes() as u32 {
        let (destinations, quantities) = demand.from_origin(origin);
        let tree = dijkstra_turns(&net.network, &turns, costs, &turn_costs, origin);
        for (&d, &q) in destinations.iter().zip(quantities) {
            expected += q * tree.node_dist(d);
        }
    }
    assert!(
        (assigned - expected).abs() < 1e-6 * expected,
        "{assigned} vs {expected}"
    );
    assert!(flows.turn_flow.iter().sum::<f64>() > 0.0);
}
