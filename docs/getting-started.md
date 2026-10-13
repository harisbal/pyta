# Getting started

## Install

You need [Rust](https://rustup.rs), Python 3.9 or newer, and [maturin](https://www.maturin.rs).
PyTA is not on PyPI yet, so you build it from the repository:

```sh
git clone <repository> pyta && cd pyta
uv venv .venv && source .venv/bin/activate        # or: python -m venv .venv
uv pip install numpy pandas pytest maturin
maturin develop --release                          # compile the Rust core and install `pyta`
pytest                                             # optional
```

Use `--release`: an optimised build is about ten times faster than a debug build.

## Your first assignment

The repository includes the Sioux Falls benchmark network.

```python
import pyta

net = pyta.Network.from_tntp("data/tntp/SiouxFalls/SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("data/tntp/SiouxFalls/SiouxFalls_trips.tntp", net)

result = pyta.assign(net, demand, gap=1e-6)
print(result.converged, result.iterations, result.relative_gap)
```

`gap` is the **relative gap**: how far the flows are from equilibrium (see
[how it works](concepts/how-it-works.md#the-relative-gap)). `1e-4` is a rough answer, `1e-6` is a good one.

Read the results:

```python
result.link_flow            # numpy array: vehicles on each link
result.objective            # the Beckmann objective
result.sweep_gaps           # gap estimate after each sweep
```

## Your own network

Build it from two tables:

=== "pandas"

    ```python
    import pandas as pd

    links = pd.DataFrame({
        "from_node":      [0, 0, 1, 2],
        "to_node":        [1, 2, 3, 3],
        "free_flow_time": [4.0, 6.0, 5.0, 3.0],   # minutes at zero flow
        "capacity":       [1800, 1800, 1200, 2400],
    })
    trips = pd.DataFrame({"origin": [0], "destination": [3], "flow": [3000.0]})

    net = pyta.network_from_tables(links)
    demand = pyta.demand_from_table(trips, num_nodes=net.num_nodes)
    result = pyta.assign(net, demand)
    ```

=== "numpy"

    ```python
    import numpy as np

    net = pyta.Network(
        4,
        np.array([0, 0, 1, 2]), np.array([1, 2, 3, 3]),
        np.array([4.0, 6.0, 5.0, 3.0]), np.array([1800.0, 1800.0, 1200.0, 2400.0]),
    )
    demand = pyta.Demand(4, np.array([0]), np.array([3]), np.array([3000.0]))
    result = pyta.assign(net, demand)
    ```

Links are numbered by row, nodes are integers from 0, and a zone is simply a node. The link delay
is the BPR function `free_flow_time * (1 + b * (volume / capacity) ** power)` with `b = 0.15` and
`power = 4` unless you give columns for them.

## Next steps

- [Turns](concepts/turns.md): bans, penalties and turn delays.
- [Traffic classes](concepts/classes.md): trucks, tolls and banned links.
- [Python guide](guide/python.md): every table column and result attribute.
