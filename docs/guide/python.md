# Python guide

This guide builds on the Home → Work example from [Getting started](../getting-started.md), so
`links`, `trips`, `net` and `demand` are the ones defined there.

```python
import numpy as np
import pandas as pd
import pyta
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
banned = pyta.assign(pyta.Network.from_tables(links, turns), demand)
banned.link_flow.round()
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
truck_demand = pyta.Demand.from_table(net, truck_trips)

cars   = pyta.TrafficClass(demand, name="cars")
trucks = pyta.TrafficClass(truck_demand, pce=2.5, name="trucks",
                           allowed_links=np.array([True, True, False, False, False]))
mixed = pyta.assign(net, [cars, trucks])
mixed.link_table().round()
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
loaded = pyta.assign(net, demand, link_preload=buses)
loaded.path_table().round(2)
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

`path_table()` keeps each path's links as a tuple. `explode` turns it into one row per path and link,
which makes select-link analysis a filter. Who uses the shortcut (link 4) in the mixed run?

```python
paths = mixed.path_table().explode("links")
paths[paths["links"] == 4].groupby("class")["flow"].sum()
# class
# cars    544.300375
```

## Results

| method or attribute | meaning |
|---|---|
| `link_table()`, `turn_table()` | DataFrames per link and per turn: flows (per class if several), volume, preload, volume/capacity, time |
| `path_table(class_index=None)` | DataFrame per path: class, origin, destination, flow, time, links |
| `link_flow`, `turn_flow` | numpy: vehicles per link and per turn (first class) |
| `class_link_flow`, `class_turn_flow` | numpy: one array per class |
| `link_volume`, `turn_volume` | numpy: all classes in car units, preload excluded |
| `link_time`, `turn_time` | numpy: travel time at the final volumes |
| `converged`, `iterations`, `relative_gap` | whether and how well it converged |
| `objective` | the Beckmann objective; only for a single class with PCE 1 |
| `paths(class_index=0)` | every path as flat numpy arrays, faster than `path_table()` for large models |

The tables need pandas; the numpy attributes do not.

## Input reference

### Links

| column | meaning |
|---|---|
| `from_node`, `to_node` | node ids, from 0 |
| `free_flow_time` | minutes on an empty road |
| `capacity` | vehicles per hour, positive |
| `b`, `power` | optional; the travel time is `free_flow_time * (1 + b * (volume / capacity) ** power)`, default `b=0.15`, `power=4` |

`Network.from_tables(links, turns=None, num_nodes=None, is_endpoint=None, allow_u_turns=False)` also
accepts a dict of numpy arrays instead of a DataFrame.

### Zones

A zone is a node: `origin` and `destination` are node ids. In networks with separate zone centroids,
pass `is_endpoint` (a bool per node) so routes can start and end at a centroid but never pass through it.
Trips with `origin == destination` are skipped.

### From a TNTP file

```python
net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp(net, "SiouxFalls_trips.tntp")
```

Nodes are renumbered from 0, and centroids are marked automatically.

### From a GMNS dataset

```python
net = pyta.Network.from_gmns("path/to/gmns", length_unit="foot", speed_unit="mph")
demand = pyta.Demand.from_gmns(net, "path/to/demand.csv",
                               origin="orig_taz", destination="dest_taz", flow="total",
                               skip_unknown_zones=True)
```

The directory needs `node.csv` and `link.csv`; `movement.csv` and `config.csv` are optional. Pass the
units when there is no `config.csv`. `net.node_ids` and `net.link_ids` map back to the ids in the files.

## Solver options

```python
pyta.assign(net, classes, gap=1e-6, max_iter=200, order="fixed", seed=0, chunk_size=0, threads=None,
            link_preload=None, turn_preload=None)
```

| argument | meaning |
|---|---|
| `gap` | stop at this relative gap. `1e-4` is a rough answer, `1e-6` a good one. |
| `max_iter` | maximum number of iterations |
| `order` | `fixed`, `shuffle`, `alternate`, `largest_gap`, `largest_demand`, `smallest_demand`. `shuffle` or `largest_gap` can be faster on large networks. |
| `chunk_size`, `threads` | parallelism; the defaults use all cores |
| `link_preload`, `turn_preload` | fixed background volume in car units (see [background traffic](#background-traffic)) |

For several time periods, call `assign` once per period with that period's demand, and scale the
capacities to the period's length.

The call releases the GIL, and the result does not depend on the number of threads.

Bad input raises `ValueError` with the reason, such as a missing column or an unreachable destination.
