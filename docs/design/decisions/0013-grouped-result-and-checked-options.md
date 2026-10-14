# 0013. A grouped result, checked options and `network.assign`

**Decision.** The Python result is grouped by what it describes instead of a flat list of names:

- `result.links` and `result.turns` each hold `flow` (vehicles), `by_class`, `volume` (car units), `preload`,
  `time` and `table()`; links also have `voc`.
- `result.paths` (`table()`, `arrays()`), `result.unrouted` (`flow`, `total`, `table()`, `arrays()`) and
  `result.diagnostics` (`sweep_gaps`, `times`, `objective`) hold the rest. `converged`, `iterations` and
  `relative_gap` stay on the result.
- `assign` takes everything after `classes` as keyword-only, and `order` and `unreachable` are string enums
  (`Order`, `Unreachable`) that reject unknown values with a message listing the valid ones.
- `assign` is also a method: `network.assign(classes, ...)`. `pytraf.assign(network, classes, ...)` stays and is
  the same function.
- `is_endpoint` is `is_centroid`, `zone_nodes_are_endpoints` is `zone_nodes_are_centroids` and
  `Demand.skipped_flow` is `Demand.unknown_zone_flow`.
- The error for demand no allowed path serves names the first five origin-destination pairs, how many there
  are and the trips they carry.

**Why.**
- Ten result names were a link/turn by quantity grid (`link_flow`, `turn_flow`, `class_link_flow`, ...). Grouping
  by element makes the name say once what it is about, lets `links` and `turns` share their shape, and leaves
  room to add quantities without growing the top level.
- Attribute names match the table columns (`flow`, `volume`, `preload`, `time`, `voc`), so the arrays and the
  tables read the same. `link_flow` meant the first class only; `by_class[0]` says so.
- A bare number after `classes` could be any of five options. Keyword-only removes the question.
- A string option fails late and with a bare message; an enum lists its values and completes in an editor,
  while plain strings keep working.
- The network owns the demand's node numbering and the delay functions; `network.assign(demand)` reads as what
  it does and is found by completion.
- `is_endpoint` described the graph; the thing it marks is a zone centroid, and `Demand.skipped_flow` counts
  trips whose zone has no node, which the name did not say.

**Consequences.**
- The group objects are plain Python over the extension's arrays (the Rust result is private). Cached arrays
  are read-only.
- The old names still work and raise a `DeprecationWarning` naming the replacement. They are removed in 0.3.0.
- `diagnostics.objective` is NaN where it was `None`; it is defined only for one class with PCE 1.
- The string enums are `enum.StrEnum` from Python 3.11 and an equivalent `str`/`Enum` before that.
