# Rust API reference

The Rust API is documented with rustdoc. Build and open it with:

```sh
cargo doc --workspace --no-deps --open
```

Start from these items in `pytraf_core`:

| item | what it is |
|---|---|
| `Network`, `Edge` | the immutable graph and its link description |
| `TurnTable`, `TurnException`, `TurnRule`, `TurnOptions` | turns, bans and penalties |
| `Vdf`, `Bpr`, `NoDelay` | delay functions |
| `OdDemand` | demand grouped by origin |
| `gradient_projection`, `gradient_projection_with_turn_delays` | single-class solver |
| `gradient_projection_multiclass`, `ClassSpec` | several classes |
| `frank_wolfe`, `frank_wolfe_with_turn_delays` | reference solver |
| `dijkstra`, `dijkstra_turns` | shortest paths |
| `OriginOrder`, `GradientProjectionOptions` | solver settings |

and `pytraf_io::tntp` for reading TNTP files. See the [Rust guide](../guide/rust.md) for a worked example.
