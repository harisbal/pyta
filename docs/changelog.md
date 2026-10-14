# Changelog

## 0.2.0

A cleaner Python API. Old names keep working with a `DeprecationWarning` and are removed in 0.3.0.

**Changed**

- The result is grouped by what it describes: `result.links`, `result.turns`, `result.paths`,
  `result.unrouted` and `result.diagnostics`. `result.summary()` prints a short account of the run.
- `network.assign(classes, ...)` is the documented way to run an assignment; `pyta.assign(network, classes, ...)`
  is the same function. Options after `classes` are keyword-only.
- `order` and `unreachable` are string enums, `pyta.Order` and `pyta.Unreachable`. Plain strings still work, and
  a wrong value lists the valid ones.
- The error for demand that no allowed path serves names the origin-destination pairs.
- `is_endpoint` is `is_centroid`, `zone_nodes_are_endpoints` is `zone_nodes_are_centroids`, and
  `Demand.skipped_flow` is `Demand.unknown_zone_flow`.

**Moving to the new names**

| before | now |
|---|---|
| `pyta.assign(net, demand, 1e-6)` | `net.assign(demand, gap=1e-6)` |
| `result.link_flow`, `result.turn_flow` | `result.links.by_class[0]`, `result.turns.by_class[0]` (first class), or `.flow` for all classes |
| `result.class_link_flow`, `result.class_turn_flow` | `result.links.by_class`, `result.turns.by_class` |
| `result.link_volume`, `result.turn_volume` | `result.links.volume`, `result.turns.volume` |
| `result.link_time`, `result.turn_time` | `result.links.time`, `result.turns.time` |
| `result.link_preload`, `result.turn_preload` | `result.links.preload`, `result.turns.preload` |
| `result.link_table()`, `result.turn_table()` | `result.links.table()`, `result.turns.table()` |
| `result.path_table(k)`, `result.paths(k)` | `result.paths.table(k)`, `result.paths.arrays(k)` |
| `result.unrouted_flow` | `result.unrouted.flow` |
| `result.unrouted_table()`, `result.unrouted()` | `result.unrouted.table()`, `result.unrouted.arrays()` |
| `result.sweep_gaps`, `result.times` | `result.diagnostics.sweep_gaps`, `result.diagnostics.times` |
| `result.objective` (`None` if undefined) | `result.diagnostics.objective` (NaN if undefined) |
| `is_endpoint=` | `is_centroid=` |
| `zone_nodes_are_endpoints=` | `zone_nodes_are_centroids=` |
| `demand.skipped_flow` | `demand.unknown_zone_flow` |

Result arrays are now read-only; copy one to change it. See [decision 0013](design/decisions/0013-grouped-result-and-checked-options.md).

## 0.1.1

- `assign(..., unreachable="skip")` leaves out demand that no allowed path serves and reports it on the result
  ([decision 0012](design/decisions/0012-skip-unreachable-demand.md)).
- Prebuilt wheels for Linux, macOS and Windows are attached to each release.

## 0.1.0

First public release: path-based user-equilibrium assignment with turn delays, several vehicle classes,
preloads, table results and TNTP and GMNS input.
