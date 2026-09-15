//! Physical in-process composition of caller-supplied host capability evidence.

use crate::HostCapabilityReportNonTemporalCore;
use receipts_orchestration::orchestration::OrchestrationDateTimeV1;

/// An immutable non-temporal core with both required lexical date-time values.
///
/// Construction stores the supplied objects unchanged. It performs no probing,
/// I/O, clock access, temporal comparison, normalization, or additional validation.
/// This record proves no capability truth, trust, currency, fingerprint validity,
/// cache reusability, event emission, plugin activity, or runtime authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityReport {
    core: HostCapabilityReportNonTemporalCore,
    probed_at: OrchestrationDateTimeV1,
    last_verified_at: OrchestrationDateTimeV1,
}

impl HostCapabilityReport {
    /// Composes already-validated values without relating the two timestamps.
    /// A `last_verified_at` earlier than `probed_at` is accepted unchanged.
    ///
    /// Both temporal arguments are required:
    ///
    /// ```compile_fail,E0061
    /// use receipts_host_integration::{HostCapabilityReport, HostCapabilityReportNonTemporalCore};
    /// fn missing_both(core: HostCapabilityReportNonTemporalCore) {
    ///     HostCapabilityReport::new(core);
    /// }
    /// ```
    ///
    /// ```compile_fail,E0061
    /// use receipts_host_integration::{HostCapabilityReport, HostCapabilityReportNonTemporalCore};
    /// use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
    /// fn missing_last_verified_at(core: HostCapabilityReportNonTemporalCore, probed_at: OrchestrationDateTimeV1) {
    ///     HostCapabilityReport::new(core, probed_at);
    /// }
    /// ```
    ///
    /// Neither temporal argument accepts absence:
    ///
    /// ```compile_fail,E0308
    /// use receipts_host_integration::{HostCapabilityReport, HostCapabilityReportNonTemporalCore};
    /// use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
    /// fn missing_probed_at(core: HostCapabilityReportNonTemporalCore, verified: OrchestrationDateTimeV1) {
    ///     HostCapabilityReport::new(core, None, verified);
    /// }
    /// ```
    ///
    /// ```compile_fail,E0308
    /// use receipts_host_integration::{HostCapabilityReport, HostCapabilityReportNonTemporalCore};
    /// use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
    /// fn absent_last_verified_at(core: HostCapabilityReportNonTemporalCore, probed: OrchestrationDateTimeV1) {
    ///     HostCapabilityReport::new(core, probed, None);
    /// }
    /// ```
    pub fn new(
        core: HostCapabilityReportNonTemporalCore,
        probed_at: OrchestrationDateTimeV1,
        last_verified_at: OrchestrationDateTimeV1,
    ) -> Self {
        Self {
            core,
            probed_at,
            last_verified_at,
        }
    }

    /// Returns the unchanged non-temporal core.
    pub fn core(&self) -> &HostCapabilityReportNonTemporalCore {
        &self.core
    }

    /// Returns the caller-supplied probe date-time evidence exactly.
    pub fn probed_at(&self) -> &OrchestrationDateTimeV1 {
        &self.probed_at
    }

    /// Returns the caller-supplied verification date-time evidence exactly.
    pub fn last_verified_at(&self) -> &OrchestrationDateTimeV1 {
        &self.last_verified_at
    }
}
