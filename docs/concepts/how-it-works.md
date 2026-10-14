# How it works

## Problem formulation

Let \(G = (N, A)\) be a directed network and \(q_{od}\) the demand between origin \(o\) and
destination \(d\). Each link \(a\) has a travel-time function \(t_a(v_a)\) of its volume \(v_a\). A flow
pattern is a **user equilibrium** if it satisfies Wardrop's first principle (Wardrop, 1952): for every
OD pair, all used paths have equal cost, and no unused path has a lower cost. In the
[Getting started](../getting-started.md) example, the three routes from Home to Work carry different
flows but have the same travel time at equilibrium.

When each link cost depends only on that link's own volume, the equilibrium is the solution of the
convex program of Beckmann et al. (1956):

\[
\min_{f \ge 0} \; \sum_a \int_0^{v_a} t_a(x)\,dx
\quad\text{subject to}\quad
\sum_{p \in P_{od}} f_p = q_{od} \;\; \forall\, (o,d),
\qquad v_a = \sum_{p \ni a} f_p,
\]

where \(f_p\) is the flow on path \(p\) and \(P_{od}\) the set of paths from \(o\) to \(d\).

Link travel times follow the Bureau of Public Roads (1964) function

\[
t_a(v_a) = t_a^0\left(1 + b\left(\frac{v_a}{c_a}\right)^{n}\right),
\]

with free-flow time \(t_a^0\), capacity \(c_a\) and, by default, \(b = 0.15\) and \(n = 4\). Turns may carry a
fixed penalty or a delay of the same form in the turn's own volume.

## Solution method

PyTraf solves the problem in path space with **gradient projection** (Jayakrishnan et al., 1994). For each
OD pair it maintains a small set of active paths. Each iteration computes the shortest paths at the current costs,
adds any new shortest path to the active set, and shifts flow from the costlier paths towards the
shortest one using a second-order (Newton) step. Origins are processed one after another, and link
costs are updated after every shift.

Because path flows are retained, path-level outputs such as select-link analysis are available
directly. The price is memory, which grows with the number of OD pairs and active paths.

The **Frank-Wolfe** algorithm (Frank & Wolfe, 1956) is included as a link-based reference method. It
is simple and memory-efficient but converges sublinearly near the solution.

## Convergence

Convergence is measured by the **relative gap**

\[
\text{gap} = 1 - \frac{\sum_{od} q_{od}\, \kappa_{od}}{\sum_a v_a\, t_a(v_a)},
\]

where \(\kappa_{od}\) is the shortest-path cost between \(o\) and \(d\) at the current flows. The gap is
non-negative and zero exactly at equilibrium. A gap of \(10^{-4}\) is usually adequate for planning
studies; \(10^{-6}\) and below suits comparisons between scenarios.

## Modelling assumptions

- **Zones are nodes.** Demand is defined between nodes and no connector links are needed. Nodes
  flagged as endpoints (centroids) may begin or end a path but may not be traversed.
- **Turns.** Shortest paths are computed with link-based labels (Kirby & Potts, 1969), so turn
  prohibitions and penalties are represented exactly. U-turns are prohibited by default.
- **Multiple classes.** Classes share the network: link costs depend on the total volume in
  passenger-car equivalents, \(v_a = \sum_k \mathrm{pce}_k\, x_{k,a}\), and each class chooses paths by its own
  generalised cost over its permitted links. When PCEs differ, the cost interactions are asymmetric
  and no equivalent optimisation problem exists (Dafermos, 1972). The equilibrium is still found by
  gradient projection, and convergence is judged by the relative gap alone.
- **Static and separable.** Queues do not spill back, and every cost depends only on its own link or
  turn volume, so interactions such as opposing flows at signals are not modelled. Fixed background
  traffic can be added to the volumes as a preload.

## References

- Beckmann, M., McGuire, C. B., & Winsten, C. B. (1956). *Studies in the Economics of Transportation*.
  Yale University Press.
- Bureau of Public Roads (1964). *Traffic Assignment Manual*. U.S. Department of Commerce.
- Dafermos, S. C. (1972). The traffic assignment problem for multiclass-user transportation networks.
  *Transportation Science*, 6(1), 73–87.
- Frank, M., & Wolfe, P. (1956). An algorithm for quadratic programming. *Naval Research Logistics
  Quarterly*, 3(1–2), 95–110.
- Jayakrishnan, R., Tsai, W. K., Prashker, J. N., & Rajadhyaksha, S. (1994). A faster path-based
  algorithm for traffic assignment. *Transportation Research Record*, 1443, 75–83.
- Kirby, R. F., & Potts, R. B. (1969). The minimum route problem for networks with turn penalties and
  prohibitions. *Transportation Research*, 3(3), 397–408.
- Wardrop, J. G. (1952). Some theoretical aspects of road traffic research. *Proceedings of the
  Institution of Civil Engineers*, 1(3), 325–378.
