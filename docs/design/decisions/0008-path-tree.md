# 0008. Paths as a tree of steps, per origin

**Decision.** Each origin stores its paths as a tree. A *step* is "arrive on link `L`, coming from
step `P`", and the root stands for the origin. A path is the step where it ends plus its flow, so
paths that start the same way share the steps of that start.

**Why.** On Chicago Regional the paths of one origin leave it along the same roads: after the
gap-1e-3 solve, 100.7 M link entries collapse into 12.1 M distinct steps (8.3×; Chicago Sketch 5.2×).
Storing every path in full also stored every shared start again. The first design (link ids plus a
parallel arena of turn ids) held about 1.9 GB of live data for 2.3 M OD pairs and peaked at 3.0 GB.
The tree brought the peak down to about 1.3 GB.

A first idea, storing only the first link and the turn ids of each path, would have saved only about
0.7 GB (the link arena's share). Counting allocations showed 1.9 GB live after the solve, of which the two
arenas were 1.5 GB; the process peak of 3.2 GB also included 0.6 GB of temporary buffers (regrown vectors,
rebuilds) and 0.6 GB the allocator kept after freeing.

**Layout.** A step holds its parent, link, turn, depth and a first-child / next-sibling pair (24
bytes). A step's id is always larger than its parent's. Equal paths end at the same step, so finding
out whether a path is already known is a walk down the tree and a comparison of ids.

**Where two paths differ.** The flow update needs only the stretch where two paths of an OD pair
differ. The tree finds it by walking up from both ends until the walks meet (the common start), then
walking back from both ends in step while the links agree (the common finish). The shared start, the
longest part of a path, is never read. The result is tested against trimming the two full paths, for
every pair of paths in randomly generated trees (this caught a case where a turn into the common finish
is shared because the links before it agree).

**Flows and costs per step.** Rebuilding link and turn flows from path flows puts each path's flow on
its last step, adds it up towards the root and gives each step's total to its link and turn once. The
exact gap check likewise computes the cost from the origin to every step in one pass. Both are one
pass over the steps instead of one over every link of every path. On Chicago Regional the rebuild
dropped from about 10 s to about 4 s.

**Cleaning up.** Removing a path with no flow leaves its steps in place, since other paths may share
them. When a tree has doubled since it was last rebuilt, the steps on the way to some live path are kept and
the rest dropped, with ids renumbered in their old order. Nothing is counted on the hot path.

**What it cost.** Reading a path means following parent links, so listing a path allocates
(`PathStore::iter` now returns owned vectors) and the flow updates on Chicago Regional take a
little longer than slice comparisons did. Totals are in the [benchmarks](../../benchmarks.md#large-networks).
Float sums are formed in a different order than before, so results agree with older runs to rounding
but are not bit-identical to them.
