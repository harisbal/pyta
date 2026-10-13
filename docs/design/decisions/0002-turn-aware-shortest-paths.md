# 0002. Link-labelled shortest paths and an explicit turn table

**Decision.** The shortest-path search labels links, not nodes, and works from an explicit turn table
stored in CSR form indexed by incoming link.

**Why.** With bans or penalties, the best way into a node depends on the way out. A node-labelled
Dijkstra is wrong in that case. Labelling links makes turn costs and bans exact without building an expanded
graph.

**Turn table.** Built from the network: every pair `(e, f)` where `f` leaves the node `e` enters, minus
U-turns (unless allowed), turns at endpoint nodes, and banned turns. Exceptions are validated and an
exception that names a turn that does not exist is an error, which catches typos in user files. A
path is stored as its links and turn ids, so turn flows follow directly.

**Costs are separate arrays.** `turn_costs[turn id]` is a slice passed to the search, so fixed
penalties and flow-dependent delays use the same code.

**Validation.** The banned-turn example, free turns against node-based Dijkstra on Sioux Falls, and
non-zero turn costs against a naive relaxation.
