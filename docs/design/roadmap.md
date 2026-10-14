# Roadmap and limitations

## Limitations today

- **Static.** No queues spilling back; each period is an independent assignment.
- **Deterministic route choice.** Travellers are assumed to know travel times exactly (deterministic user
  equilibrium). This is a deliberate choice: stochastic user equilibrium is deferred, see below.
- **Separable delays.** A turn's delay depends on its own volume. Interacting junction models (signals,
  gap acceptance, roundabouts) make the cost Jacobian asymmetric and are not implemented.
- **Multi-class has no objective** (asymmetric when PCEs differ) and is gradient projection only.
- **Path memory** grows as zones² × paths per OD × (the part of a path not shared with others from the
  same origin). The path tree keeps Chicago Regional (2.3 M OD pairs) at about 1.3 GB, but a 5 000-zone
  model can still need many gigabytes.
- **Partly parallel.** Shortest-path searches, the initial assignment and gap checks use all cores; the flow shifts are sequential by design (Gauss-Seidel).

## Planned

| item | notes |
|---|---|
| Path memory | the per-OD bookkeeping (a vector of paths per OD pair) now makes up a large share of what is left; Sydney (3 264 zones) and Austin (7 388 zones) are untested and may need bush-based storage |
| Faster flow updates | the remaining bottleneck on large networks (about 40% of the time on Chicago Regional) |
| OpenStreetMap networks | download and conversion belong in the Python layer (osm2gmns or osmnx), producing GMNS or flat tables |
| Link-direction optimiser | built on the assignment engine; see below |
| Bush-based solver | a fallback for the largest networks, with origin-level memory |
| Interacting junction models | signalised and priority junctions |
| Stochastic user equilibrium | deferred: PyTraf keeps the deterministic equilibrium for now. When added, path-based logit SUE (Fisk, 1980) fits the existing solver best: the gradient-projection step equalises \(c_p + \ln f_p / \theta\) instead of \(c_p\), with a path-size correction for overlapping routes. Open questions are the choice set (today only paths that were once shortest) and the convergence measure |

## Link-direction optimisation

The planned first consumer of the engine is an optimiser that chooses the direction of links.

- The decision is **discrete** (each link one way, the other way, or both), so gradient descent and Newton's method apply only
  to a continuous relaxation, for instance splitting a corridor's capacity between directions.
- For the relaxation, gradients come from sensitivity analysis of the equilibrium, which needs the path
  sets and cost derivatives gradient projection already has.
- Simulated annealing fits the discrete problem directly but is costly: every evaluation is an assignment.
  With very few decision links, a surrogate model fitted to a small design of experiments, or near-exhaustive
  search, is attractive.
- The engine should support warm starts (reuse the previous equilibrium after a small change) and a loose
  gap for inner evaluations. Keeping both directions as links and switching them on and off with
  a mask avoids rebuilding the graph.
