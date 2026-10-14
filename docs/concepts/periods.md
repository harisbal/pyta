# Time periods

Each period is a separate equilibrium in which trips depart evenly over its duration.
`assign_periods` runs periods in order, and trips still on the road when one period ends add to the
traffic of the next.

```python
early = pyta.Period(60, pyta.TrafficClass(demand))     # minutes
late  = pyta.Period(60, pyta.TrafficClass(demand))
first, second = pyta.assign_periods(net, [early, late])
second.link_preload      # vehicles left over from the first period
```

## Units

Durations are in minutes, **capacities stay per hour**, and demand is trips per period. A single
60-minute period gives the same answer as `assign`.

## How much is carried over

Departures are spread evenly, so the trips still travelling at the end of a period are spread evenly
along each route, measured in travel time. A route with \(q\) trips and travel time \(\tau\) in a period
of length \(T\) has \(q \min(\tau, T) / T\) trips left on it.

Each of those trips is placed on the link it has reached and adds to the volume of the **links still
ahead of it** in the next period. In the Home → Work example, 179 vehicles still have the last ring-road
link ahead of them at 8:00, so it starts the second hour with that much extra traffic.

## Approximations

- Carried-over trips keep their route, and are carried only one period forward.
- They load the whole next period, although in reality they clear early in it.
- To see what is left after the last period, add one with no demand.
