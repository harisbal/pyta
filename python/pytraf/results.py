"""The result of :func:`pytraf.assign`, grouped by what it describes.

``result.links`` and ``result.turns`` hold the flows and times per link and per turn,
``result.paths`` the paths, ``result.unrouted`` the demand that was left out and
``result.diagnostics`` how the solver got there. The convergence summary
(``converged``, ``iterations``, ``relative_gap``) is on the result itself.
"""

from __future__ import annotations

import warnings
from dataclasses import dataclass
from functools import cached_property
from typing import Any, Dict, Iterator, List, Optional, Sequence, Union

import numpy as np
from numpy.typing import NDArray

from ._pytraf import Network, _RawResult

__all__ = [
    "AssignResult",
    "ClassFlows",
    "Diagnostics",
    "LinkResults",
    "PathResults",
    "TurnResults",
    "UnroutedDemand",
]


def _pandas() -> Any:
    try:
        import pandas
    except ImportError as e:  # pragma: no cover - depends on the environment
        raise ImportError("result tables need pandas: pip install pandas") from e
    return pandas


def _fixed(array: NDArray[Any]) -> NDArray[Any]:
    """Marks a cached array read-only, so editing one access cannot change the next."""
    array.flags.writeable = False
    return array


class ClassFlows(Sequence):
    """Vehicles per class, as one array per class.

    Index by position (``flows[0]``) or by class name (``flows["truck"]``); iterate for
    the arrays in class order.
    """

    def __init__(self, names: Sequence[str], arrays: Sequence[NDArray[np.float64]]) -> None:
        self._names = list(names)
        self._arrays = list(arrays)

    @property
    def names(self) -> List[str]:
        """The class names, in class order."""
        return list(self._names)

    def __len__(self) -> int:
        return len(self._arrays)

    def __getitem__(self, key: Union[int, str, slice]) -> Any:
        if isinstance(key, str):
            try:
                return self._arrays[self._names.index(key)]
            except ValueError:
                raise KeyError(f"no class named {key!r}; the classes are {self._names}") from None
        return self._arrays[key]

    def __iter__(self) -> Iterator[NDArray[np.float64]]:
        return iter(self._arrays)

    def __repr__(self) -> str:
        return f"ClassFlows(classes={self._names})"


class _Elements:
    """What `LinkResults` and `TurnResults` share: one value per link or per turn."""

    _prefix = ""  # "link" or "turn": the names of the raw result's attributes

    def __init__(self, raw: _RawResult, class_names: Sequence[str]) -> None:
        self._raw = raw
        self._class_names = list(class_names)

    @cached_property
    def volume(self) -> NDArray[np.float64]:
        """Flow in car units (vehicles times their PCE), summed over the classes. This is
        what the delay functions see, without the preload."""
        return _fixed(getattr(self._raw, f"{self._prefix}_volume"))

    @cached_property
    def time(self) -> NDArray[np.float64]:
        """Travel time at the final volumes, preload included."""
        return _fixed(getattr(self._raw, f"{self._prefix}_time"))

    @cached_property
    def preload(self) -> Optional[NDArray[np.float64]]:
        """The fixed volume passed to ``assign``, or ``None`` if there was none."""
        preload = getattr(self._raw, f"{self._prefix}_preload")
        return None if preload is None else _fixed(preload)

    @cached_property
    def by_class(self) -> ClassFlows:
        """Vehicles per class. Index by position or by class name."""
        arrays = getattr(self._raw, f"class_{self._prefix}_flow")
        return ClassFlows(self._class_names, [_fixed(a) for a in arrays])

    @cached_property
    def flow(self) -> NDArray[np.float64]:
        """Vehicles of all classes (not weighted by PCE)."""
        return _fixed(np.sum(list(self.by_class), axis=0))

    def _flow_columns(self) -> Dict[str, NDArray[np.float64]]:
        """The flow, volume, preload and time columns of the table."""
        columns: Dict[str, NDArray[np.float64]] = {"flow": self.flow}
        if len(self.by_class) > 1:
            for name, flow in zip(self._class_names, self.by_class):
                columns[f"flow_{name}"] = flow
        columns["volume"] = self.volume
        if self.preload is not None:
            columns["preload"] = self.preload
        columns["time"] = self.time
        return columns


class LinkResults(_Elements):
    """Flows and times per link, indexed by link id."""

    _prefix = "link"

    def __init__(self, raw: _RawResult, class_names: Sequence[str]) -> None:
        super().__init__(raw, class_names)
        self._network: Network = raw.network

    @cached_property
    def voc(self) -> NDArray[np.float64]:
        """Volume over capacity: ``volume`` plus the preload, divided by the capacity."""
        total = self.volume if self.preload is None else self.volume + self.preload
        return _fixed(total / self._network.capacity)

    def table(self) -> Any:
        """One row per link as a pandas DataFrame.

        Columns: ``link_id`` for GMNS networks, ``from_node``, ``to_node``, ``flow``
        (vehicles of all classes), ``flow_<class>`` when there are several classes,
        ``volume`` (in car units), ``preload`` if one was given, ``voc`` (volume plus
        preload over capacity) and ``time``.
        """
        net = self._network
        from_node, to_node = net.link_nodes()
        columns: Dict[str, Any] = {} if net.link_ids is None else {"link_id": net.link_ids}
        columns.update(from_node=from_node, to_node=to_node)
        flows = self._flow_columns()
        time = flows.pop("time")
        columns.update(flows)
        columns["voc"] = self.voc
        columns["time"] = time
        table = _pandas().DataFrame(columns)
        table.index.name = "link"
        return table


class TurnResults(_Elements):
    """Flows and times per turn, indexed by turn id (see ``Network.turn_links``)."""

    _prefix = "turn"

    def __init__(self, raw: _RawResult, class_names: Sequence[str]) -> None:
        super().__init__(raw, class_names)
        self._network: Network = raw.network

    def table(self) -> Any:
        """One row per turn as a pandas DataFrame.

        Columns: ``from_link``, ``to_link``, ``node``, then the same flow, volume,
        preload and time columns as the link table (no ``voc``).
        """
        net = self._network
        from_link, to_link = net.turn_links()
        columns: Dict[str, Any] = {
            "from_link": from_link,
            "to_link": to_link,
            "node": net.link_nodes()[1][from_link],
        }
        columns.update(self._flow_columns())
        table = _pandas().DataFrame(columns)
        table.index.name = "turn"
        return table


class PathResults:
    """The paths of the final solution, with the flow and travel time of each."""

    def __init__(self, raw: _RawResult, class_names: Sequence[str]) -> None:
        self._raw = raw
        self._class_names = list(class_names)

    def arrays(self, class_index: int = 0) -> Dict[str, NDArray[Any]]:
        """Every path of a class as flat arrays.

        Returns ``origin``, ``destination``, ``flow`` and ``time`` (one entry per path),
        ``link_offsets`` (one more entry) and ``links`` (all link ids back to back; the
        links of path ``i`` are ``links[link_offsets[i]:link_offsets[i + 1]]``).

        Raises:
            IndexError: If the class does not exist.
        """
        return self._raw.paths(class_index)

    def table(self, class_index: Optional[int] = None) -> Any:
        """One row per path as a pandas DataFrame.

        Columns: ``class`` (when there are several), ``origin``, ``destination``,
        ``flow``, ``time`` and ``links`` (a tuple of link ids). ``class_index`` limits it
        to one class.
        """
        pd = _pandas()
        names = self._class_names
        indices = range(len(names)) if class_index is None else [class_index]
        tables = []
        for k in indices:
            p = self.arrays(k)
            links = [
                tuple(int(link) for link in p["links"][a:b])
                for a, b in zip(p["link_offsets"][:-1], p["link_offsets"][1:])
            ]
            table = pd.DataFrame(
                {
                    "origin": p["origin"],
                    "destination": p["destination"],
                    "flow": p["flow"],
                    "time": p["time"],
                    "links": links,
                }
            )
            if len(names) > 1:
                table.insert(0, "class", names[k])
            tables.append(table)
        table = pd.concat(tables, ignore_index=True)
        table.index.name = "path"
        return table

    def __call__(self, class_index: int = 0) -> Dict[str, NDArray[Any]]:
        warnings.warn(
            "result.paths(class_index) is deprecated, use result.paths.arrays(class_index)",
            DeprecationWarning,
            stacklevel=2,
        )
        return self.arrays(class_index)


class UnroutedDemand:
    """Demand that ``unreachable="skip"`` left out because no allowed path serves it.

    Empty (``len(result.unrouted) == 0``) when everything was routable or the
    ``unreachable`` option is ``"error"``.
    """

    def __init__(self, raw: _RawResult, class_names: Sequence[str]) -> None:
        self._raw = raw
        self._class_names = list(class_names)

    @cached_property
    def flow(self) -> List[float]:
        """Trips left out, one total per class."""
        return list(self._raw.unrouted_flow)

    @property
    def total(self) -> float:
        """Trips left out, over all classes."""
        return float(sum(self.flow))

    def arrays(self) -> Dict[str, NDArray[Any]]:
        """The left-out pairs as flat arrays: ``class`` (index), ``origin``,
        ``destination`` and ``flow``."""
        return self._raw.unrouted()

    def table(self) -> Any:
        """One row per left-out origin-destination pair as a pandas DataFrame.

        Columns: ``class`` (when there are several), ``origin``, ``destination``, ``flow``.
        """
        u = self.arrays()
        names = self._class_names
        columns: Dict[str, Any] = {
            "origin": u["origin"],
            "destination": u["destination"],
            "flow": u["flow"],
        }
        if len(names) > 1:
            columns = {"class": [names[k] for k in u["class"]], **columns}
        return _pandas().DataFrame(columns)

    def __len__(self) -> int:
        """The number of left-out origin-destination pairs."""
        return len(self.arrays()["flow"])

    def __call__(self) -> Dict[str, NDArray[Any]]:
        warnings.warn(
            "result.unrouted() is deprecated, use result.unrouted.arrays()",
            DeprecationWarning,
            stacklevel=2,
        )
        return self.arrays()

    def __repr__(self) -> str:
        return f"UnroutedDemand(pairs={len(self)}, flow={self.flow})"


@dataclass(frozen=True)
class Diagnostics:
    """How the solver got to the result."""

    sweep_gaps: NDArray[np.float64]
    """The relative gap after each sweep."""
    times: Dict[str, float]
    """Seconds per phase: ``initial_assignment``, ``shortest_paths``, ``flow_updates``,
    ``sweep_rebuild`` and ``gap_checks``."""
    objective: float
    """The Beckmann objective at the solution, or NaN when it is not defined: it needs
    a single class with PCE 1."""


class AssignResult:
    """The outcome of :func:`pytraf.assign`; build it with ``network.assign(...)``.

    Attributes:
        converged: Whether the relative gap reached the requested ``gap``.
        iterations: Sweeps over all origins that were run.
        relative_gap: The relative gap at the end.
        network: The network the result belongs to.
        class_names: One name per class: the ``TrafficClass`` name, or its position.
        links: Flows and times per link.
        turns: Flows and times per turn.
        paths: The paths and their flows.
        unrouted: Demand left out by ``unreachable="skip"``.
        diagnostics: Sweep gaps, phase times and the objective.
    """

    def __init__(self, raw: _RawResult) -> None:
        self._raw = raw
        self.converged: bool = raw.converged
        self.iterations: int = raw.iterations
        self.relative_gap: float = raw.relative_gap
        self.network: Network = raw.network
        self.class_names: List[str] = list(raw.class_names)
        self.links = LinkResults(raw, self.class_names)
        self.turns = TurnResults(raw, self.class_names)
        self.paths = PathResults(raw, self.class_names)
        self.unrouted = UnroutedDemand(raw, self.class_names)
        objective = raw.objective
        self.diagnostics = Diagnostics(
            sweep_gaps=_fixed(raw.sweep_gaps),
            times=dict(raw.times),
            objective=float("nan") if objective is None else objective,
        )

    def summary(self) -> str:
        """A short readable account of the run: convergence, trips per class, demand
        left out and the busiest link."""
        names = self.class_names
        state = "converged" if self.converged else "did not converge"
        lines = [f"{state} after {self.iterations} sweeps, relative gap {self.relative_gap:.3e}"]
        assigned = [float(np.sum(self.paths.arrays(k)["flow"])) for k in range(len(names))]
        noun = "class" if len(names) == 1 else "classes"
        lines.append(f"{sum(assigned):,.1f} trips assigned in {len(names)} {noun}")
        if len(names) > 1:
            width = max(len(n) for n in names)
            lines += [f"  {n:<{width}}  {trips:,.1f}" for n, trips in zip(names, assigned)]
        if self.unrouted.total > 0:
            lines.append(
                f"{self.unrouted.total:,.1f} trips left out: no allowed path "
                f"({len(self.unrouted)} origin-destination pairs)"
            )
        if len(self.links.voc) > 0:
            busiest = int(np.argmax(self.links.voc))
            lines.append(f"busiest link: {busiest} (volume/capacity {self.links.voc[busiest]:.2f})")
        return "\n".join(lines)

    def __repr__(self) -> str:
        return (
            f"AssignResult(converged={self.converged}, iterations={self.iterations}, "
            f"relative_gap={self.relative_gap:.3e}, classes={len(self.class_names)})"
        )


# ------------------------------------------------------------ deprecated names
#
# The names before 0.2.0, kept for one release. Each maps to its replacement.


def _deprecated(old: str, new: str, getter: Any) -> property:
    def fget(self: AssignResult) -> Any:
        warnings.warn(
            f"AssignResult.{old} is deprecated, use {new}", DeprecationWarning, stacklevel=2
        )
        return getter(self)

    return property(fget, doc=f"Deprecated: use ``{new}``.")


def _deprecated_method(old: str, new: str, call: Any) -> Any:
    def method(self: AssignResult, *args: Any) -> Any:
        warnings.warn(
            f"AssignResult.{old}() is deprecated, use {new}", DeprecationWarning, stacklevel=2
        )
        return call(self, *args)

    method.__name__ = old
    method.__doc__ = f"Deprecated: use ``{new}``."
    return method


_PROPERTIES = {
    "link_volume": ("links.volume", lambda r: r.links.volume),
    "turn_volume": ("turns.volume", lambda r: r.turns.volume),
    "link_time": ("links.time", lambda r: r.links.time),
    "turn_time": ("turns.time", lambda r: r.turns.time),
    "link_flow": ("links.by_class[0]", lambda r: r.links.by_class[0]),
    "turn_flow": ("turns.by_class[0]", lambda r: r.turns.by_class[0]),
    "class_link_flow": ("links.by_class", lambda r: list(r.links.by_class)),
    "class_turn_flow": ("turns.by_class", lambda r: list(r.turns.by_class)),
    "link_preload": ("links.preload", lambda r: r.links.preload),
    "turn_preload": ("turns.preload", lambda r: r.turns.preload),
    "unrouted_flow": ("unrouted.flow", lambda r: r.unrouted.flow),
    "sweep_gaps": ("diagnostics.sweep_gaps", lambda r: r.diagnostics.sweep_gaps),
    "times": ("diagnostics.times", lambda r: r.diagnostics.times),
    "objective": (
        "diagnostics.objective",
        lambda r: None if np.isnan(r.diagnostics.objective) else r.diagnostics.objective,
    ),
}
_METHODS = {
    "link_table": ("links.table()", lambda r: r.links.table()),
    "turn_table": ("turns.table()", lambda r: r.turns.table()),
    "path_table": ("paths.table()", lambda r, k=None: r.paths.table(k)),
    "unrouted_table": ("unrouted.table()", lambda r: r.unrouted.table()),
}

for _old, (_new, _getter) in _PROPERTIES.items():
    setattr(AssignResult, _old, _deprecated(_old, _new, _getter))
for _old, (_new, _call) in _METHODS.items():
    setattr(AssignResult, _old, _deprecated_method(_old, _new, _call))
del _old, _new, _getter, _call
