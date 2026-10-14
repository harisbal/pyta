# How it works

## User equilibrium

Each driver takes the quickest route, and a road gets slower as more drivers use it. The result is a
**user equilibrium**: between any origin and destination, every route in use takes the same time and no
unused route is quicker. In the [Getting started](../getting-started.md) example the three routes from
Home to Work end up equally fast.

A link's travel time follows the BPR curve

\[
t(v) = t_0\left(1 + b\left(\frac{v}{c}\right)^{n}\right)
\]

with free-flow time \(t_0\), capacity \(c\), volume \(v\), and by default \(b = 0.15\), \(n = 4\).
Turns can add a fixed penalty or their own BPR delay.

## The solver

PyTA uses **gradient projection**. For every origin–destination pair it keeps a few routes and
repeatedly moves drivers from the slower routes to the quickest one, adding new routes as they become
the quickest, until all routes in use cost the same. Because the routes are kept, path flows and
select-link analysis come for free.

**Frank-Wolfe** is also available as a simple reference solver (Rust only, single class). It converges
much more slowly.

## The relative gap

\[
\text{gap} = 1 - \frac{\text{total time if everyone took the quickest route}}{\text{total time of the current flows}}
\]

It is zero at equilibrium. At `1e-4` flows are typically within a few percent of the answer; at `1e-6`
within a few vehicles.

## Network rules

- **Zones are nodes.** Trips go from node to node, and there are no special connector links.
  Centroid nodes marked with `is_endpoint` can start or end a route but not be passed through.
- **Turns.** A route is a sequence of links joined by allowed turns, so turn bans and penalties are
  respected exactly. U-turns are banned unless you allow them.
- **Classes.** All classes share the road: travel times depend on the volume in car units,
  \(\sum_k \mathrm{pce}_k \times \text{vehicles}_k\). Each class then picks its own quickest routes, with its own
  extra costs and allowed links. With several classes there is no single objective, so convergence is
  judged by the relative gap alone.

## Limits

PyTA solves the **static** problem: no queues spilling back, and a turn's delay depends only on its own
traffic, so signal timings and opposing flows are not modelled. Several periods can be chained, with
unfinished trips carried over (see [time periods](periods.md)).
