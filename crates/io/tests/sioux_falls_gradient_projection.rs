//! Gradient projection on Sioux Falls against the published equilibrium.
//!
//! Observed in a release build with the fixed origin order (relative gap -> sweeps, worst link error):
//!   1e-4 ->  15, 75.7 veh
//!   1e-6 ->  54,  1.3 veh
//!   1e-8 ->  98, 0.014 veh
//! Frank-Wolfe needs 1 063 iterations for 1e-4 and 9 825 for 1e-5.

use pyta_core::{
    Bpr, GradientProjectionOptions, GradientProjectionResult, OdDemand, OriginOrder, TurnOptions,
    TurnTable, Vdf, gradient_projection,
};
use pyta_io::tntp::{TntpNetwork, read_flows, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

struct Case {
    network: TntpNetwork,
    demand: OdDemand,
    turns: TurnTable,
    vdf: Bpr,
    published: Vec<f64>,
}

fn load() -> Case {
    let network = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    let published = read_flows(data("SiouxFalls_flow.tntp"), &network)
        .unwrap()
        .volume;
    let demand =
        OdDemand::from_triples(24, &trips.origins, &trips.destinations, &trips.flows).unwrap();
    let turns = TurnTable::build(
        &network.network,
        &network.is_endpoint,
        &[],
        TurnOptions::default(),
    )
    .unwrap();
    let vdf = Bpr::new(
        network.free_flow_time.clone(),
        network.capacity.clone(),
        network.b.clone(),
        network.power.clone(),
    )
    .unwrap();
    Case {
        network,
        demand,
        turns,
        vdf,
        published,
    }
}

fn run(case: &Case, gap_tolerance: f64, origin_order: OriginOrder) -> GradientProjectionResult {
    let options = GradientProjectionOptions {
        max_iterations: 500,
        gap_tolerance,
        origin_order,
        ..Default::default()
    };
    gradient_projection(
        &case.network.network,
        &case.turns,
        &case.vdf,
        &case.demand,
        options,
    )
    .unwrap()
}

fn worst_error(result: &GradientProjectionResult, case: &Case) -> f64 {
    result
        .link_flow
        .iter()
        .zip(&case.published)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max)
}

#[test]
fn reaches_the_published_equilibrium() {
    let case = load();
    let published_objective = case.vdf.integral(&case.published);

    let r = run(&case, 1e-6, OriginOrder::Fixed);
    assert!(r.converged && r.relative_gap <= 1e-6);
    assert!(r.iterations < 100, "{} sweeps", r.iterations);
    let excess = (r.objective - published_objective) / published_objective;
    assert!(excess > -1e-9 && excess < 1e-7, "objective excess {excess}");
    assert!(
        worst_error(&r, &case) < 5.0,
        "worst link error {}",
        worst_error(&r, &case)
    );

    let tight = run(&case, 1e-8, OriginOrder::Fixed);
    assert!(tight.converged);
    assert!(
        worst_error(&tight, &case) < 0.1,
        "worst link error {}",
        worst_error(&tight, &case)
    );
}

#[test]
fn path_flows_are_consistent_with_demand_and_link_flows() {
    let case = load();
    let r = run(&case, 1e-6, OriginOrder::Fixed);
    let net = &case.network.network;

    let mut per_od = std::collections::HashMap::<(u32, u32), f64>::new();
    let mut link_flow = vec![0.0; net.num_links()];
    for (origin, dest, links, flow) in r.paths.iter() {
        assert!(flow > 0.0);
        *per_od.entry((origin, dest)).or_default() += flow;
        assert_eq!(net.from_node(links[0]), origin);
        assert_eq!(net.to_node(*links.last().unwrap()), dest);
        for w in links.windows(2) {
            assert_eq!(net.to_node(w[0]), net.from_node(w[1]));
        }
        for &a in links {
            link_flow[a as usize] += flow;
        }
    }
    let mut total = 0.0;
    for origin in 0..24u32 {
        let (dests, qs) = case.demand.from_origin(origin);
        for (&d, &q) in dests.iter().zip(qs) {
            if q > 0.0 && d != origin {
                assert!((per_od[&(origin, d)] - q).abs() < 1e-6, "OD {origin}->{d}");
                total += q;
            }
        }
    }
    assert_eq!(total, 360_600.0);
    for (a, b) in link_flow.iter().zip(&r.link_flow) {
        assert!((a - b).abs() < 1e-6 * b.max(1.0));
    }
}

#[test]
fn every_origin_order_reaches_the_same_equilibrium() {
    let case = load();
    for order in [
        OriginOrder::Fixed,
        OriginOrder::Shuffle { seed: 1 },
        OriginOrder::Shuffle { seed: 2 },
        OriginOrder::Alternate,
        OriginOrder::LargestGapFirst,
        OriginOrder::LargestDemandFirst,
        OriginOrder::SmallestDemandFirst,
    ] {
        let r = run(&case, 1e-6, order);
        assert!(r.converged, "{order:?} did not converge");
        assert!(
            worst_error(&r, &case) < 5.0,
            "{order:?}: worst error {}",
            worst_error(&r, &case)
        );
    }
}
