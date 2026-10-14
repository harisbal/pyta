# Changelog

PyTraf is still in development, so names can change between versions.

## 0.3.1

- The source distribution includes the license file, so it can be uploaded to PyPI.

## 0.3.0

- The package is now **PyTraf**: `import pytraf`, `pip install pytraf`. Earlier versions were called PyTA.
- First release on PyPI.

## 0.2.0

- The result is grouped (`result.links`, `result.turns`, `result.paths`, `result.unrouted`,
  `result.diagnostics`) and has a `summary()`.
- `network.assign(classes, ...)`; options after `classes` are keyword-only, and `order` and `unreachable`
  are checked string enums.
- `is_endpoint` is `is_centroid` and `Demand.skipped_flow` is `Demand.unknown_zone_flow`. The old names still
  work with a `DeprecationWarning`.

## 0.1.1

- `unreachable="skip"` leaves out demand that no allowed path serves and reports it.
- Prebuilt wheels for Linux, macOS and Windows.

## 0.1.0

First public release.
