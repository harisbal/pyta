# PyTA

**Static traffic assignment with a Rust core and Python bindings.**

PyTA finds the **user equilibrium** on a road network: the traffic pattern in which nobody can save
time by switching to another path. It works with **paths**, not just link flows, so you can
see which routes carry the traffic, and it handles **turn delays**, **turn bans** and **several
traffic classes** (cars, trucks, ...) on the same network.

<div class="grid cards" markdown>

-   :material-rocket-launch: **Get going**

    ---

    Install, load a network, run an assignment in five lines.

    [:octicons-arrow-right-24: Getting started](getting-started.md)

-   :material-table: **Bring your own tables**

    ---

    Links, turns and trips as DataFrames or plain arrays. No heavy setup.

    [:octicons-arrow-right-24: Python guide](guide/python.md)

-   :material-lightbulb-on-outline: **Understand the method**

    ---

    Gradient projection, turn-aware shortest paths, multi-class equilibrium.

    [:octicons-arrow-right-24: How it works](concepts/how-it-works.md)

-   :material-check-decagram: **See the evidence**

    ---

    Reproduces published solutions; checked against independent models.

    [:octicons-arrow-right-24: Benchmarks](benchmarks.md)

</div>

## At a glance

```python
import pyta

net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("SiouxFalls_trips.tntp", net)

result = pyta.assign(net, demand, gap=1e-6)
print(result)                  # AssignResult(converged=True, iterations=54, ...)
result.link_flow               # vehicles per link
result.paths()                 # every path, as flat arrays
```

On Sioux Falls the objective matches the published value (4 231 335.29), in about 20 ms.

## Features

- **Path-based gradient projection** with Gauss-Seidel updates by origin, and Frank-Wolfe as a reference.
- **Turn-aware routing**: bans, fixed penalties and flow-dependent turn delays, exact because labels sit on links.
- **Multiple classes** with their own demand, PCE weight, extra link costs and allowed links.
- **Endpoint nodes** for zone centroids, and a plain "zone = node" model otherwise.
- **Flat inputs**: pandas DataFrames, numpy arrays or TNTP files. The Rust core does no file IO.
- **Select-link and path analysis** from the stored paths.

!!! note "Scope"
    PyTA solves the *static* problem: each period is an equilibrium, with no queues spilling back, and
    unfinished trips carry over between periods ([time periods](concepts/periods.md)). Signal models and link-direction optimisation are on the [roadmap](design/roadmap.md).
