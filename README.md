# PyTA

Static traffic assignment with a Rust core and Python bindings.

PyTA solves the **user-equilibrium** problem on road networks: every traveller picks a cheapest
path given everyone else's choices. It is **path-based** (it keeps the paths, so select-link and
path analysis come for free), handles **turn delays and bans**, supports **several traffic classes**,
and reads flat tables, so there is little boilerplate between your data and a result.

## What it does

- **Gradient projection** (path-based, Gauss-Seidel by origin) as the main solver; **Frank-Wolfe**
  as a simple reference solver.
- **Turn-aware shortest paths**: labels sit on links, so banned turns and turn penalties are exact.
- **Turn costs**: fixed penalties, bans, and a delay that depends on the turn's own flow.
- **Multiple classes**: own demand, passenger-car-equivalent (PCE) weight, extra link costs
  (tolls, distance charges) and allowed links (e.g. a truck ban) per class.
- **Zone = node**, with optional endpoint-only nodes (centroids) that paths may start or end at but
  not pass through. No connector concept.
- **TNTP reader** for the standard benchmark networks, validated against published solutions.

Static means one time period, no queue spillback and no vehicle-level simulation.
See [limitations](#limitations).

## Install

You need Rust ([rustup](https://rustup.rs)), Python 3.9 or newer, and
[maturin](https://www.maturin.rs).

```sh
uv venv .venv && source .venv/bin/activate        # or python -m venv
uv pip install numpy pandas pytest maturin
maturin develop --release                          # builds and installs `pyta`
pytest                                             # optional: run the Python tests
```

## Python quick start

```python
import pyta

net = pyta.Network.from_tntp("data/tntp/SiouxFalls/SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("data/tntp/SiouxFalls/SiouxFalls_trips.tntp", net)

result = pyta.assign(net, demand, gap=1e-6)
result.converged, result.iterations, result.relative_gap
result.link_flow          # numpy array, one entry per link
result.objective          # Beckmann objective (single class, PCE 1)
result.paths()            # every path as flat arrays: select-link analysis, path export
```

From your own tables (DataFrames or dicts of arrays):

```python
net = pyta.network_from_tables(links_df, turns_df)           # see docs/guide/python.md for columns
demand = pyta.demand_from_table(trips_df, num_nodes=net.num_nodes)
result = pyta.assign(net, demand, gap=1e-6)
```

Several classes:

```python
cars   = pyta.TrafficClass(car_demand)
trucks = pyta.TrafficClass(truck_demand, pce=2.5,
                           extra_link_cost=toll_in_minutes,        # per link, same units as time
                           allowed_links=~truck_banned)            # bool per link
result = pyta.assign(net, [cars, trucks], gap=1e-6)
result.class_link_flow     # list of arrays, vehicles per class
result.link_volume         # PCE-weighted volume that drives travel times
```

The full guide, with every table column, is in [the Python guide](docs/guide/python.md).

## Rust quick start

```rust
use pyta_core::*;
use pyta_io::tntp::{read_net, read_trips};

let net = read_net("data/tntp/SiouxFalls/SiouxFalls_net.tntp")?;
let trips = read_trips("data/tntp/SiouxFalls/SiouxFalls_trips.tntp")?;
let demand = OdDemand::from_triples(
    net.network.num_nodes(), &trips.origins, &trips.destinations, &trips.flows)?;
let turns = TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default())?;
let vdf = Bpr::new(net.free_flow_time, net.capacity, net.b, net.power)?;

let result = gradient_projection(&net.network, &turns, &vdf, &demand,
                                 GradientProjectionOptions::default())?;
println!("{} sweeps, gap {:.2e}", result.iterations, result.relative_gap);
```

`gradient_projection_multiclass` takes a slice of `ClassSpec`s and a turn delay function.
The core crate does no file IO: it takes typed arrays, so Rust and Python users call the same code.

## Does it get the right answer?

The tests check the solvers against published results and against independent models.

| check | result |
|---|---|
| Sioux Falls objective, published 4 231 335.29 | reproduced to 7 digits; link flows within 0.014 vehicles at gap 1e-8 |
| Anaheim (real centroids) | published flows reproduced to 0.00 vehicles at gap 1e-10 |
| Chicago Sketch | converges to a lower objective than the published file, which is itself not fully converged |
| Turn costs | flows equal those of a turn-free model on an expanded line graph (Sioux Falls, Anaheim) |
| Multi-class | two identical classes reproduce the single-class equilibrium; tolls, bans and PCE match closed forms |

Speed and convergence numbers are in [docs/benchmarks.md](docs/benchmarks.md). For scale: Chicago Regional
(1 790 zones, 39 018 links, 2.3 million OD pairs) reaches a relative gap of 1e-4 in about two minutes on 8
threads, using 1.3 GB.

## Layout

```
crates/core     algorithms, no IO: network, turns, shortest paths, solvers, path store
crates/io       TNTP reader (and the benchmark example)
crates/python   PyO3 bindings (the `pyta._pyta` extension)
python/pyta     Python package: DataFrame helpers and type stubs
python/tests    pytest suite
data/tntp       benchmark networks from github.com/bstabler/TransportationNetworks
docs            the documentation site source (MkDocs): guides, concepts, design, benchmarks
```

## Documentation

The full documentation is a MkDocs site built from `docs/`:

```sh
uv pip install -e ".[docs]"
mkdocs serve                 # live preview at http://127.0.0.1:8000
mkdocs build --strict        # static site in site/
cargo doc --workspace --no-deps --open     # Rust API reference
```

## Development

```sh
cargo test --workspace        # Rust tests (about 30 s in debug, mostly one large test)
cargo clippy --workspace --all-targets
cargo fmt --check
maturin develop --release && pytest
cargo run --release -p pyta-io --example order_benchmark -- Anaheim 1e-4 1e-6
```

## Limitations

- **Static and separable.** One period; delays depend on a link's or turn's own volume. Interacting
  junction models (signals, gap acceptance) are not implemented. See [the roadmap](docs/design/roadmap.md).
- **Multi-class is gradient projection only**, and has no objective function: with different PCEs
  the problem is asymmetric, so convergence is judged by the relative gap.
- **Frank-Wolfe has no multi-class.**
- **Path memory** grows with zones squared times paths per OD (1.3 GB for 2.3 million OD pairs, with
  paths stored as a tree per origin). Sydney and Austin are untested; bush-based methods are the planned
  answer if they do not fit.
- **Not yet implemented:** multiple time periods and carry-over, GMNS input, OSM download,
  link-direction optimisation.
