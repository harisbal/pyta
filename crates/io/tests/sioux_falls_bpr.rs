//! The published equilibrium file lists each link's volume and cost. Our BPR,
//! built from the network file, must reproduce those costs from those volumes.

use pytraf_core::{Bpr, Vdf};
use pytraf_io::tntp::{TntpNetwork, read_flows, read_net};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn bpr(net: &TntpNetwork) -> Bpr {
    Bpr::new(
        net.free_flow_time.clone(),
        net.capacity.clone(),
        net.b.clone(),
        net.power.clone(),
    )
    .unwrap()
}

#[test]
fn bpr_reproduces_published_equilibrium_costs() {
    let net = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let published = read_flows(data("SiouxFalls_flow.tntp"), &net).unwrap();
    let mut costs = vec![0.0; 76];
    bpr(&net).cost(&published.volume, &mut costs);
    for (link, &want) in published.cost.iter().enumerate() {
        assert!(
            (costs[link] - want).abs() < 1e-9 * want,
            "link {link}: {} vs published {want}",
            costs[link]
        );
    }
}

#[test]
fn zero_flow_costs_free_flow_time() {
    let net = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let mut costs = vec![0.0; 76];
    bpr(&net).cost(&vec![0.0; 76], &mut costs);
    assert_eq!(costs, net.free_flow_time);
}
