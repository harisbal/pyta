//! Sioux Falls written out as a GMNS dataset reproduces the published equilibrium.
//!
//! The TNTP network is converted to GMNS tables in memory: free-flow time is stored as
//! a length in km with a speed of 60 km/h (which makes km equal minutes), the BPR
//! parameters go into `vdf_alpha` / `vdf_beta`, and zones are the nodes themselves.

use pytraf_core::{
    Bpr, GradientProjectionOptions, OdDemand, TurnOptions, TurnTable, Vdf, gradient_projection,
};
use pytraf_io::gmns::{DemandOptions, GmnsOptions, parse_demand, parse_network};
use pytraf_io::tntp::{read_flows, read_net, read_trips};

fn data(file: &str) -> String {
    format!(
        "{}/../../data/tntp/SiouxFalls/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn gmns_input_reproduces_the_published_equilibrium() {
    let tntp = read_net(data("SiouxFalls_net.tntp")).unwrap();
    let trips = read_trips(data("SiouxFalls_trips.tntp")).unwrap();
    let published = read_flows(data("SiouxFalls_flow.tntp"), &tntp)
        .unwrap()
        .volume;
    let num_links = tntp.network.num_links();

    let mut node_csv = String::from("node_id,zone_id\n");
    for n in 1..=24 {
        node_csv += &format!("{n},{n}\n");
    }
    let mut link_csv = String::from(
        "link_id,from_node_id,to_node_id,directed,length,free_speed,capacity,lanes,vdf_alpha,vdf_beta\n",
    );
    for a in 0..num_links {
        let id = a as u32;
        link_csv += &format!(
            "{a},{},{},1,{},60,{},1,{},{}\n",
            tntp.network.from_node(id) + 1,
            tntp.network.to_node(id) + 1,
            tntp.free_flow_time[a],
            tntp.capacity[a],
            tntp.b[a],
            tntp.power[a],
        );
    }
    let mut demand_csv = String::from("o_zone_id,d_zone_id,volume\n");
    for i in 0..trips.flows.len() {
        demand_csv += &format!(
            "{},{},{}\n",
            trips.origins[i] + 1,
            trips.destinations[i] + 1,
            trips.flows[i]
        );
    }

    let options = GmnsOptions {
        length_unit: Some("km".to_string()),
        speed_unit: Some("kph".to_string()),
        ..GmnsOptions::default()
    };
    let gmns = parse_network(&node_csv, &link_csv, None, None, &options).unwrap();
    assert_eq!(gmns.network.num_links(), num_links);
    let gmns_demand = parse_demand(demand_csv.as_bytes(), &gmns, &DemandOptions::default())
        .unwrap()
        .demand;
    assert_eq!(gmns_demand.flows.len(), trips.flows.len());
    assert!((gmns_demand.total() - trips.total()).abs() < 1e-6);

    let demand = OdDemand::from_triples(
        gmns.network.num_nodes(),
        &gmns_demand.origins,
        &gmns_demand.destinations,
        &gmns_demand.flows,
    )
    .unwrap();
    let turns = TurnTable::build(
        &gmns.network,
        &gmns.is_endpoint,
        &gmns.turn_exceptions,
        TurnOptions::default(),
    )
    .unwrap();
    let vdf = Bpr::new(
        gmns.free_flow_time.clone(),
        gmns.capacity.clone(),
        gmns.b.clone(),
        gmns.power.clone(),
    )
    .unwrap();

    let result = gradient_projection(
        &gmns.network,
        &turns,
        &vdf,
        &demand,
        GradientProjectionOptions {
            max_iterations: 500,
            gap_tolerance: 1e-6,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.converged);

    let published_objective = vdf.integral(&published);
    let excess = (result.objective - published_objective) / published_objective;
    assert!(excess > -1e-9 && excess < 1e-7, "objective excess {excess}");
    let worst = result
        .link_flow
        .iter()
        .zip(&published)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(worst < 5.0, "worst link error {worst}");
}
