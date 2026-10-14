# 0009. GMNS input: zones span nodes, movements restrict listed links

**Decision.** `pyta-io` reads GMNS datasets (`node`, `link`, optional `movement` and `config`) in Rust. Four rules
follow from the specification and from trying the official Lima example.

1. **A zone is the set of nodes that carry its `zone_id`.** This amends [0001](0001-zone-equals-node.md). Demand
   of `q` trips between zones `o` and `d` is split equally over every pair of one node of `o` and one node of `d`
   (`q / (n_o * n_d)` each), so totals are preserved. Zone nodes may optionally be endpoints.
2. **`movement.csv` restricts an inbound link only if it lists a movement for it.** Listed movements are allowed
   with their `penalty` (seconds, converted to minutes); every other turn from that link is banned. Links the
   table never mentions keep all turns.
3. **Units come from `config.csv`, and the options override it.** Free-flow time is `length / free_speed`,
   capacity is `capacity * lanes` (GMNS capacity is per lane), and the BPR parameters default to 0.15 and 4
   unless the link has `vdf_alpha` / `vdf_beta`. A missing length, speed, capacity or lane count is an error.
4. **Demand has its own file.** GMNS defines no demand table; the reader takes `origin, destination, flow`
   columns, with configurable names.

**Why.** Lima has 395 zones, 376 of which span 2 to 104 nodes, so a strict one-node-per-zone rule cannot load
the one official dataset with demand. Its `movement.csv` has no row for any inbound link that leaves a zone
node, so banning everything unlisted at a junction cut the network in two. Its `directed` column is blank and
its `config.csv` says miles for lengths that are in feet, so a blank `directed` means directed and the unit
options exist.

**Consequences.**
- Undirected links (`directed = false`) become two links, so the link-direction optimiser can switch each
  direction on and off.
- A pair whose origin and destination node coincide is dropped by the assignment. For intrazonal demand that
  loses the share `1 / n` of the zone's trips.
- Equal weights are a simplification. A node weight column is the natural extension if equal splits prove too crude.
- Demand rows for zones without a node are an error, or are skipped and reported with `skip_unknown_zones`.
  Lima needs this for 25 zones, all of which exist as node ids but not as `zone_id` values.
- Splitting multiplies OD pairs: Lima goes from 13 000 rows to 352 606 pairs.
