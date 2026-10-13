# Rust guide

The Rust crates are the same code the Python package runs.

| crate | contents |
|---|---|
| `pyta-core` | network, turns, shortest paths, solvers, path store. No file IO. |
| `pyta-io` | TNTP reader |

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

## The pieces

- **`Network::from_edges(num_nodes, &[Edge])`** builds the immutable graph. An `Edge` carries an
  explicit `link_id`; ids must be exactly `0..num_links`, in any order. Costs and flows live in
  separate arrays indexed by link id.
- **`TurnTable::build(&network, &is_endpoint, &exceptions, options)`** generates every allowed
  turn and applies bans and penalties. Turn ids index the turn flow and cost arrays.
- **`Vdf`** is the delay function trait: `cost_at`, `derivative_at` and `integral` over arrays.
  `Bpr` is built in, `NoDelay` means "no delay". Implement `Vdf` to add your own.
- **`OdDemand::from_triples`** groups demand by origin.
- **Solvers:** `gradient_projection`, `gradient_projection_with_turn_delays`,
  `gradient_projection_multiclass`, and `frank_wolfe` / `frank_wolfe_with_turn_delays`.
- **Lower-level pieces:** `dijkstra` (node-based), `dijkstra_turns` (link-labelled, turn-aware),
  `all_or_nothing` and `PathStore`.

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
