"""Named choices for the string options of :func:`pytraf.assign`.

Each is a string enum, so ``Order.SHUFFLE == "shuffle"`` and the plain string is accepted
wherever the enum is. A misspelt value raises a ``ValueError`` that lists the valid ones.
"""

from __future__ import annotations

import sys
from enum import Enum
from typing import Any

if sys.version_info >= (3, 11):
    from enum import StrEnum
else:  # pragma: no cover - depends on the interpreter

    class StrEnum(str, Enum):
        """`enum.StrEnum` for Python before 3.11."""

        def __str__(self) -> str:
            return str(self.value)


__all__ = ["Order", "Unreachable"]


class _Checked(StrEnum):
    """A string enum whose lookup error names the valid values."""

    @classmethod
    def _missing_(cls, value: Any) -> None:
        valid = ", ".join(repr(member.value) for member in cls)
        raise ValueError(f"unknown {cls.__name__.lower()} {value!r}: use one of {valid}")


class Order(_Checked):
    """The order in which a sweep visits the origins."""

    FIXED = "fixed"
    """The same order every sweep."""
    SHUFFLE = "shuffle"
    """A new random order each sweep, seeded by ``seed``."""
    ALTERNATE = "alternate"
    """Forwards, then backwards, on alternate sweeps."""
    LARGEST_GAP = "largest_gap"
    """Origins with the largest remaining gap first."""
    LARGEST_DEMAND = "largest_demand"
    """Origins with the most demand first."""
    SMALLEST_DEMAND = "smallest_demand"
    """Origins with the least demand first."""


class Unreachable(_Checked):
    """What :func:`pytraf.assign` does with demand that no allowed path can serve."""

    ERROR = "error"
    """Raise a ``ValueError`` that names the origin-destination pairs."""
    SKIP = "skip"
    """Leave those trips out, assign the rest and report them in ``result.unrouted``."""
