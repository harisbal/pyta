# Python guide

This guide builds on the Home → Work example from [Getting started](../getting-started.md), so
`links`, `trips`, `net` and `demand` are the ones defined there.

```python
import numpy as np
import pandas as pd
import pytraf
```

## Turns

Every pair of links that meet at a node is a turn, allowed at no cost unless you say otherwise.
U-turns are banned. To list the turns:

```python
net.turn_links()
# (array([0, 2, 2, 4]), array([1, 3, 4, 1]))    from link -> to link, by turn id
```

Turn 2 is centre → shortcut. Ban it and the 508 shortcut drivers move to the other two routes:

```python
turns = pd.DataFrame({"from_link": [2], "to_link": [4], "banned": [True]})
banned = pytraf.Network.from_tables(links, turns).assign(demand)
banned.links.flow.round()
# array([1675., 1675., 1325., 1325.,    0.])
```

Instead of a ban, a turn can have a fixed `penalty` in minutes, or a delay that grows with the
turn's own traffic:

| column | meaning |
|---|---|
| `from_link`, `to_link` | the two links, which must meet at a node |
| `banned` | the turn is not allowed |
| `penalty` | fixed extra time; NaN for none |
| `delay_free_flow`, `delay_capacity`, `delay_b`, `delay_power` | a BPR delay on the turn's own volume; all four or none |

`Network.from_tables(..., allow_u_turns=True)` allows U-turns; a penalty on one U-turn allows just that one.

## Trucks and other classes

Add 200 trucks. Each takes the road space of 2.5 cars, and trucks may not drive through the centre
(links 2, 3 and 4):

```python
truck_trips = pd.DataFrame({"origin": [0], "destination": [3], "flow": [200.0]})
truck_demand = pytraf.Demand.from_table(net, truck_trips)

cars   = pytraf.TrafficClass(demand, name="cars")
trucks = pytraf.TrafficClass(truck_demand, pce=2.5, name="trucks",
                           allowed_links=np.array([True, True, False, False, False]))
mixed = net.assign([cars, trucks])
mixed.links.table().round()
```

```text
      from_node  to_node    flow  flow_cars  flow_trucks  volume  voc  time
link
0             0        1  1346.0     1146.0        200.0  1646.0  1.0   6.0
1             1        3  1890.0     1690.0        200.0  2190.0  1.0   7.0
2             0        2  1854.0     1854.0          0.0  1854.0  1.0   5.0
3             2        3  1310.0     1310.0          0.0  1310.0  2.0   8.0
4             2        1   544.0      544.0          0.0   544.0  1.0   1.0
```

Each truck counts as 2.5 cars in `volume`, so the trucks take up room on the ring road and some cars
move to the centre.

| `TrafficClass` argument | meaning |
|---|---|
| `demand` | the class's trips |
| `pce` | car units per vehicle (default 1) |
| `extra_link_cost` | cost per link added for this class only, in minutes. Divide a toll by the value of time first. |
| `allowed_links` | bool per link; `False` closes the link to this class |
| `name` | label in the result tables (default: the class's position) |

## Background traffic

Some traffic is not part of the demand you assign: buses on fixed routes, through traffic taken from
another model, or a known volume you want to hold constant. Pass it as a **preload**, in car units per
link. It is not routed, but the travel times respond to it.

Run 100 buses an hour through the centre, each counting as 3 cars:

```python
buses = np.array([0, 0, 300.0, 300.0, 0])        # car units on links 2 and 3
loaded = net.assign(demand, link_preload=buses)
loaded.paths.table().round(2)
```

```text
      origin  destination     flow   time      links
path
0          0            3   981.13  13.21     (2, 3)
1          0            3  1491.79  13.21     (0, 1)
2          0            3   527.08  13.21  (2, 4, 1)
```

About 270 drivers leave the centre route, and everyone's trip takes half a minute longer. `turn_preload`
does the same for turns.

## Select-link analysis

`paths.table()` keeps each path's links as a tuple. `explode` turns it into one row per path and link,
which makes select-link analysis a filter. Who uses the shortcut (link 4) in the mixed run?

```python
paths = mixed.paths.table().explode("links")
paths[paths["links"] == 4].groupby("class")["flow"].sum()
# class
# cars    544.300375
```

## Results

`net.assign(...)` returns an `AssignResult`. It groups what it holds by what it describes, and every group
has the same shape:

| group | holds |
|---|---|
| `result.links`, `result.turns` | one value per link or per turn, as numpy arrays (see below), and `table()` for a DataFrame |
| `result.paths` | `table(class_index=None)`: a DataFrame per path (class, origin, destination, flow, time, links); `arrays(class_index=0)`: the same as flat numpy arrays, faster for large models |
| `result.unrouted` | demand left out by `unreachable="skip"`: `flow` per class, `total`, `table()`, `arrays()` |
| `result.diagnostics` | `sweep_gaps`, `times` per phase, and the Beckmann `objective` (NaN unless a single class has PCE 1) |
| `result.converged`, `iterations`, `relative_gap` | whether and how well it converged |

`result.links` and `result.turns` have these arrays:

| attribute | meaning |
|---|---|
| `flow` | vehicles of all classes |
| `by_class` | vehicles per class; `by_class[0]` or `by_class["trucks"]` |
| `volume` | all classes in car units (vehicles times PCE), preload excluded; this drives the delays |
| `preload` | the fixed volume passed to `assign`, or `None` |
| `time` | travel time at the final volumes |
| `voc` | (links only) volume plus preload over capacity |

The table columns carry the same names. The tables need pandas; the numpy attributes do not. The arrays are
read-only; copy one to change it.

`print(result.summary())` gives a short account: convergence, trips per class, demand left out and the busiest link.

The pre-0.2.0 flat names (`link_flow`, `link_table()`, ...) still work with a `DeprecationWarning`.

## Input reference

### Links

| column | meaning |
|---|---|
| `from_node`, `to_node` | node ids, from 0 |
| `free_flow_time` | minutes on an empty road |
| `capacity` | vehicles per hour, positive |
| `b`, `power` | optional; the travel time is `free_flow_time * (1 + b * (volume / capacity) ** power)`, default `b=0.15`, `power=4` |

`Network.from_tables(links, turns=None, num_nodes=None, is_centroid=None, allow_u_turns=False)` also
accepts a dict of numpy arrays instead of a DataFrame.

### Zones

A zone is a node: `origin` and `destination` are node ids. In networks with separate zone centroids,
pass `is_centroid` (a bool per node) so routes can start and end at a centroid but never pass through it.
Trips with `origin == destination` are skipped.

### From a TNTP file

```python
net = pytraf.Network.from_tntp("SiouxFalls_net.tntp")
demand = pytraf.Demand.from_tntp(net, "SiouxFalls_trips.tntp")
```

Nodes are renumbered from 0, and centroids are marked automatically.

### From a GMNS dataset

```python
net = pytraf.Network.from_gmns("path/to/gmns", length_unit="foot", speed_unit="mph")
demand = pytraf.Demand.from_gmns(net, "path/to/demand.csv",
                               origin="orig_taz", destination="dest_taz", flow="total",
                               skip_unknown_zones=True)
```

The directory needs `node.csv` and `link.csv`; `movement.csv` and `config.csv` are optional. Pass the
units when there is no `config.csv`. `net.node_ids` and `net.link_ids` map back to the ids in the files.

## Solver options

```python
net.assign(classes, *, gap=1e-6, max_iter=200, order=Order.FIXED, seed=0, chunk_size=0, threads=None,
           link_preload=None, turn_preload=None, unreachable=Unreachable.ERROR)
```

Everything after `classes` is keyword-only, so a stray positional number cannot be mistaken for another
option. `pytraf.assign(net, classes, ...)` is the same call.

| argument | meaning |
|---|---|
| `gap` | stop at this relative gap. `1e-4` is a rough answer, `1e-6` a good one. |
| `max_iter` | maximum number of iterations |
| `order` | an `Order`: `FIXED`, `SHUFFLE`, `ALTERNATE`, `LARGEST_GAP`, `LARGEST_DEMAND` or `SMALLEST_DEMAND`. `SHUFFLE` or `LARGEST_GAP` can be faster on large networks. |
| `chunk_size`, `threads` | parallelism; the defaults use all cores |
| `link_preload`, `turn_preload` | fixed background volume in car units (see [background traffic](#background-traffic)) |
| `unreachable` | an `Unreachable`: `ERROR` (default) or `SKIP`; see [below](#demand-that-cannot-be-routed) |

`order` and `unreachable` are string enums: `pytraf.Order.SHUFFLE` and `"shuffle"` are the same, and a misspelt
value raises a `ValueError` that lists the valid ones.

For several time periods, call `assign` once per period with that period's demand, and scale the
capacities to the period's length.

The call releases the GIL, and the result does not depend on the number of threads.

Bad input raises `ValueError` with the reason, such as a missing column or an unreachable destination.

### Demand that cannot be routed

By default a trip between nodes that no allowed path joins is an error, and the message names the pairs (the
first five, with the count and the trips they carry). On networks that change (closures, class restrictions)
this can be expected; pass `unreachable="skip"` to leave those trips out and assign the rest:

```python
result = net.assign(classes, unreachable="skip")
result.unrouted.flow      # trips left out, one total per class
result.unrouted.table()   # DataFrame: class (if several), origin, destination, flow
result.unrouted.arrays()  # the same as numpy arrays
```

Nothing else changes: link flows, paths and the gap cover only the routed demand.
