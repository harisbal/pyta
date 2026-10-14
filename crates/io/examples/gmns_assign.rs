//! Loads a GMNS dataset and runs gradient projection on a demand file.
//!
//!   cargo run --release -p pytraf-io --example gmns_assign -- <dir> <demand.csv> [tolerance]
//!
//! Unit and column names come from environment variables, since real datasets often
//! need them (the official examples declare miles for lengths that are in feet):
//!   GMNS_LENGTH_UNIT, GMNS_SPEED_UNIT                 e.g. foot, mph
//!   GMNS_ORIGIN, GMNS_DESTINATION, GMNS_FLOW          demand columns, default o_zone_id, d_zone_id, volume
//!   GMNS_ZONES_ARE_ENDPOINTS=1                        zone nodes cannot be passed through
//!   GMNS_SKIP_UNKNOWN_ZONES=1                         drop demand rows whose zone has no node

use std::env;
use std::time::Instant;

use pytraf_core::{
    Bpr, GradientProjectionOptions, OdDemand, TurnOptions, TurnTable, gradient_projection,
};
use pytraf_io::gmns::{DemandOptions, GmnsOptions, read_demand, read_network};

fn main() {
    let mut args = env::args().skip(1);
    let dir = args
        .next()
        .expect("usage: gmns_assign <dir> <demand.csv> [tolerance]");
    let demand_file = args.next().expect("demand file");
    let gap_tolerance: f64 = args.next().map_or(1e-4, |a| a.parse().expect("tolerance"));

    let options = GmnsOptions {
        length_unit: env::var("GMNS_LENGTH_UNIT").ok(),
        speed_unit: env::var("GMNS_SPEED_UNIT").ok(),
        zone_nodes_are_endpoints: env::var("GMNS_ZONES_ARE_ENDPOINTS").is_ok(),
        ..GmnsOptions::default()
    };
    let mut columns = DemandOptions {
        skip_unknown_zones: env::var("GMNS_SKIP_UNKNOWN_ZONES").is_ok(),
        ..DemandOptions::default()
    };
    if let Ok(name) = env::var("GMNS_ORIGIN") {
        columns.origin = name;
    }
    if let Ok(name) = env::var("GMNS_DESTINATION") {
        columns.destination = name;
    }
    if let Ok(name) = env::var("GMNS_FLOW") {
        columns.flow = name;
    }

    let gmns = read_network(&dir, &options).unwrap_or_else(|e| panic!("network: {e}"));
    let gmns_demand =
        read_demand(&demand_file, &gmns, &columns).unwrap_or_else(|e| panic!("demand: {e}"));
    let trips = gmns_demand.demand;
    let demand = OdDemand::from_triples(
        gmns.network.num_nodes(),
        &trips.origins,
        &trips.destinations,
        &trips.flows,
    )
    .expect("demand");
    let turns = TurnTable::build(
        &gmns.network,
        &gmns.is_endpoint,
        &gmns.turn_exceptions,
        TurnOptions::default(),
    )
    .expect("turns");
    let vdf = Bpr::new(
        gmns.free_flow_time.clone(),
        gmns.capacity.clone(),
        gmns.b.clone(),
        gmns.power.clone(),
    )
    .expect("BPR parameters");

    println!(
        "{} zones, {} nodes, {} links ({} reversed copies), {} turns ({} exceptions), {} OD pairs, demand {:.0}",
        gmns.zone_nodes.len(),
        gmns.network.num_nodes(),
        gmns.network.num_links(),
        gmns.is_reverse_copy.iter().filter(|&&r| r).count(),
        turns.num_turns(),
        gmns.turn_exceptions.len(),
        trips.flows.len(),
        demand.total()
    );
    if gmns_demand.skipped_flow > 0.0 {
        println!(
            "skipped {:.0} trips of zones without a node",
            gmns_demand.skipped_flow
        );
    }

    let start = Instant::now();
    let result = gradient_projection(
        &gmns.network,
        &turns,
        &vdf,
        &demand,
        GradientProjectionOptions {
            max_iterations: 1000,
            gap_tolerance,
            ..Default::default()
        },
    )
    .expect("assignment");
    println!(
        "converged={} after {} sweeps, gap {:.2e}, objective {:.1}, {:.2?}",
        result.converged,
        result.iterations,
        result.relative_gap,
        result.objective,
        start.elapsed()
    );
}
