//! Fresh bounded observation followed by the accepted report composition.
//!
//! Freshness means only that this invocation produced the facts. It does not
//! establish completeness, trust, or native eligibility. No cached report or
//! fingerprint is consumed; the observation adapter leaves the fingerprint None.

use crate::{
    HostCapabilityModeOverride, HostCapabilityReportSelectionCompositionError,
    HostCapabilityReportSelectionCompositionOutcome, HostCapabilityStaleReason,
    compose_host_capability_report_selection,
    host_capability_observation::{
        HostCapabilityObservation, HostCapabilityObservationRequest, observe_host_capabilities,
    },
};

/// Observation context and an optional existing, validated override only.
/// Callers cannot supply freshness, stale reasons, observations, or cached reports.
///
/// ```compile_fail,E0609
/// use receipts_host_integration::HostCapabilityReportRefreshRequest;
/// fn forge(mut request: HostCapabilityReportRefreshRequest) {
///     request.report_validity_proven_current = true;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::HostCapabilityReportRefreshRequest;
/// fn forge(mut request: HostCapabilityReportRefreshRequest) {
///     request.report_validity_proven_current = false;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostCapabilityReportRefreshRequest, HostCapabilityStaleReason};
/// fn forge(mut request: HostCapabilityReportRefreshRequest) {
///     request.stale_reason = HostCapabilityStaleReason::None;
/// }
/// ```
/// ```compile_fail,E0308
/// use receipts_host_integration::{HostCapabilityReportRefreshRequest,
///     host_capability_observation::HostCapabilityObservation};
/// fn forge(observation: HostCapabilityObservation) {
///     let _ = HostCapabilityReportRefreshRequest { observation, mode_override: None };
/// }
/// ```
/// ```compile_fail,E0560
/// use receipts_host_integration::{HostCapabilityReportRefreshRequest,
///     HostCapabilityReportNonTemporalCore,
///     host_capability_observation::HostCapabilityObservationRequest};
/// fn reuse(observation: HostCapabilityObservationRequest, report: HostCapabilityReportNonTemporalCore) {
///     let _ = HostCapabilityReportRefreshRequest {
///         observation, mode_override: None, cached_report: report,
///     };
/// }
/// ```
pub struct HostCapabilityReportRefreshRequest {
    pub observation: HostCapabilityObservationRequest,
    pub mode_override: Option<HostCapabilityModeOverride>,
}

/// Both stages remain visible, including source errors and unchanged composition
/// errors. Unresolved composition outcomes carry no selected report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityReportRefreshOutcome {
    pub observation: HostCapabilityObservation,
    pub report_selection: Result<
        HostCapabilityReportSelectionCompositionOutcome,
        HostCapabilityReportSelectionCompositionError,
    >,
}

/// Performs the integrated observation on EVERY call, then delegates adaptation
/// and report selection. There is no public observation-provider injection.
///
/// ```compile_fail,E0603
/// use receipts_host_integration::host_capability_report_refresh::refresh_with_observer;
/// ```
pub fn refresh_host_capability_report(
    request: &HostCapabilityReportRefreshRequest,
) -> HostCapabilityReportRefreshOutcome {
    refresh_with_observer(request, observe_host_capabilities)
}

fn refresh_with_observer(
    request: &HostCapabilityReportRefreshRequest,
    observe: impl FnOnce(&HostCapabilityObservationRequest) -> HostCapabilityObservation,
) -> HostCapabilityReportRefreshOutcome {
    let observation = observe(&request.observation);
    let inputs = observation.report_selection_inputs(
        true,
        HostCapabilityStaleReason::None,
        request.mode_override.clone(),
    );
    let report_selection = compose_host_capability_report_selection(&inputs);
    HostCapabilityReportRefreshOutcome {
        observation,
        report_selection,
    }
}

#[cfg(test)]
#[path = "host_capability_report_refresh_tests.rs"]
mod tests;
