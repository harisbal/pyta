# PyTA

[![Release](https://img.shields.io/github/v/release/harisbal/pyta)](https://github.com/harisbal/pyta/releases)
[![Docs](https://img.shields.io/badge/docs-harisbal.github.io%2Fpyta-blue)](https://harisbal.github.io/pyta/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Path-based traffic assignment with turn delays and multiple vehicle classes.

**Documentation: <https://harisbal.github.io/pyta/>**

PyTA is an open-source package for **static user-equilibrium traffic assignment** on road networks.
Given a network and an origin–destination demand matrix, it computes the link and path flows that
satisfy Wardrop's first principle, solving the problem in path space with gradient projection.

Its main features are:

- **Path-based equilibration** by gradient projection, with Frank-Wolfe as a reference method. Path
  flows are retained, so select-link and route-level analyses need no post-processing.
- **Turn-aware routing**: turn prohibitions, fixed turn penalties and flow-dependent turn delays, handled
  exactly by labelling links rather than nodes in the shortest-path search.
- **Multi-class assignment**, with class-specific demand, passenger-car equivalents, generalised link
  costs (e.g. tolls) and link restrictions.
- **Background traffic**: fixed preloads on links and turns, such as buses or through traffic.
- **Tabular results**: link, turn and path results as pandas DataFrames, or as NumPy arrays.
- **Standard input formats**: TNTP benchmark files, GMNS datasets, or plain tables (pandas DataFrames,
  NumPy arrays).
- **Validation** against published equilibrium solutions of the standard test networks.
- **Performance**: a multi-threaded Rust core, usable from Python or directly from Rust.

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
demand = pyta.Demand.from_tntp(net, "data/tntp/SiouxFalls/SiouxFalls_trips.tntp")

result = pyta.assign(net, demand, gap=1e-6)
result.converged, result.iterations, result.relative_gap
result.link_flow          # numpy array, one entry per link
result.objective          # Beckmann objective (single class, PCE 1)
result.link_table()       # DataFrame: flow, volume/capacity and time per link
result.path_table()       # DataFrame: every path with its flow, time and links
```

From your own tables (DataFrames or dicts of arrays):

```python
net = pyta.Network.from_tables(links_df, turns_df)           # see docs/guide/python.md for columns
demand = pyta.Demand.from_table(net, trips_df)
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

## Limitations

PyTA solves the static problem: no queue spillback, and delays depend only on a link's or turn's own
traffic. See [the roadmap](docs/design/roadmap.md).

## Next steps

Planned extensions, roughly in order:

- **Larger networks**: lower path memory and faster flow updates, for models with several thousand zones.
- **OpenStreetMap networks**, converted to GMNS or flat tables in the Python layer.
- **Link-direction optimisation**, built on the assignment engine.
- **Interacting junction models** for signalised and priority junctions.

**Stochastic user equilibrium** is deliberately deferred: PyTA keeps the deterministic equilibrium for
now. When it is added, path-based logit SUE fits the existing solver best. The full list, with the
current limitations, is in the [roadmap](docs/design/roadmap.md).

## Contributing

Bug reports, questions and pull requests are welcome on
[GitHub](https://github.com/harisbal/pyta/issues). For larger changes, please open an issue first to
discuss the approach.

- **Tests.** Check new solver features against something independent of the code: a published solution,
  a closed-form equilibrium or an equivalent model.
- **Design decisions** are recorded in [docs/design/decisions](docs/design/decisions/); add one for any change to the model or the API.
- **Before opening a pull request**, run:

```sh
cargo fmt --check && cargo clippy --workspace --all-targets
cargo test --workspace
maturin develop --release && pytest
mkdocs build --strict
```

Benchmarks: `cargo run --release -p pyta-io --example order_benchmark -- Anaheim 1e-4 1e-6`.

**Releases.** The version lives in one place, `[workspace.package]` in `Cargo.toml`; the crates, the
Python package (`pyta.__version__`) and the docs read it from there. To release, bump it, update
`version` and `date-released` in `CITATION.cff` and the citation in this README, then tag `vX.Y.Z` and
publish a GitHub release.

## Citing PyTA

If you use PyTA in your research, please cite it as below (GitHub's "Cite this repository" button gives
the same from [`CITATION.cff`](CITATION.cff)):

> Ballis, H. (2026). *PyTA: Path-based static traffic assignment* (Version 0.1.1)
> [Computer software]. https://github.com/harisbal/pyta

```bibtex
@software{ballis_pyta_2026,
  author  = {Ballis, Haris},
  title   = {{PyTA}: Path-based static traffic assignment},
  year    = {2026},
  version = {0.1.1},
  url     = {https://github.com/harisbal/pyta}
}
```

## License

MIT. The benchmark networks in `data/tntp` come from
[TransportationNetworks](https://github.com/bstabler/TransportationNetworks) and keep their own terms.
