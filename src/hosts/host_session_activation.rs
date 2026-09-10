//! Session start/resume preparation using the integrated Host boundaries.
//!
//! This is technical readiness consideration only: successful identity checks
//! do not mean report selection succeeded. Consumers must inspect the retained
//! refresh/selection result. No adapter session handle is started here.

use std::path::PathBuf;

use crate::{
    HostAdapter, HostCapabilityFreshnessDisposition, HostCapabilityModeOverride,
    HostCapabilityReportNonTemporalCore, HostCapabilityReportRefreshOutcome,
    HostCapabilityReportRefreshRequest, HostDetectionError, HostDetectionSignals, HostId,
    freshness_disposition, host_capability_observation::HostCapabilityObservationRequest,
    refresh_host_capability_report, resolve_host,
};

/// Already-observed presence, authorized overrides, and bounded observation context.
/// The cached candidate is never current evidence, even with a nonempty fingerprint.
/// No timestamp, mtime, current fingerprint, or caller validity assertion is accepted.
/// Project root has the existing observation request's fixed-source semantics.
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.report_validity_proven_current = true;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.cache_is_valid = true;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.fingerprints_match = true;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.current_fingerprint = String::new();
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest, HostCapabilityStaleReason};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.stale_reason = HostCapabilityStaleReason::None;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest, HostCapabilitySelectedMode};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.selected_mode = HostCapabilitySelectedMode::Embedded;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest, HostCapabilityInactiveReason};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.inactive_reason = HostCapabilityInactiveReason::None;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest, host_capability_observation::HostCapabilityObservation};
/// fn forge(mut request: HostSessionActivationRequest<'_>, value: HostCapabilityObservation) {
///     request.observation = value;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest, HostId};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.observation_host = HostId::Codex;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.probed_at = 0;
/// }
/// ```
/// ```compile_fail,E0609
/// use receipts_host_integration::{HostSessionActivationRequest};
/// fn forge(mut request: HostSessionActivationRequest<'_>) {
///     request.mtime = 0;
/// }
/// ```
pub struct HostSessionActivationRequest<'a> {
    pub detection_signals: HostDetectionSignals,
    pub explicit_host_override: Option<HostId>,
    pub project_root: PathBuf,
    pub mode_override: Option<HostCapabilityModeOverride>,
    pub cached_report: Option<&'a HostCapabilityReportNonTemporalCore>,
}

/// Identity reconciled and capability refresh attempted, not unconditional readiness.
/// Selection errors, indeterminacy, and outstanding reprobe remain in `capability_refresh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSessionActivationOutcome {
    pub resolved_host: HostId,
    /// Disposition of prior capability state at session entry, before refresh.
    pub freshness_disposition: HostCapabilityFreshnessDisposition,
    pub capability_refresh: HostCapabilityReportRefreshOutcome,
}

/// Startup identity failures occur before any capability observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSessionActivationError {
    Detection(HostDetectionError),
    AdapterHostMismatch {
        resolved_host: HostId,
        adapter_host: HostId,
    },
}

impl std::fmt::Display for HostSessionActivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Detection(error) => error.fmt(f),
            Self::AdapterHostMismatch {
                resolved_host,
                adapter_host,
            } => write!(
                f,
                "resolved host {} does not match adapter {}",
                resolved_host.as_str(),
                adapter_host.as_str(),
            ),
        }
    }
}

impl std::error::Error for HostSessionActivationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Detection(error) => Some(error),
            Self::AdapterHostMismatch { .. } => None,
        }
    }
}

/// Prepare a session start or resume; not a per-command probe.
///
/// Positive cache reuse is currently unavailable: no integrated physical
/// fingerprint proof exists. Every reconciled entry delegates to A3-021.
/// `HostAdapter::id` is the only adapter operation consumed.
///
/// The private refresh seam cannot be used by production callers.
/// ```compile_fail,E0603
/// use receipts_host_integration::host_session_activation::prepare_with_refresh;
/// ```
pub fn prepare_host_session<A: HostAdapter>(
    adapter: &A,
    request: &HostSessionActivationRequest<'_>,
) -> Result<HostSessionActivationOutcome, HostSessionActivationError> {
    prepare_with_refresh(adapter, request, refresh_host_capability_report)
}

fn prepare_with_refresh<A: HostAdapter>(
    adapter: &A,
    request: &HostSessionActivationRequest<'_>,
    refresh: impl FnOnce(&HostCapabilityReportRefreshRequest) -> HostCapabilityReportRefreshOutcome,
) -> Result<HostSessionActivationOutcome, HostSessionActivationError> {
    let resolved_host = resolve_host(request.detection_signals, request.explicit_host_override)
        .map_err(HostSessionActivationError::Detection)?;
    let adapter_host = adapter.id();
    if resolved_host != adapter_host {
        return Err(HostSessionActivationError::AdapterHostMismatch {
            resolved_host,
            adapter_host,
        });
    }

    // SESSION_START_OR_RESUME: every candidate's validity is unproven. Neither
    // its contents nor its absence can supply the missing physical proof.
    // ponytail: always refresh until an authoritative fingerprint proof is integrated.
    let freshness_disposition = freshness_disposition(false);
    let capability_refresh = refresh(&HostCapabilityReportRefreshRequest {
        observation: HostCapabilityObservationRequest {
            host: resolved_host,
            project_root: request.project_root.clone(),
        },
        mode_override: request.mode_override.clone(),
    });
    Ok(HostSessionActivationOutcome {
        resolved_host,
        freshness_disposition,
        capability_refresh,
    })
}

#[cfg(test)]
#[path = "host_session_activation_tests.rs"]
mod tests;
