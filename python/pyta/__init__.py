"""PyTA: path-based static traffic assignment.

The Rust extension takes numpy arrays. The helpers here accept pandas DataFrames
(or any mapping of column name to array-like) so flat tables can be passed straight in.

    import pyta

    net = pyta.Network.from_tables(links_df, turns_df)
    demand = pyta.Demand.from_table(net, trips_df)
    result = net.assign(demand, gap=1e-6)
    result.links.table()      # or the numpy arrays: result.links.flow, ...
"""

from __future__ import annotations

import warnings
from typing import Any, Optional, Sequence, Union

import numpy as np
from numpy.typing import NDArray

from . import _pyta
from ._pyta import (
    __version__,
    Demand,
    Network,
    TrafficClass,
)
from .options import Order, Unreachable
from .results import AssignResult

__all__ = [
    "__version__",
    "AssignResult",
    "Demand",
    "Network",
    "Order",
    "TrafficClass",
    "Unreachable",
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
    is_centroid: Optional[Any] = None,
    allow_u_turns: bool = False,
    is_endpoint: Optional[Any] = None,
) -> Network:
    """Implements `Network.from_tables`; documented in the type stubs."""
    if is_endpoint is not None:
        warnings.warn("is_endpoint is deprecated, use is_centroid", DeprecationWarning, stacklevel=2)
        if is_centroid is None:
            is_centroid = is_endpoint
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
    centroid = None if is_centroid is None else np.ascontiguousarray(np.asarray(is_centroid), dtype=np.bool_)
    return Network(
        num_nodes,
        from_node,
        to_node,
        _column(links, "free_flow_time", np.float64),
        _column(links, "capacity", np.float64),
        _column(links, "b", np.float64, required=False),
        _column(links, "power", np.float64, required=False),
        centroid,
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


def assign(
    network: Network,
    classes: Union[Demand, TrafficClass, Sequence[Union[Demand, TrafficClass]]],
    *,
    gap: float = 1e-6,
    max_iter: int = 200,
    order: Union[Order, str] = Order.FIXED,
    seed: int = 0,
    chunk_size: int = 0,
    threads: Optional[int] = None,
    link_preload: Optional[NDArray[np.float64]] = None,
    turn_preload: Optional[NDArray[np.float64]] = None,
    unreachable: Union[Unreachable, str] = Unreachable.ERROR,
) -> AssignResult:
    """Finds the user equilibrium by path-based gradient projection.

    This is also a method of the network: ``network.assign(classes, ...)`` is
    ``pyta.assign(network, classes, ...)``. Everything after ``classes`` is keyword-only.

    Args:
        network: The network.
        classes: A `Demand`, a `TrafficClass`, or a list of them.
        gap: Stop when the relative gap is at or below this.
        max_iter: Maximum number of sweeps over all origins.
        order: Origin order per sweep, an `Order` or its string value.
        seed: Seed for `Order.SHUFFLE`.
        chunk_size: Origins whose shortest paths are searched in parallel before their
            flow shifts are applied one at a time. `0` picks about 1/32 of the origins
            (between 1 and 64); `1` is the plain sequential sweep. Results do not
            depend on the number of threads.
        threads: Number of threads for the parallel parts (default: all cores, or
            `RAYON_NUM_THREADS`).
        link_preload: Fixed volume per link in car units, such as buses or through
            traffic, that is not assigned but adds to the volume the delays see.
        turn_preload: The same per turn.
        unreachable: What to do with demand between nodes that no allowed path joins, an
            `Unreachable` or its string value. `Unreachable.ERROR` raises and names the
            pairs; `Unreachable.SKIP` leaves those trips out, assigns the rest and
            reports them in `AssignResult.unrouted`.

    Returns:
        The flows, paths and convergence information.

    Raises:
        ValueError: For malformed input, an unknown `order` or `unreachable`, or demand
            that cannot be routed while `unreachable` is `Unreachable.ERROR`.
    """
    raw = _pyta.assign(
        network,
        classes,
        gap=gap,
        max_iter=max_iter,
        order=str(Order(order)),
        seed=seed,
        chunk_size=chunk_size,
        threads=threads,
        link_preload=link_preload,
        turn_preload=turn_preload,
        unreachable=str(Unreachable(unreachable)),
    )
    return AssignResult(raw)


def _deprecated_property(cls: type, old: str, new: str, getter: Any) -> None:
    def fget(self: Any) -> Any:
        warnings.warn(f"{cls.__name__}.{old} is deprecated, use {new}", DeprecationWarning, stacklevel=2)
        return getter(self)

    setattr(cls, old, property(fget, doc=f"Deprecated: use ``{new}``."))


# The helpers live here because they handle DataFrames or wrap the extension; attach them
# to the extension classes so they sit next to `from_tntp` and `from_gmns`.
Network.from_tables = staticmethod(_network_from_tables)
Network.assign = assign
Demand.from_table = staticmethod(_demand_from_table)
_deprecated_property(Network, "is_endpoint", "is_centroid", lambda net: net.is_centroid)
_deprecated_property(Demand, "skipped_flow", "unknown_zone_flow", lambda d: d.unknown_zone_flow)
