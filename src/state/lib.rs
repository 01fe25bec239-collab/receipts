//! State-layer foundation for the Receipts orchestration core.
//!
//! This crate implements the first slice of `M-STATE-1`: the SQLite-backed
//! State repository boundary, mandatory SQLite configuration, versioned
//! forward-only migrations, and an atomic transaction primitive.
//!
//! Encapsulation rules honored by this crate:
//!
//! * no `rusqlite` type and no raw SQL string crosses the public API;
//! * callers cannot execute arbitrary SQL;
//! * the backing store can be replaced later without other managers ever
//!   depending on SQLite directly.
//!
//! The frozen architecture has a single authoritative State writer (the
//! orchestrator core). This foundation provides no worker-, host-, adapter-,
//! or model-facing mutation path.

pub mod error;
pub mod repository;

mod migrations;

#[cfg(test)]
mod tests;

pub use error::StateError;
pub use repository::SqliteStateRepository;
