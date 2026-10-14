"""Type stubs and docstrings for the Rust extension module."""

from typing import Any, Dict, List, Optional, Sequence, Tuple, Union

import numpy as np
from numpy.typing import NDArray

from .options import Order, Unreachable
from .results import AssignResult

__version__: str

class Network:
    """A road network with its link delay functions and turn table.

    Links are numbered by their position in the input arrays and nodes are integers
    from 0. Link delay is the BPR function
    ``free_flow_time * (1 + b * (volume / capacity) ** power)``.

    Args:
        num_nodes: Number of nodes; node ids are ``0..num_nodes``.
        from_node: From-node of each link.
        to_node: To-node of each link.
        free_flow_time: Delay at zero flow, per link.
        capacity: Capacity per link; must be positive.
        b: BPR ``b`` per link (default 0.15).
        power: BPR power per link (default 4).
        is_centroid: Bool per node. A path may start or end at a centroid but never
            passes through it. Use for zone centroids.
        turns: Optional turn table as a dict of arrays, with the columns ``from_link``
            and ``to_link`` and optionally ``banned``, ``penalty`` (NaN = none),
            ``delay_free_flow``, ``delay_capacity``, ``delay_b`` and ``delay_power``.
            Use :meth:`Network.from_tables` to pass DataFrames instead.
        allow_u_turns: Allow immediate returns to the node a link came from.
    """

    def __init__(
        self,
        num_nodes: int,
        from_node: NDArray[np.int64],
        to_node: NDArray[np.int64],
        free_flow_time: NDArray[np.float64],
        capacity: NDArray[np.float64],
        b: Optional[NDArray[np.float64]] = None,
        power: Optional[NDArray[np.float64]] = None,
        is_centroid: Optional[NDArray[np.bool_]] = None,
        turns: Optional[Dict[str, NDArray[Any]]] = None,
        allow_u_turns: bool = False,
        is_endpoint: Optional[NDArray[np.bool_]] = None,
    ) -> None: ...
    @staticmethod
    def from_tables(
        links: Any,
        turns: Optional[Any] = None,
        *,
        num_nodes: Optional[int] = None,
        is_centroid: Optional[Any] = None,
        allow_u_turns: bool = False,
        is_endpoint: Optional[Any] = None,
    ) -> "Network":
        """Builds a network from flat tables (pandas DataFrames or dicts of arrays).

        ``links`` needs the columns ``from_node``, ``to_node``, ``free_flow_time`` and
        ``capacity``; ``b`` (default 0.15) and ``power`` (default 4) are optional.
        Node ids are integers starting at 0 (``num_nodes`` defaults to the largest id + 1).
        Links get their ids from their row order, 0, 1, 2, ...

        ``turns`` is optional and lists exceptions and delays by link ids. Columns:
        ``from_link`` and ``to_link`` (required), ``banned`` (bool), ``penalty`` (fixed
        delay, NaN = none) and ``delay_free_flow``, ``delay_capacity``, ``delay_b``,
        ``delay_power`` (a BPR delay on the turn's own flow; give all four or none).
        Turns not listed are allowed at zero cost, except U-turns unless ``allow_u_turns``.
        Use :meth:`turn_links` to see which turns exist.

        ``is_centroid`` marks nodes (zone centroids) that paths may start or end at but
        not pass through. ``is_endpoint`` is the deprecated old name."""
    @staticmethod
    def from_tntp(path: str) -> "Network":
        """Reads a TNTP ``*_net.tntp`` file. Node numbers become 0-based, links keep
        file order and ``<FIRST THRU NODE>`` becomes the centroid mask."""
    @staticmethod
    def from_gmns(
        path: str,
        length_unit: Optional[str] = None,
        speed_unit: Optional[str] = None,
        zone_nodes_are_centroids: bool = False,
        zone_nodes_are_endpoints: Optional[bool] = None,
    ) -> "Network":
        """Reads a GMNS dataset directory (``node.csv``, ``link.csv``, optional
        ``movement.csv`` and ``config.csv``). Times are in minutes; capacity is per
        link (per-lane capacity times lanes). Undirected links become two links.

        ``length_unit`` (e.g. ``"mile"``, ``"foot"``, ``"km"``) and ``speed_unit``
        (``"mph"``, ``"kph"``) override ``config.csv`` and are required without it. A
        movement table restricts an inbound link only if it lists a movement for it.
        With ``zone_nodes_are_centroids`` zone nodes cannot be passed through.
        ``zone_nodes_are_endpoints`` is the deprecated old name."""
    @property
    def node_ids(self) -> Optional[List[str]]:
        """``node_id`` of every node as written in the GMNS files (None otherwise)."""
    @property
    def link_ids(self) -> Optional[List[str]]:
        """``link_id`` of every link; an undirected link appears twice (None otherwise)."""
    @property
    def num_nodes(self) -> int:
        """Number of nodes."""
    @property
    def num_links(self) -> int:
        """Number of links."""
    @property
    def num_turns(self) -> int:
        """Number of allowed turns in the turn table."""
    @property
    def capacity(self) -> NDArray[np.float64]:
        """Capacity per link."""
    @property
    def is_centroid(self) -> NDArray[np.bool_]:
        """Per node: True if paths may only start or end there."""
    def link_nodes(self) -> Tuple[NDArray[np.uint32], NDArray[np.uint32]]:
        """Returns the from-node and to-node of every link."""
    def turn_links(self) -> Tuple[NDArray[np.uint32], NDArray[np.uint32]]:
        """Returns the incoming and outgoing link of every turn, indexed by turn id."""
    def assign(
        self,
        classes: Union["Demand", "TrafficClass", Sequence[Union["Demand", "TrafficClass"]]],
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
        """Finds the user equilibrium on this network by path-based gradient projection.

        The same as ``pytraf.assign(self, classes, ...)``, where all the options are
        described. Everything after ``classes`` is keyword-only."""

class Demand:
    """Trips between nodes in long format (a zone is a node).

    Repeated origin-destination pairs add up. Trips with the same origin and
    destination use no network and are skipped.

    Args:
        num_nodes: Number of nodes of the network the demand is for.
        origins: Origin node of each entry.
        destinations: Destination node of each entry.
        flows: Trips per entry; non-negative.
    """

    def __init__(
        self,
        num_nodes: int,
        origins: NDArray[np.int64],
        destinations: NDArray[np.int64],
        flows: NDArray[np.float64],
    ) -> None: ...
    @staticmethod
    def from_table(network: Network, table: Any) -> "Demand":
        """Builds the demand for ``network`` from a long-format table (a pandas
        DataFrame or a dict of arrays) with columns ``origin``, ``destination`` and
        ``flow``. Origins and destinations are node ids."""
    @staticmethod
    def from_tntp(network: Network, path: str) -> "Demand":
        """Reads a TNTP ``*_trips.tntp`` file for ``network``."""
    @staticmethod
    def from_gmns(
        network: Network,
        path: str,
        origin: str = "o_zone_id",
        destination: str = "d_zone_id",
        flow: str = "volume",
        skip_unknown_zones: bool = False,
    ) -> "Demand":
        """Reads a zone-to-zone demand CSV for a network read with ``Network.from_gmns``.
        ``origin``, ``destination`` and ``flow`` name the columns. A zone's trips are
        split equally over its nodes. Rows of zones without a node are an error unless
        ``skip_unknown_zones``, in which case they are summed in ``unknown_zone_flow``."""
    @property
    def unknown_zone_flow(self) -> float:
        """Trips dropped because their zone has no node (0 unless read with ``from_gmns``)."""
    @property
    def num_nodes(self) -> int:
        """Number of nodes the demand was built for."""
    def total(self) -> float:
        """Total number of trips."""

class TrafficClass:
    """One traffic class: demand plus how its vehicles load and move on the network.

    Args:
        demand: The class's trips.
        pce: Passenger-car units one vehicle loads on the network (cars 1.0,
            trucks 2.5). Link delays depend on the PCE-weighted volume.
        extra_link_cost: Extra cost per link in the same units as travel time, for
            example a toll divided by the class's value of time.
        allowed_links: Bool per link. A turn is usable only if both its links are.
        name: Label for the class in result tables (default: its position).
    """

    def __init__(
        self,
        demand: Demand,
        pce: float = 1.0,
        extra_link_cost: Optional[NDArray[np.float64]] = None,
        allowed_links: Optional[NDArray[np.bool_]] = None,
        name: Optional[str] = None,
    ) -> None: ...

class _RawResult:
    """The solver's result as numpy arrays; wrapped by :class:`pytraf.AssignResult`."""

    network: Network
    class_names: List[str]
    link_volume: NDArray[np.float64]
    turn_volume: NDArray[np.float64]
    link_time: NDArray[np.float64]
    turn_time: NDArray[np.float64]
    link_preload: Optional[NDArray[np.float64]]
    turn_preload: Optional[NDArray[np.float64]]
    class_link_flow: List[NDArray[np.float64]]
    class_turn_flow: List[NDArray[np.float64]]
    relative_gap: float
    iterations: int
    converged: bool
    sweep_gaps: NDArray[np.float64]
    times: Dict[str, float]
    objective: Optional[float]
    unrouted_flow: List[float]
    def paths(self, class_index: int = 0) -> Dict[str, NDArray[Any]]: ...
    def unrouted(self) -> Dict[str, NDArray[Any]]: ...

def assign(
    network: Network,
    classes: Any,
    *,
    gap: float = ...,
    max_iter: int = ...,
    order: str = ...,
    seed: int = ...,
    chunk_size: int = ...,
    threads: Optional[int] = ...,
    link_preload: Optional[NDArray[np.float64]] = ...,
    turn_preload: Optional[NDArray[np.float64]] = ...,
    unreachable: str = ...,
) -> _RawResult: ...
