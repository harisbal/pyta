//! Anaheim (with real centroids) and Chicago Sketch: loading, and gradient projection on Anaheim.

use pyta_core::{
    Bpr, GradientProjectionOptions, OdDemand, OriginOrder, TurnOptions, TurnTable, Vdf,
    gradient_projection,
};
use pyta_io::tntp::{TntpNetwork, read_flows, read_net, read_trips};

fn dir(name: &str) -> String {
    format!("{}/../../data/tntp/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn load(name: &str) -> (TntpNetwork, OdDemand) {
    let net = read_net(format!("{}/{name}_net.tntp", dir(name))).unwrap();
    let trips = read_trips(format!("{}/{name}_trips.tntp", dir(name))).unwrap();
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
fn chicago_sketch_loads() {
    let (net, demand) = load("ChicagoSketch");
    assert_eq!(net.num_zones, 387);
    assert_eq!(net.network.num_nodes(), 933);
    assert_eq!(net.network.num_links(), 2950);
    assert!((demand.total() - 1_260_907.44).abs() < 1e-3 * 1260.0);
    read_flows(
        format!("{}/ChicagoSketch_flow.tntp", dir("ChicagoSketch")),
        &net,
    )
    .unwrap();
}

#[test]
fn anaheim_equilibrium_respects_centroids_and_matches_published_objective() {
    let (net, demand) = load("Anaheim");
    assert_eq!(net.num_zones, 38);
    // FIRST THRU NODE 39: nodes 1..=38 are endpoint-only.
    assert_eq!(net.is_endpoint.iter().filter(|&&e| e).count(), 38);
    assert!(net.is_endpoint[..38].iter().all(|&e| e));

    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let vdf = Bpr::new(
        net.free_flow_time.clone(),
        net.capacity.clone(),
        net.b.clone(),
        net.power.clone(),
    )
    .unwrap();
    let options = GradientProjectionOptions {
        max_iterations: 100,
        gap_tolerance: 1e-6,
        origin_order: OriginOrder::Fixed,
        ..Default::default()
    };
    let r = gradient_projection(&net.network, &turns, &vdf, &demand, options).unwrap();
    assert!(r.converged, "gap {}", r.relative_gap);
    assert!(r.iterations < 30, "{} sweeps", r.iterations);

    // No path passes through a centroid: every node strictly inside a path is a through node.
    for (origin, dest, links, _) in r.paths.iter() {
        for &link in &links[..links.len() - 1] {
            let node = net.network.to_node(link) as usize;
            assert!(
                !net.is_endpoint[node],
                "OD {origin}->{dest} passes through centroid {node}"
            );
        }
    }

    // The published solution is itself an equilibrium to high accuracy for Anaheim.
    let published = read_flows(format!("{}/Anaheim_flow.tntp", dir("Anaheim")), &net).unwrap();
    let reference = vdf.integral(&published.volume);
    let excess = (r.objective - reference) / reference;
    assert!(excess > -1e-9 && excess < 1e-7, "objective excess {excess}");
}
