//! Checks the link-labelled Dijkstra on Sioux Falls.

use pytraf_core::{TurnOptions, TurnTable, dijkstra, dijkstra_turns};
use pytraf_io::tntp::{TntpNetwork, read_net};

fn load() -> TntpNetwork {
    read_net(format!(
        "{}/../../data/tntp/SiouxFalls/SiouxFalls_net.tntp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// With every turn free, turn-aware and node-based searches must agree, whether
/// or not U-turns exist (a U-turn can never shorten a path with non-negative costs).
#[test]
fn free_turns_match_node_based_dijkstra() {
    let net = load();
    let costs = &net.free_flow_time;
    for allow_u_turns in [false, true] {
        let options = TurnOptions { allow_u_turns };
        let turns = TurnTable::build(&net.network, &net.is_endpoint, &[], options).unwrap();
        let turn_costs = vec![0.0; turns.num_turns()];
        for origin in 0..net.network.num_nodes() as u32 {
            let node_based = dijkstra(&net.network, costs, origin, &net.is_endpoint);
            let tree = dijkstra_turns(&net.network, &turns, costs, &turn_costs, origin);
            for node in 0..net.network.num_nodes() {
                let (got, want) = (tree.node_dist(node as u32), node_based.dist[node]);
                assert!(
                    (got - want).abs() < 1e-9,
                    "origin {origin} node {node}: {got} vs {want}"
                );
            }
        }
    }
}

/// Reference: relax every turn repeatedly until nothing changes.
fn reference_labels(
    net: &TntpNetwork,
    turns: &TurnTable,
    turn_costs: &[f64],
    origin: u32,
) -> Vec<f64> {
    let costs = &net.free_flow_time;
    let mut label = vec![f64::INFINITY; net.network.num_links()];
    for pos in net.network.out_links(origin) {
        let link = net.network.link_id(pos) as usize;
        label[link] = costs[link];
    }
    for _ in 0..net.network.num_links() {
        for (t, &turn_cost) in turn_costs.iter().enumerate() {
            let (e, f) = (turns.from_link(t) as usize, turns.to_link(t) as usize);
            let candidate = label[e] + turn_cost + costs[f];
            if candidate < label[f] {
                label[f] = candidate;
            }
        }
    }
    label
}

#[test]
fn nonzero_turn_costs_match_reference_and_paths_add_up() {
    let net = load();
    let costs = &net.free_flow_time;
    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    // Deterministic, uneven turn delays between 0 and 8.
    let turn_costs: Vec<f64> = (0..turns.num_turns())
        .map(|t| ((t * 7) % 9) as f64)
        .collect();

    for origin in 0..net.network.num_nodes() as u32 {
        let tree = dijkstra_turns(&net.network, &turns, costs, &turn_costs, origin);
        let want = reference_labels(&net, &turns, &turn_costs, origin);
        for link in 0..net.network.num_links() as u32 {
            let (a, b) = (tree.label(link), want[link as usize]);
            assert!(
                a == b || (a - b).abs() < 1e-9,
                "origin {origin} link {link}: {a} vs {b}"
            );
        }

        // The path to every node costs exactly what node_dist says.
        for node in 0..net.network.num_nodes() as u32 {
            let path = tree
                .path_to(node)
                .expect("Sioux Falls is strongly connected");
            let turn_ids = turns
                .path_turns(&path)
                .expect("consecutive links must be joined by a turn");
            assert_eq!(turn_ids.len(), path.len().saturating_sub(1));
            // First link + turn ids reproduce the path.
            for (i, &t) in turn_ids.iter().enumerate() {
                assert_eq!(turns.from_link(t), path[i]);
                assert_eq!(turns.to_link(t), path[i + 1]);
            }
            let link_total: f64 = path.iter().map(|&l| costs[l as usize]).sum();
            let turn_total: f64 = turn_ids.iter().map(|&t| turn_costs[t]).sum();
            let total = link_total + turn_total;
            assert!(
                (total - tree.node_dist(node)).abs() < 1e-9,
                "origin {origin} node {node}"
            );
        }
    }
}
