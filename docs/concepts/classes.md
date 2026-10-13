# Traffic classes

A class is a group of vehicles with its own demand and its own view of the network.

| setting | meaning |
|---|---|
| `demand` | the class's trips |
| `pce` | passenger-car units one vehicle loads on the network (cars 1.0, trucks 2.5) |
| `extra_link_cost` | per-link cost added for this class: tolls, distance charges |
| `allowed_links` | links the class may use; a truck ban is `False` there |

```python
cars   = pyta.TrafficClass(car_demand)
trucks = pyta.TrafficClass(truck_demand, pce=2.5,
                           extra_link_cost=toll / value_of_time,
                           allowed_links=~truck_banned)
result = pyta.assign(net, [cars, trucks])
```

## How classes interact

All classes share the road. A link's delay depends on the **PCE-weighted volume**

\[
v_a = \sum_k \mathrm{pce}_k\, x_{k,a}
\]

where \(x_{k,a}\) is the number of class-\(k\) vehicles on link \(a\). Each class then chooses its own
cheapest paths with cost = travel time + its own extra cost, using only its allowed links. A turn is usable
by a class only if both of its links are.

`extra_link_cost` is in time units, so convert a toll with the class's value of time yourself
(`toll / value_of_time`).

## Results

`result.class_link_flow[k]` holds the vehicles of class `k` per link, `result.link_volume` the
PCE-weighted total that drives the delays, and `result.paths(k)` the paths of class `k`.

## What to know

- With different PCEs the equilibrium problem is **asymmetric**: there is no single objective function
  to minimise. PyTA solves it with gradient projection and judges convergence by the relative gap. The
  Beckmann `objective` is reported only for a single class with PCE 1.
- Frank-Wolfe does not support several classes, since its line search needs an objective.
- Two identical classes reproduce the single-class equilibrium exactly, which is one of the tests.
