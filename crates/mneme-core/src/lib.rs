//! mneme-core: the memory engine.
//!
//! An append-only, content-addressed claim log with git-like history ([`store`]), replayed into
//! an indexed [`state::State`]; changes are staged with [`state::Batch`]. Comprehension frames
//! ([`frames`]) measure understanding; [`digest`] validates interpretations; [`recall`] compiles
//! context packages; [`project`] builds regenerable views. Knowledge types come from
//! `organon-protocol`, so other organs speak the same language.
//!
//! This crate does no terminal output and no network I/O: the CLI does.

#![forbid(unsafe_code)]

pub mod digest;
pub mod error;
pub mod frames;
pub mod model;
pub mod project;
pub mod recall;
pub mod state;
pub mod store;
pub mod util;

pub use error::{Error, Result};
pub use frames::Frames;
pub use state::{Batch, State};
pub use store::Repo;
