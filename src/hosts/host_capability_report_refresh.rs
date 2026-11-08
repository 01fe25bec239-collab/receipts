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
        HostCapabilityObservation, HostCapabilityObservationRequest,
        observe_host_capabilities_with_fingerprint,
    },
    host_probe_port::{
        HostCapabilityCacheEntryV1, HostObservationFingerprintV1, HostProbeProcessPort,
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
    /// Digest-only fingerprint of this observation, for diagnostics. It
    /// authorizes nothing on its own; `None` for injected test observers.
    pub fingerprint: Option<HostObservationFingerprintV1>,
    /// Minted here only when the fingerprint is complete and a report was
    /// selected from this same observation and mode override.
    pub cache_entry: Option<HostCapabilityCacheEntryV1>,
}

/// Performs the integrated observation on EVERY call, then delegates adaptation
/// and report selection. There is no public observation-provider injection.
///
/// ```compile_fail,E0603
/// use receipts_host_integration::host_capability_report_refresh::refresh_with_observer;
/// ```
pub fn refresh_host_capability_report(
    request: &HostCapabilityReportRefreshRequest,
    port: &dyn HostProbeProcessPort,
) -> HostCapabilityReportRefreshOutcome {
    let (observation, fingerprint) =
        observe_host_capabilities_with_fingerprint(&request.observation, port);
    let outcome = refresh_with_observer(request, |_| observation);
    let cache_entry = match &outcome.report_selection {
        Ok(HostCapabilityReportSelectionCompositionOutcome::Selected { report }) => {
            HostCapabilityCacheEntryV1::mint(
                fingerprint.clone(),
                request.mode_override.clone(),
                (**report).clone(),
            )
        }
        _ => None,
    };
    HostCapabilityReportRefreshOutcome {
        fingerprint: Some(fingerprint),
        cache_entry,
        ..outcome
    }
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
        fingerprint: None,
        cache_entry: None,
    }
}

#[cfg(test)]
#[path = "host_capability_report_refresh_tests.rs"]
mod tests;
