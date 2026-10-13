//! Compares origin orders for gradient projection on a TNTP benchmark network.
//!
//!   cargo run --release -p pyta-io --example order_benchmark -- SiouxFalls
//!   cargo run --release -p pyta-io --example order_benchmark -- Anaheim 1e-4 1e-6
//!
//! The network name is a folder under `data/tntp`. Tolerances default to 1e-4 and 1e-6.
//! Shuffle is run with several seeds and reported as min / median / max.

use std::time::Instant;

use pyta_core::{
    Bpr, GradientProjectionOptions, OdDemand, OriginOrder, TurnOptions, TurnTable, Vdf,
    gradient_projection,
};
use pyta_io::tntp::{read_flows, read_net, read_trips};

const SHUFFLE_SEEDS: u64 = 5;
const MAX_SWEEPS: usize = 3000;

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args
        .next()
        .expect("usage: order_benchmark <network> [tolerance ...]");
    let mut tolerances: Vec<f64> = args.map(|a| a.parse().expect("tolerance")).collect();
    if tolerances.is_empty() {
        tolerances = vec![1e-4, 1e-6];
    }

    let dir = format!("{}/../../data/tntp/{name}", env!("CARGO_MANIFEST_DIR"));
    let net = read_net(format!("{dir}/{name}_net.tntp")).expect("net file");
    let trips = read_trips(format!("{dir}/{name}_trips.tntp")).expect("trips file");
    let demand = OdDemand::from_triples(
        net.network.num_nodes(),
        &trips.origins,
        &trips.destinations,
        &trips.flows,
    )
    .expect("demand");
    let turns = TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default())
        .expect("turns");
    let vdf = Bpr::new(
        net.free_flow_time.clone(),
        net.capacity.clone(),
        net.b.clone(),
        net.power.clone(),
    )
    .expect("BPR parameters");

    println!(
        "{name}: {} zones, {} nodes, {} links, {} turns, {} OD pairs, demand {:.0}",
        net.num_zones,
        net.network.num_nodes(),
        net.network.num_links(),
        turns.num_turns(),
        trips.flows.len(),
        demand.total()
    );

    let run = |order: OriginOrder, gap_tolerance: f64| {
        let options = GradientProjectionOptions {
            max_iterations: MAX_SWEEPS,
            gap_tolerance,
            origin_order: order,
            ..Default::default()
        };
        let start = Instant::now();
        let result = gradient_projection(&net.network, &turns, &vdf, &demand, options)
            .expect("gradient projection");
        (result, start.elapsed().as_secs_f64())
    };

    let fixed_orders = [
        ("Fixed", OriginOrder::Fixed),
        ("Alternate", OriginOrder::Alternate),
        ("LargestGapFirst", OriginOrder::LargestGapFirst),
        ("LargestDemandFirst", OriginOrder::LargestDemandFirst),
        ("SmallestDemandFirst", OriginOrder::SmallestDemandFirst),
    ];

    println!(
        "\n{:<26}{:>10}{:>14}{:>10}",
        "order", "gap", "sweeps", "seconds"
    );
    for &tolerance in &tolerances {
        for (label, order) in fixed_orders {
            let (r, secs) = run(order, tolerance);
            let flag = if r.converged { "" } else { " (not converged)" };
            println!(
                "{label:<26}{tolerance:>10.0e}{:>14}{secs:>10.3}{flag}",
                r.iterations
            );
        }
        let mut sweeps = Vec::new();
        let mut times = Vec::new();
        for seed in 1..=SHUFFLE_SEEDS {
            let (r, secs) = run(OriginOrder::Shuffle { seed }, tolerance);
            sweeps.push(r.iterations);
            times.push(secs);
        }
        sweeps.sort();
        let mean_time = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "{:<26}{tolerance:>10.0e}{:>14}{mean_time:>10.3}",
            format!("Shuffle ({SHUFFLE_SEEDS} seeds)"),
            format!(
                "{}/{}/{}",
                sweeps[0],
                sweeps[sweeps.len() / 2],
                sweeps[sweeps.len() - 1]
            ),
        );
        println!();
    }

    // Sanity check against the published equilibrium, if the file loads.
    if let Ok(published) = read_flows(format!("{dir}/{name}_flow.tntp"), &net) {
        let tightest = tolerances.iter().cloned().fold(f64::INFINITY, f64::min);
        let (r, _) = run(OriginOrder::Fixed, tightest);
        let worst = r
            .link_flow
            .iter()
            .zip(&published.volume)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        println!(
            "vs published (Fixed, gap {tightest:.0e}): objective {:.2} vs {:.2}, worst link flow difference {worst:.2}",
            r.objective,
            vdf.integral(&published.volume)
        );
    }
}
