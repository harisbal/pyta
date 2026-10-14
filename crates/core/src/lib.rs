//! PyTA core: static traffic assignment.
//!
//! The crate does no file IO; it works on typed arrays so Rust and Python users call the
//! same code. A typical session builds a [`Network`] from [`Edge`]s, a [`TurnTable`], a
//! delay function ([`Bpr`] or anything implementing [`Vdf`]) and an [`OdDemand`], then calls
//! [`gradient_projection()`] (or [`gradient_projection_multiclass()`]).
//!
//! * Link ids are dense (`0..num_links`) and index all per-link arrays; turn ids index
//!   per-turn arrays.
//! * Shortest paths are link-labelled so bans and turn costs are exact ([`dijkstra_turns()`]).
//! * [`frank_wolfe()`] is a simple reference solver; [`gradient_projection()`] is the main one.
//!
//! See the user guide in `docs/` for the concepts.

pub mod assignment;
pub mod frank_wolfe;
pub mod gradient_projection;
pub mod network;
pub mod order;
pub mod path_store;
pub mod shortest_path;
pub mod turns;
pub mod vdf;

pub use assignment::{AssignmentError, Flows, OdDemand, SolverError, all_or_nothing};
pub use frank_wolfe::{
    FrankWolfeOptions, FrankWolfeResult, frank_wolfe, frank_wolfe_with_turn_delays,
};
pub use gradient_projection::{
    ClassResult, ClassSpec, GradientProjectionOptions, GradientProjectionResult, MultiClassResult,
    PhaseTimes, Unrouted, find_unrouted, gradient_projection, gradient_projection_multiclass,
    gradient_projection_with_turn_delays,
};
pub use network::{Edge, Network, NetworkError};
pub use order::{OriginOrder, OriginSequencer};
pub use path_store::PathStore;
pub use shortest_path::{
    LinkTree, NO_PRED, ShortestPathTree, TurnSearch, dijkstra, dijkstra_turns,
};
pub use turns::{TurnError, TurnException, TurnOptions, TurnRule, TurnTable};
pub use vdf::{Bpr, NoDelay, Preloaded, Vdf, VdfError};
