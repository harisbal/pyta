# PyTA

**Static traffic assignment with a Rust core and Python bindings.**

Give PyTA a road network and a table of trips, and it works out which routes the traffic takes once
everyone has settled on their quickest one. It keeps the routes, not just link totals, and handles
**turn bans and delays**, **several vehicle classes** (cars, trucks, ...) and **consecutive time periods**.

```python
import pyta

net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("SiouxFalls_trips.tntp", net)

result = pyta.assign(net, demand)
result.link_flow               # vehicles per link
result.paths()                 # every route and how many use it
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

    User equilibrium, the solver and the relative gap, in brief.

    [:octicons-arrow-right-24: How it works](concepts/how-it-works.md)

-   :material-check-decagram: **Benchmarks**

    ---

    Reproduces published solutions on standard test networks.

    [:octicons-arrow-right-24: Benchmarks](benchmarks.md)

</div>
