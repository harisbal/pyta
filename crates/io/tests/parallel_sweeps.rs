//! Chunked sweeps (shortest-path searches of several origins in parallel).

use pytraf_core::{
    Bpr, GradientProjectionOptions, GradientProjectionResult, OdDemand, OriginOrder, TurnOptions,
    TurnTable, gradient_projection,
};
use pytraf_io::tntp::{TntpNetwork, read_flows, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

struct Case {
    net: TntpNetwork,
    demand: OdDemand,
    turns: TurnTable,
    vdf: Bpr,
    published: Vec<f64>,
}

fn load() -> Case {
    let net = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    let published = read_flows(data("SiouxFalls_flow.tntp"), &net)
        .unwrap()
        .volume;
    let demand =
        OdDemand::from_triples(24, &trips.origins, &trips.destinations, &trips.flows).unwrap();
    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let vdf = Bpr::new(
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
        vdf,
        published,
    }
}

fn run(case: &Case, chunk_size: usize, gap_tolerance: f64) -> GradientProjectionResult {
    let options = GradientProjectionOptions {
        max_iterations: 1000,
        gap_tolerance,
        origin_order: OriginOrder::Fixed,
        chunk_size,
    };
    gradient_projection(
        &case.net.network,
        &case.turns,
        &case.vdf,
        &case.demand,
        options,
    )
    .unwrap()
}

#[test]
fn every_chunk_size_reaches_the_published_equilibrium() {
    let case = load();
    for chunk_size in [1, 2, 8, 24, 100] {
        let r = run(&case, chunk_size, 1e-8);
        assert!(
            r.converged,
            "chunk size {chunk_size}: gap {}",
            r.relative_gap
        );
        let worst = r
            .link_flow
            .iter()
            .zip(&case.published)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(
            worst < 0.1,
            "chunk size {chunk_size}: worst link error {worst}"
        );
    }
}

#[test]
fn results_do_not_depend_on_the_number_of_threads() {
    let case = load();
    let on_threads = |n: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
            .install(|| run(&case, 8, 1e-7))
    };
    let (one, four) = (on_threads(1), on_threads(4));
    assert_eq!(one.iterations, four.iterations);
    assert_eq!(one.link_flow, four.link_flow); // bit-for-bit
    assert_eq!(one.relative_gap, four.relative_gap);
}

#[test]
fn the_automatic_chunk_size_is_one_on_a_small_network() {
    // 24 origins: about 1/32 of them rounds up to a chunk of 1, the plain sequential sweep.
    let case = load();
    let sequential = run(&case, 1, 1e-6);
    let automatic = run(&case, 0, 1e-6);
    assert_eq!(sequential.link_flow, automatic.link_flow);
    assert_eq!(sequential.iterations, automatic.iterations);
}
