"""Type stubs and docstrings for the Rust extension module."""

from typing import Any, Dict, List, Optional, Sequence, Tuple, Union

import numpy as np
import pandas as pd
from numpy.typing import NDArray

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
        is_endpoint: Bool per node. A path may start or end at an endpoint node but
            never passes through it. Use for zone centroids.
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
        is_endpoint: Optional[NDArray[np.bool_]] = None,
        turns: Optional[Dict[str, NDArray[Any]]] = None,
        allow_u_turns: bool = False,
    ) -> None: ...
    @staticmethod
    def from_tables(
        links: Any,
        turns: Optional[Any] = None,
        *,
        num_nodes: Optional[int] = None,
        is_endpoint: Optional[Any] = None,
        allow_u_turns: bool = False,
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

        ``is_endpoint`` marks nodes (e.g. zone centroids) that paths may start or end at
        but not pass through."""
    @staticmethod
    def from_tntp(path: str) -> "Network":
        """Reads a TNTP ``*_net.tntp`` file. Node numbers become 0-based, links keep
        file order and ``<FIRST THRU NODE>`` becomes the endpoint mask."""
    @staticmethod
    def from_gmns(
        path: str,
        length_unit: Optional[str] = None,
        speed_unit: Optional[str] = None,
        zone_nodes_are_endpoints: bool = False,
    ) -> "Network":
        """Reads a GMNS dataset directory (``node.csv``, ``link.csv``, optional
        ``movement.csv`` and ``config.csv``). Times are in minutes; capacity is per
        link (per-lane capacity times lanes). Undirected links become two links.

        ``length_unit`` (e.g. ``"mile"``, ``"foot"``, ``"km"``) and ``speed_unit``
        (``"mph"``, ``"kph"``) override ``config.csv`` and are required without it. A
        movement table restricts an inbound link only if it lists a movement for it.
        With ``zone_nodes_are_endpoints`` zone nodes cannot be passed through."""
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
    def is_endpoint(self) -> NDArray[np.bool_]:
        """Per node: True if paths may only start or end there."""
    def link_nodes(self) -> Tuple[NDArray[np.uint32], NDArray[np.uint32]]:
        """Returns the from-node and to-node of every link."""
    def turn_links(self) -> Tuple[NDArray[np.uint32], NDArray[np.uint32]]:
        """Returns the incoming and outgoing link of every turn, indexed by turn id."""

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
        ``skip_unknown_zones``, in which case they are summed in ``skipped_flow``."""
    @property
    def skipped_flow(self) -> float:
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

class AssignResult:
    """The outcome of :func:`assign`.

    The attributes are numpy arrays indexed by link or turn id. :meth:`link_table`,
    :meth:`turn_table` and :meth:`path_table` give the same results as pandas DataFrames.
    """

    def link_table(self) -> pd.DataFrame:
        """One row per link: ``from_node``, ``to_node``, ``flow`` (vehicles of all
        classes), ``flow_<class>`` when there are several classes, ``volume`` (in car
        units), ``preload`` if one was given, ``voc`` (volume plus preload over capacity)
        and ``time``. GMNS networks also get their ``link_id``."""
    def turn_table(self) -> pd.DataFrame:
        """One row per turn: ``from_link``, ``to_link``, ``node``, then the same flow,
        volume and time columns as :meth:`link_table`."""
    def path_table(self, class_index: Optional[int] = None) -> pd.DataFrame:
        """One row per path: ``class`` (when there are several), ``origin``,
        ``destination``, ``flow``, ``time`` and ``links`` (a tuple of link ids).
        ``class_index`` selects one class; by default all are included.

        ``.explode("links")`` gives one row per path and link. For very large models,
        :meth:`paths` is faster."""
    @property
    def network(self) -> Network:
        """The network that was assigned."""
    @property
    def class_names(self) -> List[str]:
        """One name per class: the :class:`TrafficClass` name, or its position."""
    @property
    def link_volume(self) -> NDArray[np.float64]:
        """PCE-weighted volume per link."""
    @property
    def turn_volume(self) -> NDArray[np.float64]:
        """PCE-weighted volume per turn."""
    @property
    def link_time(self) -> NDArray[np.float64]:
        """Travel time per link at the final volumes, preload included."""
    @property
    def turn_time(self) -> NDArray[np.float64]:
        """Travel time per turn: fixed penalty plus delay."""
    @property
    def link_flow(self) -> NDArray[np.float64]:
        """Vehicles of the first class per link."""
    @property
    def turn_flow(self) -> NDArray[np.float64]:
        """Vehicles of the first class per turn."""
    @property
    def class_link_flow(self) -> List[NDArray[np.float64]]:
        """Vehicles per link, one array per class."""
    @property
    def class_turn_flow(self) -> List[NDArray[np.float64]]:
        """Vehicles per turn, one array per class."""
    @property
    def link_preload(self) -> Optional[NDArray[np.float64]]:
        """The ``link_preload`` passed to :func:`assign`, or None."""
    @property
    def turn_preload(self) -> Optional[NDArray[np.float64]]:
        """The ``turn_preload`` passed to :func:`assign`, or None."""
    @property
    def relative_gap(self) -> float:
        """Exact relative gap of the final flows."""
    @property
    def iterations(self) -> int:
        """Number of sweeps performed."""
    @property
    def converged(self) -> bool:
        """Whether the gap target was reached."""
    @property
    def sweep_gaps(self) -> NDArray[np.float64]:
        """Cheap gap estimate measured during each sweep."""
    @property
    def times(self) -> Dict[str, float]:
        """Seconds per phase: ``initial_assignment``, ``shortest_paths``, ``flow_updates``,
        ``sweep_rebuild`` and ``gap_checks``."""
    @property
    def objective(self) -> Optional[float]:
        """Beckmann objective; ``None`` unless there is one class with PCE 1."""
    def paths(self, class_index: int = 0) -> Dict[str, NDArray[Any]]:
        """Returns every path of a class as flat arrays.

        The dict has ``origin``, ``destination``, ``flow`` and ``time`` (one entry per
        path), ``link_offsets`` (one more entry) and ``links`` (all link ids back to
        back). Path ``i`` uses ``links[link_offsets[i]:link_offsets[i + 1]]``.

        Raises:
            IndexError: If the class does not exist.
        """

def assign(
    network: Network,
    classes: Union[Demand, TrafficClass, Sequence[Union[Demand, TrafficClass]]],
    gap: float = 1e-6,
    max_iter: int = 200,
    order: str = "fixed",
    seed: int = 0,
    chunk_size: int = 0,
    threads: Optional[int] = None,
    link_preload: Optional[NDArray[np.float64]] = None,
    turn_preload: Optional[NDArray[np.float64]] = None,
) -> AssignResult:
    """Finds the user equilibrium by path-based gradient projection.

    Args:
        network: The network.
        classes: A :class:`Demand`, a :class:`TrafficClass`, or a list of them.
        gap: Stop when the relative gap is at or below this.
        max_iter: Maximum number of sweeps over all origins.
        order: Origin order per sweep: ``fixed``, ``shuffle``, ``alternate``,
            ``largest_gap``, ``largest_demand`` or ``smallest_demand``.
        seed: Seed for ``shuffle``.
        chunk_size: Origins whose shortest paths are searched in parallel before their
            flow shifts are applied one at a time. ``0`` picks about 1/32 of the origins
            (between 1 and 64); ``1`` is the plain sequential sweep. Results do not
            depend on the number of threads.
        threads: Number of threads for the parallel parts (default: all cores, or
            ``RAYON_NUM_THREADS``).
        link_preload: Fixed volume per link in car units, such as buses or through
            traffic, that is not assigned but adds to the volume the delays see.
        turn_preload: The same per turn.

    Returns:
        The flows, paths and convergence information.

    Raises:
        ValueError: For malformed input or demand that cannot be routed.
    """
