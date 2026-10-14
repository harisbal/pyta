# Time periods

A period is one static assignment: trips depart uniformly over its duration and the network is in
equilibrium. `assign_periods` runs several periods in a row. Trips that have not arrived by the end of
a period are not assigned again; they **start the links they still have to travel with some volume** in
the next period.

```python
am = pyta.Period(180, pyta.TrafficClass(am_demand))      # duration in minutes
pm = pyta.Period(180, pyta.TrafficClass(pm_demand))
am_result, pm_result = pyta.assign_periods(net, [am, pm], gap=1e-5)

pm_result.link_preload      # vehicles already on each link when the second period begins
pm_result.link_volume       # the second period's own trips
```

Each result is an ordinary `AssignResult` with two extra arrays, `link_preload` and `turn_preload`
(`None` for plain `assign`). Travel times in a period depend on `link_volume + link_preload`.

## Units

Travel times and durations are in minutes. **Link and turn capacities are per hour**; the solver
multiplies them by `duration / 60` for each period, and a period's demand is in trips per period. A
one-hour period therefore gives the same answer as `assign`.

## How much is left over

Take a path with flow $q$ and travel time $\tau$ in a period of length $T$. A trip departing at time $s$
is still travelling at the end if $s + \tau > T$. Departures are uniform, so the trips still on the
network are spread uniformly along the path **in travel time**. If link $j$ occupies the
travel-time interval $[a_j, b_j]$ of the path (its own time plus the fixed penalty and delay of the turn
onto the next link), the trips on it at the end are

\[
c_j = q \,\frac{\bigl|[a_j, b_j] \cap [0, \min(\tau, T)]\bigr|}{T}.
\]

Summed over the path this is $q \min(\tau, T) / T$: flow times time on the network, divided by the
period (Little's law).

## The preload

The $c_j$ trips are on link $j$ and will go on along the rest of the path, so in the next period every
**later link** of that path, and every turn from link $j$ onwards, carries $c_j$ extra vehicles (times the class's PCE).
Summed over all paths this is the preload. It is added to the volume the delay functions see, and the
solver does nothing else differently.

## Approximations

- The carried trips are not routed again, and they are not carried on a second time if their remaining
  trip is longer than the next period.
- A carried trip is placed at the end of the link it is on, so that link gets no preload from it.
- The preload loads the whole next period, although in reality those trips are there at its start.
- To see what is left after the last period, add one with no demand.

The reasoning is in [decision 0010](../design/decisions/0010-time-periods.md).
