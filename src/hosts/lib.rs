//! Host Integration foundation: the host-neutral [`HostAdapter`]
//! translation boundary.
//!
//! This crate implements only the interface slice of `M-HOST-1`: the frozen
//! architectural boundary between the Receipts core and external hosts
//! (Claude Code, Codex, headless runners). An adapter is a translation
//! boundary and nothing else. It contains no orchestration, routing, state,
//! review, workspace, or runtime-worker logic, and it never gains authority
//! merely because an interface method exists.
//!
//! Boundary rules honored by this crate:
//!
//! * Rust `std`/`core` only; zero dependencies, zero feature flags;
//! * no concrete host behavior: no detection or environment/process
//!   probing, no installation, no process control, no Claude/Codex hooks,
//!   no plugin manifests or packaging, no event normalization, no
//!   capability probing, no rendering, no credential handling, no
//!   networking, no subprocess/shell execution, and no filesystem writes;
//! * adjacent frozen contracts (`InstallPlan`, `CoreHandle`,
//!   `NormalizedHostEvent`, `CoreView`, `UserPrompt`, `UserResponse`,
//!   `HostCapabilityReport`, shutdown reason) remain externally owned and
//!   appear here only as unbound associated-type placeholders;
//! * no authoritative state read or write path exists here.

pub mod adapter;
pub mod host_id;

#[cfg(test)]
mod adapter_tests;

#[cfg(test)]
mod host_id_tests;

pub use adapter::HostAdapter;
pub use host_id::HostId;
