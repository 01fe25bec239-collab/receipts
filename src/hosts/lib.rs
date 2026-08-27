//! Host Integration foundation: the host-neutral [`HostAdapter`]
//! translation boundary and the pure host detection/selection policy.
//!
//! This crate implements only the interface slice of `M-HOST-1`: the frozen
//! architectural boundary between the Receipts core and external hosts
//! (Claude Code, Codex, headless runners). An adapter is a translation
//! boundary and nothing else. It contains no orchestration, routing, state,
//! review, workspace, or runtime-worker logic, and it never gains authority
//! merely because an interface method exists.
//!
//! The [`host_detection`] module adds the pure resolution policy over
//! caller-supplied host-presence facts: explicit override first, then
//! single-host automatic detection, Headless as the non-host fallback, and
//! a typed failure for ambiguous Claude + Codex detection. It observes
//! nothing itself.
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
pub mod host_detection;
pub mod host_id;
pub mod normalized_host_event;
pub mod normalized_host_event_source_class;

#[cfg(test)]
mod adapter_tests;

#[cfg(test)]
mod host_detection_tests;

#[cfg(test)]
mod host_id_tests;

#[cfg(test)]
mod normalized_host_event_tests;

#[cfg(test)]
mod normalized_host_event_source_class_tests;

pub use adapter::HostAdapter;
pub use host_detection::{HostDetectionError, HostDetectionSignals, resolve_host};
pub use host_id::HostId;
pub use normalized_host_event::{NormalizedHostEventConfidence, NormalizedHostEventType};
pub use normalized_host_event_source_class::NormalizedHostEventSourceClass;
