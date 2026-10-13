# 0001. A zone is a node; no connectors

**Decision.** Demand is given between nodes. There is no connector concept. A per-node flag
`is_endpoint` marks nodes that paths may start or end at but not pass through.

**Why.** Connectors are artificial links whose placement and cost quietly bias results, and they add a link
type the whole pipeline has to carry. Many networks, Sioux Falls among them, have zones that are
ordinary intersections. Users who want several access points can add a node and ordinary links in
their data.

**The exception.** Benchmarks such as Anaheim have real centroids that traffic must not pass through.
Reproducing their published solutions needs that rule, so it exists as one boolean per node, filled
from `<FIRST THRU NODE>` by the TNTP reader.

**Consequences.** The turn table has no turns after a link that enters an endpoint, so the shortest-path
search needs no special check. A turn table is valid for one endpoint mask.

**Trade-off.** All of a zone's trips load at one node. For coarse zones that aggregates away access
detail, and a trip leaving a node pays no turn cost there.
