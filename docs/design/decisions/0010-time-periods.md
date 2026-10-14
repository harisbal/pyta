# 0010. Time periods: unfinished trips preload the next period

!!! warning "Superseded by [0011](0011-explicit-preloads.md)"
    `assign_periods` and the automatic carry-over were removed. The record is kept for the reasoning.

**Decision.** `assign_periods` solves each period as a static multi-class assignment. Trips still
travelling at the end of a period are not assigned again. On a path of travel time $\tau$ in a period
of length $T$ they leave $\min(\tau, T)/T$ of its flow on the network, spread along the path in proportion to
travel time. In the next period, the links they still have to travel (and the turns from there on) start with
that volume, times the class's PCE. The preload is added to the flow the delay functions see.

**Why.** The first version turned unfinished trips into demand from the end node of the link they were on
and assigned them again. That adds a class-matching rule, a demand merge, and trips that can switch to
another route halfway, which they would not do. All the model needs is for the links these trips will use
to be busier. A preload does that, needs no change to the solver (a wrapper around the delay function adds
it), and keeps periods independent of each other's classes.

**Units.** Durations are minutes, capacities per hour, demand per period. The solver scales capacity
by `duration / 60` for each period, so one network table serves periods of any length.

**Consequences.**
- Periods may have different classes; the preload is PCE-weighted at the time it is computed.
- The results are plain solver results plus `link_preload` and `turn_preload`; there is no separate period result type
  in Python.
- Periods run in order, each to its own gap, with no warm start between them.
- The Beckmann objective is not reported for periods.
- A preload is not carried on again if its remaining trip outlasts the next period, so very short periods
  underestimate congestion.

**Not done.** Resuming at the exact position inside a link, loading carried trips at the start of the
period only, and time-varying capacities within a period.
