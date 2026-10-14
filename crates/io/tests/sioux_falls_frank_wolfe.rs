//! Frank-Wolfe on Sioux Falls against the published equilibrium.
//!
//! Observed in a release build (relative gap -> iterations, objective, worst link error):
//!   1e-3 ->   128, 4235372, 147 veh
//!   1e-4 -> 1 063, 4231787,  17 veh
//!   1e-5 -> 9 825, 4231384, 1.7 veh
//! The published objective is 4231335.29.

use pytraf_core::{Bpr, FrankWolfeOptions, OdDemand, TurnOptions, TurnTable, Vdf, frank_wolfe};
use pytraf_io::tntp::{read_flows, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

struct Case {
    network: pytraf_io::tntp::TntpNetwork,
    demand: OdDemand,
    turns: TurnTable,
    vdf: Bpr,
    published_volume: Vec<f64>,
}

fn load() -> Case {
    let network = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    let published = read_flows(data("SiouxFalls_flow.tntp"), &network).unwrap();
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
        published_volume: published.volume,
    }
}

fn run(case: &Case, gap_tolerance: f64) -> pytraf_core::FrankWolfeResult {
    let options = FrankWolfeOptions {
        max_iterations: 20_000,
        gap_tolerance,
    };
    frank_wolfe(
        &case.network.network,
        &case.turns,
        &case.vdf,
        &case.demand,
        options,
    )
    .unwrap()
}

#[test]
fn published_objective_is_the_known_value() {
    let case = load();
    let objective = case.vdf.integral(&case.published_volume);
    assert!((objective - 4_231_335.29).abs() < 0.01, "{objective}");
}

#[test]
fn converges_to_the_published_equilibrium() {
    let case = load();
    let published_objective = case.vdf.integral(&case.published_volume);

    let result = run(&case, 1e-4);
    assert!(result.converged);
    assert!(result.relative_gap <= 1e-4);

    // The equilibrium minimises the objective, so FW approaches it from above.
    let excess = (result.objective - published_objective) / published_objective;
    assert!(
        excess > -1e-9,
        "objective below the published optimum by {excess}"
    );
    assert!(
        excess < 2e-4,
        "objective {} too far above {published_objective}",
        result.objective
    );

    // Link flows are unique at equilibrium, so they must match the published ones.
    let worst = result
        .link_flow
        .iter()
        .zip(&case.published_volume)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(worst < 30.0, "worst link flow error {worst} vehicles");
}

#[test]
fn a_tighter_gap_gets_closer_to_the_published_flows() {
    let case = load();
    let error = |tol| {
        run(&case, tol)
            .link_flow
            .iter()
            .zip(&case.published_volume)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    };
    assert!(error(1e-3) > error(1e-4));
}
