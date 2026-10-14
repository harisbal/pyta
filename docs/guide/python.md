# Python guide

```python
import pyta
```

All arrays are numpy arrays. Node ids are integers starting at 0. **Link ids are the row numbers**
of the link table (0, 1, 2, ...). **Turn ids** follow the network's turn table; use
`net.turn_links()` to see them.

## Building a network

### From a TNTP file

```python
net = pyta.Network.from_tntp("SiouxFalls_net.tntp")
```

Node numbers become 0-based, links keep file order, and `<FIRST THRU NODE>` becomes the endpoint mask.

### From a GMNS dataset

```python
net = pyta.Network.from_gmns("path/to/gmns", length_unit="foot", speed_unit="mph")
demand = pyta.Demand.from_gmns("path/to/demand.csv", net,
                               origin="orig_taz", destination="dest_taz", flow="total",
                               skip_unknown_zones=True)
net.node_ids, net.link_ids     # the ids as written in the files
demand.skipped_flow            # trips of zones without a node
```

The directory needs `node.csv` and `link.csv`; `movement.csv` and `config.csv` are optional. Units
come from `config.csv` unless you pass `length_unit` / `speed_unit`, and you must pass them when
there is no config or when it is wrong. Times are in minutes. Undirected links become two links,
a zone's trips are split equally over the nodes that carry its `zone_id`, and a movement table
restricts an inbound link only if it lists a movement for it. GMNS has no demand table, so
`Demand.from_gmns` reads a flat zone-to-zone CSV. The reasons are in
[decision 0009](../design/decisions/0009-gmns-input.md).

### From tables

```python
net = pyta.network_from_tables(links, turns=None, num_nodes=None,
                               is_endpoint=None, allow_u_turns=False)
```

`links` is a DataFrame or a dict of arrays:

| column | meaning |
|---|---|
| `from_node`, `to_node` | integer node ids |
| `free_flow_time` | delay at zero flow |
| `capacity` | must be positive |
| `b`, `power` | BPR parameters, optional (defaults 0.15 and 4) |

Link delay is the BPR function `free_flow_time * (1 + b * (volume / capacity) ** power)`.
`num_nodes` defaults to the largest node id plus one.

`is_endpoint` is a bool per node. A path may start or end at an endpoint node but never passes
through it. Use it for zone centroids. If every zone is an ordinary intersection, leave it out.

### Turns

`turns` is optional and lists exceptions and delays by link ids:

| column | meaning |
|---|---|
| `from_link`, `to_link` | required; the two links must meet at a node |
| `banned` | bool; the turn does not exist |
| `penalty` | fixed extra delay; NaN means none |
| `delay_free_flow`, `delay_capacity`, `delay_b`, `delay_power` | a BPR delay on the turn's own volume; give all four or none |

Turns you do not list are allowed at zero cost. **U-turns are banned** unless `allow_u_turns=True`;
an explicit penalty on a U-turn allows that one. There are no turns after a link that enters an
endpoint node.

## Demand

```python
demand = pyta.demand_from_table(trips, num_nodes=net.num_nodes)   # origin, destination, flow
demand = pyta.Demand.from_tntp("SiouxFalls_trips.tntp", net)
demand.total()
```

Zones are nodes: `origin` and `destination` are node ids. Trips with `origin == destination` use no
network and are skipped.

## Assigning

```python
result = pyta.assign(net, demand, gap=1e-6, max_iter=200, order="fixed", seed=0,
                     chunk_size=0, threads=None)
```

| argument | meaning |
|---|---|
| `classes` | a `Demand`, a `TrafficClass`, or a list of them |
| `gap` | stop when the relative gap is at or below this |
| `max_iter` | maximum number of sweeps over all origins |
| `order` | origin order per sweep: `fixed`, `shuffle`, `alternate`, `largest_gap`, `largest_demand`, `smallest_demand` |
| `seed` | seed for `shuffle` |
| `chunk_size` | origins whose shortest paths are searched in parallel before their flow shifts are applied. `0` (default) picks about 1/32 of the origins, between 1 and 64; `1` is the plain sequential sweep |
| `threads` | number of threads for the parallel parts (default: all cores, or `RAYON_NUM_THREADS`) |

The call releases the GIL, so several assignments can run in threads. Results do **not** depend on the
number of threads: a given `chunk_size` always gives the same answer. `chunk_size` itself changes the
path of the iteration slightly (see [parallel sweeps](../design/decisions/0007-parallel-sweeps.md)),
but not the equilibrium it converges to.

### Classes

```python
pyta.TrafficClass(demand, pce=1.0, extra_link_cost=None, allowed_links=None)
```

- `pce`: how many passenger-car units one vehicle loads on the network. Delays depend on the
  PCE-weighted volume of all classes.
- `extra_link_cost`: per link, in the same units as travel time. Convert tolls with the class's
  value of time before passing them in.
- `allowed_links`: bool per link. A turn is usable only if both its links are.

## Several periods

```python
first, second = pyta.assign_periods(net, [pyta.Period(180, am_classes), pyta.Period(180, pm_classes)])
second.link_volume      # the second period's own trips
second.link_preload     # trips left over from the first, already on their remaining links
```

Durations are minutes and capacities per hour; see [time periods](../concepts/periods.md).

## Results

| attribute | meaning |
|---|---|
| `link_flow`, `turn_flow` | vehicles of the first class per link and per turn |
| `class_link_flow`, `class_turn_flow` | lists with one array per class |
| `link_volume`, `turn_volume` | PCE-weighted volumes (what delays depend on) |
| `relative_gap` | exact relative gap at the final flows |
| `iterations`, `converged`, `sweep_gaps` | sweeps done, whether the gap target was met, per-sweep gap estimate |
| `objective` | Beckmann objective; `None` unless there is one class with PCE 1 |
| `times` | seconds per phase: `initial_assignment`, `shortest_paths`, `flow_updates`, `sweep_rebuild`, `gap_checks` |
| `paths(class_index=0)` | every path as flat arrays |

`paths()` returns a dict: `origin`, `destination` and `flow` (one entry per path), `link_offsets`
(one more entry) and `links` (all link ids back to back). Path `i` uses
`links[link_offsets[i]:link_offsets[i + 1]]`.

```python
p = result.paths()
through_link_7 = np.array([7 in p["links"][a:b] for a, b in zip(p["link_offsets"][:-1], p["link_offsets"][1:])])
select_link_flow = p["flow"][through_link_7].sum()
```

## Errors

Bad input raises `ValueError` with the reason (an unknown order, a missing column, a node id out of
range, two links that do not meet in a turn row, demand to a node a class cannot reach, ...).
`result.paths(class_index=n)` raises `IndexError` for a class that does not exist.

## What the gap means

The relative gap is `1 - (cost of everyone on their cheapest path) / (cost of the current flows)`.
At 1e-4 flows are typically within a few percent of the equilibrium; at 1e-6 within a few
vehicles on a network with hundreds of thousands of trips (Sioux Falls: 1.3 vehicles). Gradient
projection reaches 1e-6 quickly but has a slow tail below about 1e-8.
