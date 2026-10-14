# 0012. Unreachable demand: an error by default, skipped on request

**Decision.** `assign` takes `unreachable="error"` (default) or `"skip"`. With `"skip"`, demand between nodes that
no allowed path joins is removed before the solve and reported in the result (`unrouted_flow`, `unrouted()`,
`unrouted_table()`). The core exposes `find_unrouted` and `OdDemand::retain` for the same purpose.

**Why.**
- A network that changes between runs (closures, banned links for one class) can disconnect zones, and a
  caller that expects this has to find the unreachable pairs itself to keep the solver from failing.
- Whether a path exists does not depend on link costs, so one search per origin and class, with unit costs,
  decides it for the whole solve. The solver then never meets an unreachable pair.
- The default stays an error: a silently dropped trip in a model that is expected to be connected is a bug in
  the input, and the unrouted flow is only visible if the caller looks.

**Consequences.**
- Results cover the routed demand only. Link flows, paths and the gap do not include skipped trips.
- The check costs one extra shortest-path search per origin and class, run in parallel. It is skipped with
  `"error"`.
- A pair is dropped for every duplicate row of the same `(origin, destination)` in a class.
