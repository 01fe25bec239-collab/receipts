//! Pure host capability freshness disposition and EMBEDDED eligibility policy.
//!
//! Callers supply already-observed facts. This module performs no fingerprint
//! checking, probing, reprobe execution, or mode selection, and supplied facts
//! do not themselves grant authority.

use crate::{HostCapabilityProbeStatus, HostCapabilityStaleReason};

/// The closed disposition after a caller has determined whether report
/// validity is proven current.
///
/// This value describes the next step only; it performs no checking, probing,
/// or mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityFreshnessDisposition {
    /// Reuse the report because the caller supplied proven-current validity.
    ///
    /// This does not imply that EMBEDDED is eligible or selected.
    ReuseReport,
    /// Reprobe before a later, separate mode-selection step.
    ///
    /// This value does not execute a probe or select any mode.
    ReprobeThenSelect,
}

impl HostCapabilityFreshnessDisposition {
    /// Every disposition in machine-authority order.
    pub const ALL: [Self; 2] = [Self::ReuseReport, Self::ReprobeThenSelect];

    /// Returns the exact machine-authority string for this disposition.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReuseReport => "REUSE_REPORT",
            Self::ReprobeThenSelect => "REPROBE_THEN_SELECT",
        }
    }
}

/// Purely maps caller-supplied report-validity proof to its disposition.
///
/// No fingerprint is checked and no probe or mode selection is performed.
/// A supplied `true` value records a fact only and grants no external authority.
pub const fn freshness_disposition(
    report_validity_proven_current: bool,
) -> HostCapabilityFreshnessDisposition {
    if report_validity_proven_current {
        HostCapabilityFreshnessDisposition::ReuseReport
    } else {
        HostCapabilityFreshnessDisposition::ReprobeThenSelect
    }
}

/// Purely determines whether caller-supplied facts make EMBEDDED eligible.
///
/// This predicate performs no validity checking or probing and does not select
/// EMBEDDED (or any other mode). Caller-supplied facts do not themselves grant
/// authority; every prerequisite must have been established by the caller.
pub const fn is_embedded_eligible(
    report_validity_proven_current: bool,
    probe_status: HostCapabilityProbeStatus,
    stale_reason: HostCapabilityStaleReason,
    all_native_prerequisites: bool,
) -> bool {
    report_validity_proven_current
        && matches!(probe_status, HostCapabilityProbeStatus::Complete)
        && matches!(stale_reason, HostCapabilityStaleReason::None)
        && all_native_prerequisites
}
