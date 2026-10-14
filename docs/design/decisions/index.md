# Decision records

Short notes on the choices that shaped PyTA, with the reason for each.

| # | decision |
|---|---|
| [0001](0001-zone-equals-node.md) | A zone is a node; no connectors |
| [0002](0002-turn-aware-shortest-paths.md) | Link-labelled shortest paths and an explicit turn table |
| [0003](0003-gradient-projection.md) | Gradient projection with a refresh after every path shift |
| [0004](0004-multi-class.md) | Multi-class model: PCE, extra costs, allowed links |
| [0005](0005-python-api.md) | Python API conventions |
| [0006](0006-frank-wolfe-reference.md) | Frank-Wolfe stays a reference solver |
| [0007](0007-parallel-sweeps.md) | Parallel sweeps and the hot path |
| [0008](0008-path-tree.md) | Paths as a tree of steps, per origin |
| [0009](0009-gmns-input.md) | GMNS input: zones span nodes, movements restrict listed links |
| [0010](0010-time-periods.md) | Time periods: unfinished trips preload the next period (superseded) |
| [0011](0011-explicit-preloads.md) | Explicit preloads instead of carried-over trips |
| [0012](0012-skip-unreachable-demand.md) | Unreachable demand: an error by default, skipped on request |
| [0013](0013-grouped-result-and-checked-options.md) | A grouped result, checked options and `network.assign` |
