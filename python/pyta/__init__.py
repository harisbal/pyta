"""PyTA: static traffic assignment with a Rust core.

The Rust extension takes numpy arrays. The helpers here accept pandas DataFrames
(or any mapping of column name to array-like) so flat tables can be passed straight in.

    import pyta

    net = pyta.network_from_tables(links_df, turns_df)
    demand = pyta.demand_from_table(trips_df, num_nodes=net.num_nodes)
    result = pyta.assign(net, demand, gap=1e-6)
    result.link_flow
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

import numpy as np

from ._pyta import AssignResult, Demand, Network, TrafficClass, assign

__all__ = [
    "AssignResult",
    "Demand",
    "Network",
    "TrafficClass",
    "assign",
    "demand_from_table",
    "network_from_tables",
]


def _column(table: Any, name: str, dtype: Any, *, required: bool = True) -> Optional[np.ndarray]:
    """One column of a DataFrame or mapping as a contiguous numpy array of `dtype`."""
    if name not in table:
        if required:
            raise ValueError(f"missing column '{name}'")
        return None
    return np.ascontiguousarray(np.asarray(table[name]), dtype=dtype)


def network_from_tables(
    links: Any,
    turns: Optional[Any] = None,
    *,
    num_nodes: Optional[int] = None,
    is_endpoint: Optional[Any] = None,
    allow_u_turns: bool = False,
) -> Network:
    """Builds a `Network` from flat tables.

    `links` needs the columns ``from_node``, ``to_node``, ``free_flow_time`` and
    ``capacity``; ``b`` (default 0.15) and ``power`` (default 4) are optional.
    Node ids are integers starting at 0 (`num_nodes` defaults to the largest id + 1).
    Links get their ids from their row order, 0, 1, 2, ...

    `turns` is optional and lists exceptions and delays by link ids. Columns:
    ``from_link`` and ``to_link`` (required), ``banned`` (bool), ``penalty`` (fixed
    delay, NaN = none) and ``delay_free_flow``, ``delay_capacity``, ``delay_b``,
    ``delay_power`` (a BPR delay on the turn's own flow; give all four or none).
    Turns not listed are allowed at zero cost, except U-turns unless ``allow_u_turns``.
    Use ``Network.turn_links()`` to see which turns exist.

    `is_endpoint` marks nodes (e.g. zone centroids) that paths may start or end at but
    not pass through.
    """
    from_node = _column(links, "from_node", np.int64)
    to_node = _column(links, "to_node", np.int64)
    if num_nodes is None:
        num_nodes = int(max(from_node.max(initial=-1), to_node.max(initial=-1))) + 1
    turn_arrays = None
    if turns is not None:
        turn_arrays = {"from_link": _column(turns, "from_link", np.int64),
                       "to_link": _column(turns, "to_link", np.int64)}
        for name, dtype in [("banned", np.bool_), ("penalty", np.float64),
                            ("delay_free_flow", np.float64), ("delay_capacity", np.float64),
                            ("delay_b", np.float64), ("delay_power", np.float64)]:
            column = _column(turns, name, dtype, required=False)
            if column is not None:
                turn_arrays[name] = column
    endpoint = None if is_endpoint is None else np.ascontiguousarray(np.asarray(is_endpoint), dtype=np.bool_)
    return Network(
        num_nodes,
        from_node,
        to_node,
        _column(links, "free_flow_time", np.float64),
        _column(links, "capacity", np.float64),
        _column(links, "b", np.float64, required=False),
        _column(links, "power", np.float64, required=False),
        endpoint,
        turn_arrays,
        allow_u_turns,
    )


def demand_from_table(table: Any, num_nodes: int) -> Demand:
    """Builds a `Demand` from a long-format table with columns ``origin``,
    ``destination`` and ``flow`` (node ids start at 0)."""
    return Demand(
        num_nodes,
        _column(table, "origin", np.int64),
        _column(table, "destination", np.int64),
        _column(table, "flow", np.float64),
    )
