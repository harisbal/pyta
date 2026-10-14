# Rust guide

The Rust crates are the same code the Python package runs.

| crate | contents |
|---|---|
| `pyta-core` | network, turns, shortest paths, solvers, path store. No file IO. |
| `pyta-io` | TNTP and GMNS readers |

## A full example

```rust
use pyta_core::*;
use pyta_io::tntp::{read_net, read_trips};

let net = read_net("data/tntp/SiouxFalls/SiouxFalls_net.tntp")?;
let trips = read_trips("data/tntp/SiouxFalls/SiouxFalls_trips.tntp")?;

let demand = OdDemand::from_triples(
    net.network.num_nodes(), &trips.origins, &trips.destinations, &trips.flows)?;
let turns = TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default())?;
let vdf = Bpr::new(net.free_flow_time, net.capacity, net.b, net.power)?;

let result = gradient_projection(
    &net.network, &turns, &vdf, &demand, GradientProjectionOptions::default())?;
println!("{} sweeps, gap {:.2e}, objective {:.2}",
         result.iterations, result.relative_gap, result.objective);
```

## Reading a GMNS dataset

```rust
use pyta_io::gmns::{DemandOptions, GmnsOptions, read_demand, read_network};

let gmns = read_network("path/to/gmns", &GmnsOptions::default())?;
let trips = read_demand("path/to/demand.csv", &gmns, &DemandOptions::default())?.demand;

let demand = OdDemand::from_triples(
    gmns.network.num_nodes(), &trips.origins, &trips.destinations, &trips.flows)?;
let turns = TurnTable::build(
    &gmns.network, &gmns.is_endpoint, &gmns.turn_exceptions, TurnOptions::default())?;
let vdf = Bpr::new(gmns.free_flow_time, gmns.capacity, gmns.b, gmns.power)?;
```

Set `GmnsOptions::length_unit` and `speed_unit` when the dataset has no `config.csv`.
`cargo run --release -p pyta-io --example gmns_assign` runs a dataset end to end.

## The pieces

- **`Network::from_edges`** builds the graph; link ids must be `0..num_links`.
- **`TurnTable::build`** generates the allowed turns and applies bans and penalties.
- **`Vdf`** is the travel-time function trait. `Bpr` is built in; implement `Vdf` for your own.
- **`OdDemand::from_triples`** builds the demand.
- **Solvers:** `gradient_projection`, `gradient_projection_with_turn_delays`,
  `gradient_projection_multiclass`, and the reference `frank_wolfe`.

## Several classes

```rust
let cars = ClassSpec::new(&car_demand);
let trucks = ClassSpec {
    pce: 2.5,
    extra_link_cost: Some(&toll),       // per link, in time units
    allowed_links: Some(&truck_allowed),
    ..ClassSpec::new(&truck_demand)
};
let result = gradient_projection_multiclass(
    &network, &turns, &vdf, &turn_vdf, &[cars, trucks], options)?;
```

Rust API documentation is generated with `cargo doc --workspace --no-deps --open`; see the
[Rust reference](../reference/rust.md).
