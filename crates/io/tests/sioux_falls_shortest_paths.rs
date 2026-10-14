//! Checks Dijkstra against a deliberately naive Bellman-Ford on Sioux Falls.

use pytraf_core::dijkstra;
use pytraf_io::tntp::{TntpNetwork, read_net};

fn load() -> TntpNetwork {
    read_net(format!(
        "{}/../../data/tntp/SiouxFalls/SiouxFalls_net.tntp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Reference implementation: relax every link until nothing changes.
/// Slow but too simple to get wrong. Returns (from, to) per link id via the CSR.
fn bellman_ford(net: &TntpNetwork, costs: &[f64], origin: usize) -> Vec<f64> {
    let n = net.network.num_nodes();
    let mut links = Vec::new(); // (from, to, link_id)
    for node in 0..n as u32 {
        for pos in net.network.out_links(node) {
            links.push((
                node as usize,
                net.network.target(pos) as usize,
                net.network.link_id(pos),
            ));
        }
    }
    let mut dist = vec![f64::INFINITY; n];
    dist[origin] = 0.0;
    for _ in 0..n {
        for &(from, to, id) in &links {
            let candidate = dist[from] + costs[id as usize];
            if candidate < dist[to] {
                dist[to] = candidate;
            }
        }
    }
    dist
}

#[test]
fn dijkstra_matches_bellman_ford_from_every_origin() {
    let net = load();
    let costs = &net.free_flow_time;
    for origin in 0..net.network.num_nodes() {
        let tree = dijkstra(&net.network, costs, origin as u32, &net.is_endpoint);
        let expected = bellman_ford(&net, costs, origin);
        for (node, (got, want)) in tree.dist.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-9,
                "origin {origin} node {node}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn extracted_path_cost_equals_distance() {
    let net = load();
    let costs = &net.free_flow_time;
    let tree = dijkstra(&net.network, costs, 0, &net.is_endpoint);
    for dest in 0..net.network.num_nodes() as u32 {
        let path = tree
            .path_to(dest)
            .expect("Sioux Falls is strongly connected");
        let total: f64 = path.iter().map(|&l| costs[l as usize]).sum();
        assert!((total - tree.dist[dest as usize]).abs() < 1e-9);
    }
}

#[test]
fn known_free_flow_distances_from_node_1() {
    // Values computed independently with a plain-Python Dijkstra on the raw file.
    let net = load();
    let tree = dijkstra(&net.network, &net.free_flow_time, 0, &net.is_endpoint);
    assert_eq!(tree.dist[19], 22.0); // node 20
    assert_eq!(tree.dist[12], 11.0); // node 13
    assert_eq!(tree.dist[23], 15.0); // node 24
}
