# PyTA

Static traffic assignment with a Rust core and Python bindings.

**Documentation: <https://harisbal.github.io/pyta/>**

PyTA is an open-source package for **static user-equilibrium traffic assignment** on road networks,
with a computational core written in Rust and an interface for Python. Given a network and an
origin–destination demand matrix, it computes the link and path flows that satisfy Wardrop's first
principle, solving the problem in path space with gradient projection.

Its main features are:

- **Path-based equilibration** by gradient projection, with Frank-Wolfe as a reference method. Path
  flows are retained, so select-link and route-level analyses need no post-processing.
- **Turn-aware routing**: turn prohibitions, fixed turn penalties and flow-dependent turn delays, handled
  exactly by labelling links rather than nodes in the shortest-path search.
- **Multi-class assignment**, with class-specific demand, passenger-car equivalents, generalised link
  costs (e.g. tolls) and link restrictions.
- **Sequential time periods**, in which trips not completed by the end of one period load the next.
- **Standard input formats**: TNTP benchmark files, GMNS datasets, or plain tables (pandas DataFrames,
  NumPy arrays).
- **Validation** against published equilibrium solutions of the standard test networks.

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

A worked example is in [Getting started](docs/getting-started.md); every table column is in
[the Python guide](docs/guide/python.md).

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

It reproduces the published solutions of the standard benchmark networks (Sioux Falls, Anaheim) and is
checked against independent models; see [docs/benchmarks.md](docs/benchmarks.md).

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

PyTA solves the static problem: no queue spillback, and delays depend only on a link's or turn's own
traffic. See [the roadmap](docs/design/roadmap.md).

## Citing PyTA

If you use PyTA in your research, please cite it as:

> Ballis, H. (2026). *PyTA: Static traffic assignment with a Rust core and Python bindings* (Version 0.1.0)
> [Computer software]. https://github.com/harisbal/pyta

```bibtex
@software{ballis_pyta_2026,
  author  = {Ballis, Haris},
  title   = {{PyTA}: Static traffic assignment with a {Rust} core and {Python} bindings},
  year    = {2026},
  version = {0.1.0},
  url     = {https://github.com/harisbal/pyta}
}
```

## License

MIT. The benchmark networks in `data/tntp` come from
[TransportationNetworks](https://github.com/bstabler/TransportationNetworks) and keep their own terms.
