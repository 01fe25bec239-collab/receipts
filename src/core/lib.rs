//! Minimal Orchestration crate facade.
//!
//! Existing graph API remains available at crate root while graph is also
//! exposed as a proper module.
//!
//! ```
//! use receipts_orchestration::GraphNodeState;
//! use receipts_orchestration::graph;
//!
//! assert_eq!(GraphNodeState::Planned.as_str(), "PLANNED");
//! assert_eq!(graph::GraphNodeState::Planned.as_str(), "PLANNED");
//! ```

#[path = "graph/lib.rs"]
pub mod graph;

pub use graph::*;

#[cfg(test)]
mod facade_tests;
