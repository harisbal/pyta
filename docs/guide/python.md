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
banned = pyta.assign(pyta.network_from_tables(links, turns), demand)
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

`network_from_tables(..., allow_u_turns=True)` allows U-turns; a penalty on one U-turn allows just that one.

## Trucks and other classes

Add 200 trucks. Each takes the road space of 2.5 cars, and trucks may not drive through the centre
(links 2, 3 and 4):

```python
truck_trips = pd.DataFrame({"origin": [0], "destination": [3], "flow": [200.0]})
truck_demand = pyta.demand_from_table(truck_trips, num_nodes=net.num_nodes)

cars   = pyta.TrafficClass(demand)
trucks = pyta.TrafficClass(truck_demand, pce=2.5,
                           allowed_links=np.array([True, True, False, False, False]))
mixed = pyta.assign(net, [cars, trucks])

mixed.class_link_flow[0].round()    # cars:   array([1146., 1690., 1854., 1310.,  544.])
mixed.class_link_flow[1].round()    # trucks: array([ 200.,  200.,    0.,    0.,    0.])
mixed.link_volume.round()           # in car units: array([1646., 2190., 1854., 1310.,  544.])
```

The trucks take up room on the ring road, so some cars move to the centre.

| `TrafficClass` argument | meaning |
|---|---|
| `demand` | the class's trips |
| `pce` | car units per vehicle (default 1) |
| `extra_link_cost` | cost per link added for this class only, in minutes. Divide a toll by the value of time first. |
| `allowed_links` | bool per link; `False` closes the link to this class |

## Several time periods

Split the morning into 7:00–8:00 and 8:00–9:00 with the same demand in each. Drivers still on the road
at 8:00 carry on into the second hour and take up road space there:

```python
early = pyta.Period(60, pyta.TrafficClass(demand))     # duration in minutes
late  = pyta.Period(60, pyta.TrafficClass(demand))
first, second = pyta.assign_periods(net, [early, late])

second.link_preload.round()   # array([  0., 179.,   0., 107.,  43.])  still driving at 8:00
second.link_flow.round()      # array([1249., 1826., 1751., 1174.,  578.])
```

Capacities stay per hour and demand is per period. See [time periods](../concepts/periods.md) for how
the leftover traffic is worked out.

## Select-link analysis

How many of the cars in the mixed run use the shortcut (link 4)?

```python
p = mixed.paths()                     # class 0: cars
uses_shortcut = np.array([4 in p["links"][a:b]
                          for a, b in zip(p["link_offsets"][:-1], p["link_offsets"][1:])])
p["flow"][uses_shortcut].sum()       # 544.3
```

`mixed.paths(1)` gives the truck paths.

## Results

| attribute | meaning |
|---|---|
| `link_flow`, `turn_flow` | vehicles per link and per turn (first class) |
| `class_link_flow`, `class_turn_flow` | one array per class |
| `link_volume`, `turn_volume` | all classes in car units: what the travel times depend on |
| `link_preload`, `turn_preload` | traffic left over from the previous period (`assign_periods` only) |
| `converged`, `iterations`, `relative_gap` | whether and how well it converged |
| `objective` | the Beckmann objective; only for a single class with PCE 1 |
| `paths(class_index=0)` | every path: `origin`, `destination`, `flow`, `links`, `link_offsets` |

## Input reference

### Links

| column | meaning |
|---|---|
| `from_node`, `to_node` | node ids, from 0 |
| `free_flow_time` | minutes on an empty road |
| `capacity` | vehicles per hour, positive |
| `b`, `power` | optional; the travel time is `free_flow_time * (1 + b * (volume / capacity) ** power)`, default `b=0.15`, `power=4` |

`network_from_tables(links, turns=None, num_nodes=None, is_endpoint=None, allow_u_turns=False)` also
accepts a dict of numpy arrays instead of a DataFrame.

### Zones

A zone is a node: `origin` and `destination` are node ids. In networks with separate zone centroids,
pass `is_endpoint` (a bool per node) so routes can start and end at a centroid but never pass through it.
Trips with `origin == destination` are skipped.

### From a TNTP file

```python
net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
demand = pyta.Demand.from_tntp("SiouxFalls_trips.tntp", net)
```

Nodes are renumbered from 0, and centroids are marked automatically.

### From a GMNS dataset

```python
net = pyta.Network.from_gmns("path/to/gmns", length_unit="foot", speed_unit="mph")
demand = pyta.Demand.from_gmns("path/to/demand.csv", net,
                               origin="orig_taz", destination="dest_taz", flow="total",
                               skip_unknown_zones=True)
```

The directory needs `node.csv` and `link.csv`; `movement.csv` and `config.csv` are optional. Pass the
units when there is no `config.csv`. `net.node_ids` and `net.link_ids` map back to the ids in the files.

## Solver options

```python
pyta.assign(net, classes, gap=1e-6, max_iter=200, order="fixed", seed=0, chunk_size=0, threads=None)
```

| argument | meaning |
|---|---|
| `gap` | stop at this relative gap. `1e-4` is a rough answer, `1e-6` a good one. |
| `max_iter` | maximum number of iterations |
| `order` | `fixed`, `shuffle`, `alternate`, `largest_gap`, `largest_demand`, `smallest_demand`. `shuffle` or `largest_gap` can be faster on large networks. |
| `chunk_size`, `threads` | parallelism; the defaults use all cores |

The call releases the GIL, and the result does not depend on the number of threads.

Bad input raises `ValueError` with the reason, such as a missing column or an unreachable destination.
