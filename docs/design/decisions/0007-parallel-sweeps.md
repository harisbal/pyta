# 0007. Parallel sweeps and the hot path

**Decision.** A sweep processes origins in *chunks*. The shortest-path searches of a chunk's
origins run in parallel (rayon); the flow shifts are then applied one origin at a time, in order.
The initial assignment and the exact gap check run fully in parallel. `chunk_size = 1` is the plain
sequential sweep.

**Why this split.** Gauss-Seidel needs each flow shift to see the previous one's costs, so the
shifts must stay sequential. The searches only *generate candidate paths*. A path found with costs
that are a few origins old is still a valid path, and the shift that uses it always prices it with
current costs. So the expensive part parallelises without changing what the method converges to.

**Determinism.** Each origin's search is independent of the others, results are collected in
order, and the gap sums are formed per origin and added in order. So a given `chunk_size` gives
bit-identical results on 1 thread or 8 (tested). Different chunk sizes follow slightly different
iteration paths and reach the same equilibrium (tested on Sioux Falls, all within 0.1 vehicles at gap 1e-8).

**What it cost in convergence.** A larger chunk means staler candidate paths early on, so the
first sweeps reduce the gap a little more slowly (Chicago Regional after two sweeps: gap 9.2e-2
with chunk 16, against 6.5e-2 sequentially). See [benchmarks](../../benchmarks.md#large-networks).

## Other changes made for speed

Profiling on Chicago Regional (2.3 M OD pairs, 39 k links) showed where time went, in order of effect:

1. **No allocation in the loops.** `TurnSearch` reuses its buffers, paths are written into
   reusable vectors, and the starting assignment builds each origin's paths without intermediate
   allocations.
2. **Work only where paths differ.** Two paths of one OD pair share a long prefix and suffix. They
   cancel out of every cost comparison and every shift, so the update trims them and works on the stretch
   where the paths differ. The shortest path just found is used as the reference, so no full path is ever
   priced. On Chicago Sketch this made the flow updates 4× faster.
3. **Cheaper delay evaluation.** After each shift, every touched link and turn gets its cost and
   derivative refreshed. They are now computed together (sharing `x^(p-1)`), with `1/capacity`
   precomputed and the four BPR parameters stored side by side. See the numbers in the benchmarks.
4. **Fewer exact gap checks.** The cheap in-sweep gap estimate understates the exact gap (by about 8×
   on Chicago Regional). The solver learns that ratio and only runs the exact check, which costs
   about a sweep's worth of searches, when the scaled estimate could pass.

## What is not parallel

The flow shifts, the end-of-sweep rebuild of volumes, and the path bookkeeping are sequential. They
are the remaining bottleneck on large networks. A Jacobi-style parallel update inside a chunk would
oscillate for the same reason per-OD-pair updates did ([0003](0003-gradient-projection.md)), so
it is not planned.
