//! Multi-period assignment on Sioux Falls: invariants that must hold whatever the numbers.

use pyta_core::{
    Bpr, ClassSpec, GradientProjectionOptions, OdDemand, Period, TurnOptions, TurnTable, Vdf,
    assign_periods, gradient_projection_multiclass,
};
use pyta_io::tntp::{read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn unfinished_trips_load_the_next_period_and_a_single_period_matches_the_plain_solver() {
    let net = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    let n = net.network.num_nodes();
    let full =
        OdDemand::from_triples(n, &trips.origins, &trips.destinations, &trips.flows).unwrap();
    // A fifth of the demand, so that trips are short next to the 30-minute periods.
    let flows: Vec<f64> = trips.flows.iter().map(|q| q / 5.0).collect();
    let demand = OdDemand::from_triples(n, &trips.origins, &trips.destinations, &flows).unwrap();
    let none = OdDemand::from_triples(n, &[], &[], &[]).unwrap();
    assert!(full.total() > demand.total());

    let turns =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    let link_vdf = Bpr::new(
        net.free_flow_time.clone(),
        net.capacity.clone(),
        net.b.clone(),
        net.power.clone(),
    )
    .unwrap();
    let turn_vdf = Bpr::new(
        vec![0.0; turns.num_turns()],
        vec![1.0; turns.num_turns()],
        vec![0.0; turns.num_turns()],
        vec![1.0; turns.num_turns()],
    )
    .unwrap();
    let options = GradientProjectionOptions {
        gap_tolerance: 1e-6,
        max_iterations: 300,
        ..Default::default()
    };

    let busy = [ClassSpec::new(&demand)];
    let quiet = [ClassSpec::new(&none)];
    let periods = [
        Period {
            duration: 30.0,
            classes: &busy,
        },
        Period {
            duration: 30.0,
            classes: &busy,
        },
        Period {
            duration: 30.0,
            classes: &quiet,
        },
        Period {
            duration: 30.0,
            classes: &quiet,
        },
    ];
    let r = assign_periods(
        &net.network,
        &turns,
        &link_vdf,
        &turn_vdf,
        &periods,
        options,
    )
    .unwrap();
    assert!(r.iter().all(|p| p.assignment.converged));

    // The first period starts empty; the busy period leaves trips on the network, which load
    // the second period and make its links slower; a quiet period leaves none behind.
    let total = |v: &[f64]| v.iter().sum::<f64>();
    assert_eq!(total(&r[0].link_preload), 0.0);
    assert!(total(&r[1].link_preload) > 0.0 && total(&r[1].turn_preload) > 0.0);
    assert!(total(&r[2].link_preload) > 0.0);
    assert_eq!(total(&r[3].link_preload), 0.0);
    // Same demand in periods 0 and 1, but period 1 starts with trips on the network, so the
    // total travel time of its own trips is higher (capacities are halved for 30 minutes).
    let scaled = link_vdf.with_capacity_scaled(0.5);
    let total_time = |p: &pyta_core::PeriodResult| -> f64 {
        (0..net.network.num_links())
            .map(|l| {
                let v = p.assignment.link_volume[l];
                v * scaled.cost_at(l, v + p.link_preload[l])
            })
            .sum()
    };
    assert!(total_time(&r[1]) > total_time(&r[0]));

    // One period of an hour has capacity per hour times one, so it is the plain solver.
    let plain =
        gradient_projection_multiclass(&net.network, &turns, &link_vdf, &turn_vdf, &busy, options)
            .unwrap();
    let one = assign_periods(
        &net.network,
        &turns,
        &link_vdf,
        &turn_vdf,
        &[Period {
            duration: 60.0,
            classes: &busy,
        }],
        options,
    )
    .unwrap();
    assert_eq!(one[0].assignment.link_volume, plain.link_volume);
}
