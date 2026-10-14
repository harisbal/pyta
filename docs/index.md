# PyTA

**Path-based traffic assignment with turn delays and multiple vehicle classes.**

Version {{ version }} · [Release notes](https://github.com/harisbal/pyta/releases) · [Source on GitHub](https://github.com/harisbal/pyta)

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

## Example

```python
import pyta

net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp(net, "SiouxFalls_trips.tntp")

result = net.assign(demand)
result.links.flow              # vehicles per link
result.links.table()           # flows, volume/capacity and travel time per link
result.paths.table()           # equilibrium path flows and times
```

<div class="grid cards" markdown>

-   :material-rocket-launch: **Getting started**

    ---

    Install, then work through a small example from start to finish.

    [:octicons-arrow-right-24: Getting started](getting-started.md)

-   :material-table: **Python guide**

    ---

    Turn bans, trucks, background traffic and select-link analysis on the same example.

    [:octicons-arrow-right-24: Python guide](guide/python.md)

-   :material-lightbulb-on-outline: **How it works**

    ---

    Problem formulation, solution method and convergence measure.

    [:octicons-arrow-right-24: How it works](concepts/how-it-works.md)

-   :material-check-decagram: **Benchmarks**

    ---

    Accuracy and run times on the standard test networks.

    [:octicons-arrow-right-24: Benchmarks](benchmarks.md)

</div>

## Next steps

Planned extensions, roughly in order:

- **Larger networks**: lower path memory and faster flow updates, for models with several thousand zones.
- **OpenStreetMap networks**, converted to GMNS or flat tables in the Python layer.
- **Link-direction optimisation**, built on the assignment engine.
- **Interacting junction models** for signalised and priority junctions.

**Stochastic user equilibrium** is deliberately deferred: PyTA keeps the deterministic equilibrium for
now. When it is added, path-based logit SUE fits the existing solver best. The full list, with the
current limitations, is in the [roadmap](design/roadmap.md).

## Contributing

Bug reports, questions and pull requests are welcome on
[GitHub](https://github.com/harisbal/pyta/issues). For larger changes, please open an issue first to
discuss the approach.

- **Tests.** Check new solver features against something independent of the code: a published solution,
  a closed-form equilibrium or an equivalent model.
- **Design decisions** are recorded in the [decision records](design/decisions/index.md); add one for any change to the model or the API.
- **Before opening a pull request**, run:

```sh
cargo fmt --check && cargo clippy --workspace --all-targets
cargo test --workspace
maturin develop --release && pytest
mkdocs build --strict
```

## Citing PyTA

If you use PyTA in your research, please cite it as:

> Ballis, H. (2026). *PyTA: Path-based static traffic assignment* (Version {{ version }})
> [Computer software]. https://github.com/harisbal/pyta

```bibtex
@software{ballis_pyta_2026,
  author  = {Ballis, Haris},
  title   = {{PyTA}: Path-based static traffic assignment},
  year    = {2026},
  version = {{{ version }}},
  url     = {https://github.com/harisbal/pyta}
}
```
