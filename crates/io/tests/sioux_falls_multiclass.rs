//! Multi-class gradient projection on Sioux Falls.

use std::collections::HashMap;

use pyta_core::{
    Bpr, ClassSpec, GradientProjectionOptions, MultiClassResult, NoDelay, OdDemand, OriginOrder,
    TurnOptions, TurnTable, gradient_projection_multiclass,
};
use pyta_io::tntp::{TntpNetwork, read_flows, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

struct Case {
    net: TntpNetwork,
    trips: pyta_io::tntp::Demand,
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
        trips,
        turns,
        vdf,
        published,
    }
}

/// The trip table scaled by `share`.
fn demand(case: &Case, share: f64) -> OdDemand {
    let scaled: Vec<f64> = case.trips.flows.iter().map(|q| q * share).collect();
    OdDemand::from_triples(24, &case.trips.origins, &case.trips.destinations, &scaled).unwrap()
}

fn solve(case: &Case, classes: &[ClassSpec], gap_tolerance: f64) -> MultiClassResult {
    let options = GradientProjectionOptions {
        max_iterations: 1000,
        gap_tolerance,
        origin_order: OriginOrder::Fixed,
        ..Default::default()
    };
    let no_delay = NoDelay::new(case.turns.num_turns());
    gradient_projection_multiclass(
        &case.net.network,
        &case.turns,
        &case.vdf,
        &no_delay,
        classes,
        options,
    )
    .unwrap()
}

fn worst_difference(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

#[test]
fn two_identical_classes_reproduce_the_published_single_class_equilibrium() {
    let case = load();
    let (cars, vans) = (demand(&case, 0.6), demand(&case, 0.4));
    let classes = [ClassSpec::new(&cars), ClassSpec::new(&vans)];

    let r = solve(&case, &classes, 1e-6);
    assert!(r.converged);
    let sum: Vec<f64> = (0..76)
        .map(|a| r.classes[0].link_flow[a] + r.classes[1].link_flow[a])
        .collect();
    // A gap of 1e-6 means flows within a few vehicles; the tight run below is the precise check.
    assert!(worst_difference(&sum, &case.published) < 5.0);
    assert!(worst_difference(&r.link_volume, &case.published) < 5.0);

    let tight = solve(&case, &classes, 1e-8);
    assert!(tight.converged);
    assert!(worst_difference(&tight.link_volume, &case.published) < 0.1);
}

#[test]
fn trucks_with_pce_and_banned_links() {
    let case = load();
    let (cars, trucks) = (demand(&case, 0.85), demand(&case, 0.15));
    // Trucks may not use every 9th link (ids 0, 9, 18, ...).
    let allowed: Vec<bool> = (0..76).map(|a| a % 9 != 0).collect();
    // A toll of 4 minutes-equivalent on three links for the trucks.
    let mut toll = vec![0.0; 76];
    for a in [5, 20, 41] {
        toll[a] = 4.0;
    }
    let classes = [
        ClassSpec::new(&cars),
        ClassSpec {
            pce: 2.5,
            extra_link_cost: Some(&toll),
            allowed_links: Some(&allowed),
            ..ClassSpec::new(&trucks)
        },
    ];

    let r = solve(&case, &classes, 1e-6);
    assert!(
        r.converged,
        "gap {} after {} sweeps",
        r.relative_gap, r.iterations
    );
    assert!(r.relative_gap <= 1e-6);

    // Trucks stay off banned links.
    for (a, &ok) in allowed.iter().enumerate() {
        if !ok {
            assert_eq!(r.classes[1].link_flow[a], 0.0, "trucks on banned link {a}");
        }
    }
    // The volume that drives travel times is cars plus 2.5 per truck.
    for a in 0..76 {
        let volume = r.classes[0].link_flow[a] + 2.5 * r.classes[1].link_flow[a];
        assert!(
            (volume - r.link_volume[a]).abs() < 1e-6 * volume.max(1.0),
            "link {a}"
        );
    }
    // Each class's path flows add up to its own demand, OD pair by OD pair.
    for (k, class) in [&cars, &trucks].into_iter().enumerate() {
        let mut got: HashMap<(u32, u32), f64> = HashMap::new();
        for (o, d, links, flow) in r.classes[k].paths.iter() {
            *got.entry((o, d)).or_default() += flow;
            if k == 1 {
                assert!(links.iter().all(|&a| allowed[a as usize]));
            }
        }
        for o in 0..24u32 {
            let (ds, qs) = class.from_origin(o);
            for (&d, &q) in ds.iter().zip(qs) {
                if q > 0.0 && d != o {
                    assert!((got[&(o, d)] - q).abs() < 1e-6, "class {k} OD {o}->{d}");
                }
            }
        }
    }
    // The heavier, restricted, tolled trucks really change the outcome versus all-cars.
    let all_cars = demand(&case, 1.0);
    let baseline = solve(&case, &[ClassSpec::new(&all_cars)], 1e-6);
    assert!(worst_difference(&r.link_volume, &baseline.link_volume) > 50.0);
}
