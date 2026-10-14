# Benchmarks

All benchmark networks come from the [Transportation Networks for Research](https://github.com/bstabler/TransportationNetworks)
repository and live in `data/tntp`. Timings are release builds, one run each, on an 8-thread development machine (15 GB RAM).

## Correctness

| network | zones / nodes / links | result |
|---|---|---|
| Sioux Falls | 24 / 24 / 76 | objective 4 231 335.29 equals the published value; worst link flow difference 0.014 vehicles at gap 1e-8 |
| Anaheim | 38 / 416 / 914 | objective equals the published 1 286 032.17; worst difference 0.00 vehicles at gap 1e-10. Zones 1–38 are endpoint-only centroids |
| Chicago Sketch | 387 / 933 / 2 950 | our objective 16 748 439 is *lower* than the published 16 748 596: the published file is not fully converged |

Other checks: turn costs against a turn-free model on an expanded graph, two identical classes
against the single-class equilibrium, and closed-form equilibria for tolls, bans, PCE and turn delays.

## Gradient projection against Frank-Wolfe

Sioux Falls, sweeps (gradient projection) or iterations (Frank-Wolfe) to reach a relative gap:

| gap | gradient projection | Frank-Wolfe |
|---|---|---|
| 1e-4 | 15 sweeps, 4 ms | 1 063 iterations, 0.46 s |
| 1e-5 | about 30 sweeps | 9 825 iterations, 2.1 s |
| 1e-6 | 54 sweeps, 20 ms | not run |
| 1e-8 | 98 sweeps, 32 ms | not run |

Frank-Wolfe needs about ten times more iterations for every extra digit. Gradient projection's
cost grows much more slowly, but it also has a tail: Anaheim needs 9 sweeps for 1e-6 and about 135 for 1e-8.

## Solve times

Gradient projection with the default settings (automatic chunk size, 8 threads), release build:

| network | gap 1e-4 | gap 1e-6 |
|---|---|---|
| Sioux Falls | 10 ms, 14 sweeps | 30 ms, 56 sweeps |
| Anaheim | 30 ms, 4 sweeps | 50 ms, 9 sweeps |
| Chicago Sketch | 1.0 s, 11 sweeps | 1.8 s, 22 sweeps |

## Large networks

| network | zones / nodes / links / OD pairs | gap | sweeps | time | peak memory |
|---|---|---|---|---|---|
| Chicago Regional | 1 790 / 12 982 / 39 018 / 2.3 M | 1e-3 | 15 | 82 s | 3.2 GB |
| Chicago Regional | | 1e-4 | 32 | 122 s | 3.0 GB |
| Philadelphia | 1 525 / 13 389 / 40 003 / 1.2 M | 1e-4 | 24 | 69 s | 1.9 GB |

Loading is fast: Chicago Regional's 66 MB trip table is read in under a second.

**Against the published solution.** For Chicago Regional our objective at gap 1e-4 is 25 846 004.59,
*lower* than the published 25 859 850.56 (0.05% lower; the worst link flow differs by 760 vehicles).
As on Chicago Sketch, the published file is not a fully converged equilibrium. Philadelphia has no
published flow file.

**Where the time goes** (Chicago Regional, gap 1e-4, 32 sweeps): shortest-path searches 58 s, flow updates
49 s, rebuilds between sweeps 10 s, exact gap checks 3 s, initial assignment 2 s. The flow updates are the
sequential part of Gauss-Seidel and are the remaining bottleneck.

**Chunk size** (Chicago Regional, gap 1e-3, 8 threads):

| chunk size | sweeps | time | shortest paths | flow updates |
|---|---|---|---|---|
| 1 (sequential sweep) | 13 | 162 s | 115 s | 30 s |
| 16 | 15 | 99 s | 38 s | 39 s |
| 64 | 15 | 82 s | 27 s | 34 s |

Larger chunks need about two more sweeps, because the candidate paths found early are staler, but run
about twice as fast. On small networks the same relative staleness hurts more (Sioux Falls needs 93
sweeps with chunk 16 instead of 56), which is why the default chunk size is about 1/32 of the number of
origins, between 1 and 64.

**Speed-up from the performance work** (Chicago Regional, per sweep, early sweeps where most OD pairs change):
about 32 s per sweep at the start, down to about 12 s with the sequential code improvements and a further
2–3× from parallel shortest paths. Reaching gap 1e-3 took an estimated eight minutes originally and now takes
under 90 seconds.

## Against AequilibraE

AequilibraE 1.7.0 (biconjugate Frank-Wolfe, its best method) was run on the same machine as PyTA, with
8 cores each and one after the other. Its own benchmark loaders were used (`tests/aeq/benchmarks/conftest.py`
in its repository), so the graph is built as it builds it. Times are solve times only. The machine is an
i7-7700HQ with 8 threads. Both report a relative gap.

| network | gap | AequilibraE | PyTA | speed-up |
|---|---|---|---|---|
| Sioux Falls | 1e-4 | 1.3 s (118 iterations) | 0.01 s (14 sweeps) | ~130x |
| Sioux Falls | 1e-6 | 12.5 s (976) | 0.03 s (56) | ~400x |
| Anaheim | 1e-4 | 0.3 s (14) | 0.03 s (4) | ~10x |
| Anaheim | 1e-6 | 1.4 s (81) | 0.05 s (9) | ~28x |
| Chicago Sketch | 1e-4 | 11.1 s (48) | 1.13 s (11) | ~10x |
| Chicago Sketch | 1e-6 | 116.5 s (518) | 1.83 s (22) | ~64x |
| Chicago Regional | 1e-3 | 96.7 s (56) | 75.1 s (15) | 1.3x |
| Chicago Regional | 1e-4 | 303 s (192) | 114 s (30) | 2.7x |
| Chicago Regional | 1e-5 | 945 s (697) | 252 s (78) | 3.8x |

How to read this:

- The AequilibraE runs reproduce the claims on its own validation page (under 200 iterations for 1e-4 and just
  over 700 for 1e-5 on Chicago Regional), so the setup is faithful.
- A PyTA sweep costs more than an AequilibraE iteration on Chicago Regional (3 to 5 s against 1.4 s), but PyTA
  needs 6 to 9 times fewer of them. The advantage therefore grows as the gap tightens and nearly disappears at 1e-3.
- On the small networks the AequilibraE time is mostly per-iteration Python overhead (about 13 ms per iteration
  on Sioux Falls), so those ratios say little about the algorithms.
- PyTA keeps paths and needs about 3 GB on Chicago Regional. AequilibraE's memory was not measured, but a
  link-based method should need far less.
- AequilibraE's documentation quotes 0.44 s per iteration on a Ryzen 9 under Windows. This machine gave
  1.36 s per iteration, so absolute times from its page are not comparable with these.

## Origin order

Sweeps to reach a gap of 1e-6 (Shuffle: minimum / median / maximum over five seeds):

| order | Sioux Falls | Anaheim | Chicago Sketch |
|---|---|---|---|
| fixed | 54 | 9 | 26 |
| alternate | 55 | 14 | 60 |
| largest gap first | 95 | 10 | 18 |
| largest demand first | 26 | 10 | 25 |
| smallest demand first | 91 | 8 | 23 |
| shuffle | 86 / 98 / 100 | 9 / 9 / 11 | 18 / 19 / 21 |

No order wins everywhere. On Chicago Sketch, `largest_gap` and `shuffle` needed about 30% fewer sweeps
than `fixed`; on Sioux Falls, shuffling was almost twice as slow, though there the counts depend on
exactly when the gap check passes. `alternate` was clearly worst on the two larger networks. The
default stays `fixed` until more networks are tested.

## Reproduce

```sh
python data/tntp/fetch.py ChicagoRegional Philadelphia                    # large networks
cargo run --release -p pyta-io --example scale_benchmark -- ChicagoRegional 1e-4 80
cargo run --release -p pyta-io --example order_benchmark -- SiouxFalls
cargo run --release -p pyta-io --example order_benchmark -- Anaheim 1e-4 1e-6
cargo run --release -p pyta-io --example order_benchmark -- ChicagoSketch
```

`order_benchmark` takes the folder name under `data/tntp` and a list of gap tolerances; it compares every
origin order and, if the network has a published flow file, checks the result against it.
`scale_benchmark` takes the folder name, a gap, a sweep limit and a chunk size, and reports the phase
timings, the number of paths and peak memory.

To repeat the AequilibraE side, install `aequilibrae` in a separate environment, copy the TNTP files out of
`data/tntp` (its loader writes an `.omx` cache next to the data) and run its `tests/aeq/benchmarks/conftest.py`
loaders with `TrafficAssignment` set to `bfw`, `max_iter` 1000 and the gap target as the stopping rule.
