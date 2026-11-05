//! mneme-core: an append-only, content-addressed claim log with git-like history,
//! comprehension frames, and regenerable projections (graph, wiki, visual explorer).
//!
//! The log is the memory. Everything else (graph, pages, indexes, the viewer) is a view
//! compiled from it and can be thrown away and rebuilt.

pub mod digest;
pub mod frames;
pub mod model;
pub mod project;
pub mod recall;
pub mod relations;
pub mod state;
pub mod store;
pub mod util;

pub use model::*;
pub use state::State;
pub use store::Repo;
