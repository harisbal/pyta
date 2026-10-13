//! Times gradient projection on one TNTP network, with memory use.
//!
//!   cargo run --release -p pyta-io --example scale_benchmark -- ChicagoRegional 1e-3 10 16
//!
//! Arguments: network folder under `data/tntp`, gap tolerance (default 1e-4), maximum
//! number of sweeps (default 50) and chunk size (default 0 = automatic). The run stops at whichever
//! comes first. Set `RAYON_NUM_THREADS` to limit the threads.

use std::time::Instant;

use pyta_core::Vdf;
use pyta_core::{
    Bpr, GradientProjectionOptions, OdDemand, OriginOrder, TurnOptions, TurnTable,
    gradient_projection,
};
use pyta_io::tntp::{read_flows, read_net, read_trips};

/// Peak resident memory of this process in MB (Linux).
fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|kb| kb.parse::<f64>().ok())
            })
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args
        .next()
        .expect("usage: scale_benchmark <network> [gap] [max sweeps]");
    let gap: f64 = args.next().map_or(1e-4, |a| a.parse().expect("gap"));
    let max_sweeps: usize = args.next().map_or(50, |a| a.parse().expect("sweeps"));
    let chunk_size: usize = args.next().map_or(0, |a| a.parse().expect("chunk size"));

    let dir = format!("{}/../../data/tntp/{name}", env!("CARGO_MANIFEST_DIR"));
    let t = Instant::now();
    let net = read_net(format!("{dir}/{name}_net.tntp")).expect("net file");
    let net_time = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let trips = read_trips(format!("{dir}/{name}_trips.tntp")).expect("trips file");
    let trips_time = t.elapsed().as_secs_f64();

    let t = Instant::now();
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
    let setup_time = t.elapsed().as_secs_f64();

    println!(
        "{name}: {} zones, {} nodes, {} links, {} turns, {} OD pairs, demand {:.0}",
        net.num_zones,
        net.network.num_nodes(),
        net.network.num_links(),
        turns.num_turns(),
        trips.flows.len(),
        demand.total()
    );
    println!(
        "read net {net_time:.2}s, read trips {trips_time:.2}s, build demand/turns {setup_time:.2}s, memory after loading {:.0} MB",
        peak_rss_mb()
    );

    let options = GradientProjectionOptions {
        max_iterations: max_sweeps,
        gap_tolerance: gap,
        origin_order: OriginOrder::Fixed,
        chunk_size,
    };
    let t = Instant::now();
    let result = gradient_projection(&net.network, &turns, &vdf, &demand, options).expect("solve");
    let solve_time = t.elapsed().as_secs_f64();

    println!(
        "solve {solve_time:.2}s: {} sweeps ({:.2}s per sweep), gap {:.3e}, converged {}, objective {:.2}",
        result.iterations,
        solve_time / result.iterations.max(1) as f64,
        result.relative_gap,
        result.converged,
        result.objective
    );
    println!(
        "{} paths stored ({:.2} per OD pair), peak memory {:.0} MB",
        result.paths.num_paths(),
        result.paths.num_paths() as f64 / trips.flows.len() as f64,
        peak_rss_mb()
    );
    if let Ok(published) = read_flows(format!("{dir}/{name}_flow.tntp"), &net) {
        let worst = result
            .link_flow
            .iter()
            .zip(&published.volume)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        println!(
            "vs published: objective {:.2} against {:.2}, worst link flow difference {worst:.1}",
            result.objective,
            vdf.integral(&published.volume)
        );
    }
    let t = result.times;
    println!(
        "time split: initial assignment {:.2}s, shortest paths {:.2}s, flow updates {:.2}s, sweep rebuild {:.2}s, gap checks {:.2}s",
        t.initial_assignment, t.shortest_paths, t.flow_updates, t.sweep_rebuild, t.gap_checks
    );
    let gaps: Vec<String> = result
        .sweep_gaps
        .iter()
        .map(|g| format!("{g:.1e}"))
        .collect();
    println!("sweep gap estimates: {}", gaps.join(" "));
}
