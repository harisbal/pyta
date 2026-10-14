# PyTA

**Static traffic assignment with a Rust core and Python bindings.**

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

## Example

```python
import pyta

net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("SiouxFalls_trips.tntp", net)

result = pyta.assign(net, demand)
result.link_flow               # vehicles per link
result.paths()                 # equilibrium path flows
```

<div class="grid cards" markdown>

-   :material-rocket-launch: **Getting started**

    ---

    Install, then work through a small example from start to finish.

    [:octicons-arrow-right-24: Getting started](getting-started.md)

-   :material-table: **Python guide**

    ---

    Turn bans, trucks, time periods and select-link analysis on the same example.

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
