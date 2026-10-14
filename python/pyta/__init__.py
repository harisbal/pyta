"""PyTA: path-based static traffic assignment.

The Rust extension takes numpy arrays. The helpers here accept pandas DataFrames
(or any mapping of column name to array-like) so flat tables can be passed straight in.

    import pyta

    net = pyta.Network.from_tables(links_df, turns_df)
    demand = pyta.Demand.from_table(net, trips_df)
    result = pyta.assign(net, demand, gap=1e-6)
    result.link_table()       # or the numpy arrays: result.link_flow, ...
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

import numpy as np

from ._pyta import (
    AssignResult,
    Demand,
    Network,
    TrafficClass,
    assign,
)

__all__ = [
    "AssignResult",
    "Demand",
    "Network",
    "TrafficClass",
    "assign",
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


def _pandas() -> Any:
    try:
        import pandas
    except ImportError as e:  # pragma: no cover - depends on the environment
        raise ImportError("result tables need pandas: pip install pandas") from e
    return pandas


def _flow_columns(result: AssignResult, class_flows: list, volume: np.ndarray,
                  preload: Optional[np.ndarray], time: np.ndarray) -> dict:
    """The flow, volume and time columns shared by the link and turn tables."""
    columns = {"flow": np.sum(class_flows, axis=0)}
    if len(class_flows) > 1:
        for name, flow in zip(result.class_names, class_flows):
            columns[f"flow_{name}"] = flow
    columns["volume"] = volume
    if preload is not None:
        columns["preload"] = preload
    columns["time"] = time
    return columns


def _link_table(result: AssignResult) -> Any:
    """Implements `AssignResult.link_table`; documented in the type stubs."""
    net = result.network
    from_node, to_node = net.link_nodes()
    columns = {} if net.link_ids is None else {"link_id": net.link_ids}
    columns.update(from_node=from_node, to_node=to_node)
    columns.update(_flow_columns(result, result.class_link_flow, result.link_volume,
                                 result.link_preload, result.link_time))
    total = result.link_volume if result.link_preload is None else result.link_volume + result.link_preload
    columns["voc"] = total / net.capacity
    columns["time"] = columns.pop("time")
    table = _pandas().DataFrame(columns)
    table.index.name = "link"
    return table


def _turn_table(result: AssignResult) -> Any:
    """Implements `AssignResult.turn_table`; documented in the type stubs."""
    net = result.network
    from_link, to_link = net.turn_links()
    columns = {"from_link": from_link, "to_link": to_link, "node": net.link_nodes()[1][from_link]}
    columns.update(_flow_columns(result, result.class_turn_flow, result.turn_volume,
                                 result.turn_preload, result.turn_time))
    table = _pandas().DataFrame(columns)
    table.index.name = "turn"
    return table


def _path_table(result: AssignResult, class_index: Optional[int] = None) -> Any:
    """Implements `AssignResult.path_table`; documented in the type stubs."""
    pd = _pandas()
    names = result.class_names
    indices = range(len(names)) if class_index is None else [class_index]
    tables = []
    for k in indices:
        p = result.paths(k)
        links = [tuple(int(l) for l in p["links"][a:b])
                 for a, b in zip(p["link_offsets"][:-1], p["link_offsets"][1:])]
        table = pd.DataFrame({"origin": p["origin"], "destination": p["destination"],
                              "flow": p["flow"], "time": p["time"], "links": links})
        if len(names) > 1:
            table.insert(0, "class", names[k])
        tables.append(table)
    table = pd.concat(tables, ignore_index=True)
    table.index.name = "path"
    return table


# The table helpers live here because they handle DataFrames; attach them to the
# extension classes so they sit next to `from_tntp` and `from_gmns`.
Network.from_tables = staticmethod(_network_from_tables)
Demand.from_table = staticmethod(_demand_from_table)
AssignResult.link_table = _link_table
AssignResult.turn_table = _turn_table
AssignResult.path_table = _path_table
