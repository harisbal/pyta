"""PyTA: static traffic assignment with a Rust core.

The Rust extension takes numpy arrays. The helpers here accept pandas DataFrames
(or any mapping of column name to array-like) so flat tables can be passed straight in.

    import pyta

    net = pyta.Network.from_tables(links_df, turns_df)
    demand = pyta.Demand.from_table(net, trips_df)
    result = pyta.assign(net, demand, gap=1e-6)
    result.link_flow
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

import numpy as np

from ._pyta import (
    AssignResult,
    Demand,
    Network,
    Period,
    TrafficClass,
    assign,
    assign_periods,
)

__all__ = [
    "AssignResult",
    "Demand",
    "Network",
    "Period",
    "TrafficClass",
    "assign",
    "assign_periods",
]


def _column(table: Any, name: str, dtype: Any, *, required: bool = True) -> Optional[np.ndarray]:
    """One column of a DataFrame or mapping as a contiguous numpy array of `dtype`."""
    if name not in table:
        if required:
            raise ValueError(f"missing column '{name}'")
        return None
    return np.ascontiguousarray(np.asarray(table[name]), dtype=dtype)


def _network_from_tables(
    links: Any,
    turns: Optional[Any] = None,
    *,
    num_nodes: Optional[int] = None,
    is_endpoint: Optional[Any] = None,
    allow_u_turns: bool = False,
) -> Network:
    """Implements `Network.from_tables`; documented in the type stubs."""
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


def _demand_from_table(network: Network, table: Any) -> Demand:
    """Implements `Demand.from_table`; documented in the type stubs."""
    return Demand(
        network.num_nodes,
        _column(table, "origin", np.int64),
        _column(table, "destination", np.int64),
        _column(table, "flow", np.float64),
    )


# The table helpers live here because they handle DataFrames; attach them to the
# extension classes so they sit next to `from_tntp` and `from_gmns`.
Network.from_tables = staticmethod(_network_from_tables)
Demand.from_table = staticmethod(_demand_from_table)
