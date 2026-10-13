# Turns

Junctions cost time, and some movements are not allowed. PyTA models this with turns.

## What a turn is

A turn is a pair of consecutive links `(e, f)`: arrive on `e`, leave on `f`. The **turn table** lists
every allowed turn, indexed by incoming link. Unless you say otherwise:

- every pair of links meeting at a node is allowed at zero cost,
- **U-turns are banned** (`allow_u_turns=True` allows them),
- there are no turns after a link that enters an endpoint node, because paths end there.

## Three levels of detail

| level | example | support |
|---|---|---|
| A. Fixed penalties and bans | no left turn; +15 s for a U-turn | yes |
| B. Delay depending on the turn's own volume | a BPR curve on the turn | yes |
| C. Interacting junction models | left-turn delay depends on opposing flow; signals | not yet |

A turn costs its fixed `penalty` plus its flow-dependent delay. In the Python tables:

```python
turns = pd.DataFrame({
    "from_link": [4, 4, 9],
    "to_link":   [7, 8, 7],
    "banned":    [False, True, False],
    "penalty":   [np.nan, np.nan, 0.5],
    "delay_free_flow": [0.2, np.nan, np.nan],   # a BPR delay on turn 4->7
    "delay_capacity":  [900.0, np.nan, np.nan],
    "delay_b":         [0.15, np.nan, np.nan],
    "delay_power":     [4.0, np.nan, np.nan],
})
net = pyta.network_from_tables(links, turns)
net.turn_links()        # which turn has which id
```

## Why the shortest path works on links

With turn restrictions, the best way into a node depends on the way out. A node-labelled Dijkstra
may keep the cheapest arrival at a junction and then find the onward turn banned, discarding a legal route
it should have kept.

PyTA labels **links**: `label[e]` is the cheapest cost to reach the end of link `e`, turn costs included.
Relaxing a link scans only its allowed turns. The banned turn A→J→K therefore forces the search onto the
legal longer route, which a node-labelled search cannot find.

## Paths and turn flows

A path stores its links and its turn ids. Turn flows come out directly, and the gradient-projection
step includes the turns in its cost and slope sums: forgetting them would give a wrong step size and
make the method oscillate.

## Checked against a model without turns

The turn code is validated by an independent construction. Every link becomes an edge, every turn a
pseudo-edge between two link edges, and each zone gets zero-cost source and sink edges. A link-only solver
on that graph is the same equilibrium problem, so its flows must equal the turn-aware flows. They do, on
Sioux Falls (under 0.05 vehicles) and Anaheim (under 1 vehicle).
