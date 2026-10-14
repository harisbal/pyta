# 0011. Explicit preloads instead of carried-over trips

**Decision.** PyTA no longer carries unfinished trips from one period into the next. `assign_periods`,
`Period` and the carry-over computation are removed. In their place, `assign` takes optional
`link_preload` and `turn_preload`: fixed volumes in car units that are not routed but add to the volume
the delay functions see. In Rust the same is done by wrapping a delay function in `Preloaded`.

**Why.**
- The carried share, \(q \min(\tau, T)/T\), depends on path travel times. In a static model these come
  from volume-delay functions with no queues, and they are inflated on overloaded links. More traffic is
  then carried, which congests the next period further: the feature amplifies a known weakness of the
  static model.
- With periods of two or three hours the carried share is a few percent and changes little. It matters only
  for short periods relative to trip times, where a dynamic model is the right tool.
- The approximations (no rerouting, no second carry-over, spread over the whole next period) were hard to
  defend together, and static tools commonly run periods independently.
- A fixed preload is the general, standard mechanism (background traffic, buses, through traffic) and
  needed no new solver code.

**Consequences.**
- Several periods are separate `assign` calls, with capacities scaled to each period's length by the caller.
- A user who wants carry-over can compute it from `path_table()` and pass it as the next preload, as an
  explicit modelling choice.
- The Beckmann objective with a preload counts only the area above the preload, so it is still reported for
  a single class with PCE 1.
