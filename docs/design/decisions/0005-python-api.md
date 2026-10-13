# 0005. Python API conventions

- **Arrays, not objects.** The extension takes numpy arrays; a thin Python layer turns DataFrames and
  dicts into arrays. File and table handling stays out of the core.
- **Ids are positions.** Link `i` is row `i`; nodes are integers from 0. Mapping external ids to these is the caller's job,
  which keeps the core simple. The TNTP reader makes node numbers 0-based.
- **Turn tables by link ids.** Users list turns as `(from_link, to_link)` rows; the engine assigns turn
  ids. `Network.turn_links()` shows the mapping.
- **Results are arrays.** Flows are numpy arrays; paths come as flat arrays (`link_offsets`, `links`),
  which export straight to Arrow or pandas.
- **Frozen classes and the GIL.** Network, demand and class objects are immutable, and `assign` releases the GIL, so
  assignments can run in threads.
- **Errors** are `ValueError` with the reason, `IndexError` for a missing class.
- **Wheels** are built with maturin against the stable ABI (Python 3.9 and later).

**Not done yet:** Arrow PyCapsule input (zero-copy from polars, pyarrow), which numpy arrays cover for now.
