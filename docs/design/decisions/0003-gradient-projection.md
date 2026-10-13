# 0003. Gradient projection with a refresh after every path shift

**Decision.** The main solver is path-based gradient projection, Gauss-Seidel by origin. After every
single path-to-path shift, the cost and derivative of the touched links and turns are refreshed.

**Why path-based.** Paths give path flows, select-link analysis and turn flows for free, and gradient
projection reaches tight gaps far faster than Frank-Wolfe.

**Why refresh after every shift.** The first version refreshed after each OD pair, computing all of an
OD pair's shifts from one cost snapshot. That works when delays are mild. With strong turn delays on Anaheim, several paths shifting onto the same links overshoot, and the
gap stalled and oscillated around 1e-4 for 2 000 sweeps. Refreshing after each shift converged smoothly
in 160 sweeps and reached a lower objective (3 131 848.07 against 3 131 850.7).

On link-only benchmarks the two behaved identically (Chicago Sketch 4.60 s against 4.57 s), because
refreshing a touched link costs about as much as shifting flow on it, and shortest-path search
dominates. The per-OD mode was removed.

**Other details.**

- The relative gap is checked cheaply during a sweep, then confirmed exactly with fresh shortest paths
  at one cost snapshot.
- Link volumes are rebuilt from path flows at the end of each sweep to remove drift.
- Origin order is configurable; the default stays `fixed` ([benchmarks](../../benchmarks.md#origin-order)).
