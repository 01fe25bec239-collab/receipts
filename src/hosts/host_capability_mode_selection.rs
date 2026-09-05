//! Pure deterministic selection from already-observed capability facts.
//!
//! Authority: `HOST_CAPABILITY_DISCOVERY.md`,
//! `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`, and the capability
//! fixtures at architecture commit `f49d621ee510705939394f7df4996223a73fdcb7`.
//! No observation, fingerprint comparison, reprobe, or authorization occurs.
//!
//! Selection order after COMPLETE validation and policy assessment:
//! - Unproven validity: require reprobe, without selecting. Stale reports also
//!   require reprobe, except a current FAILED/PROBE_FAILED observation permits
//!   the explicitly prescribed conservative SUPERVISED result.
//! - Eligible native path: EMBEDDED, or indeterminate with an explicit override.
//! - Failed probe or unknown native prerequisites: conservative SUPERVISED.
//! - Known unavailable native path: SUPERVISED, preserving the policy reason.
//! - Insufficient required coverage: NONE selects SUPERVISED; a COMPLETE probe
//!   with PARTIAL vendor coverage and all other prerequisites satisfied selects
//!   HYBRID (positive fixture 04). FULL/UNKNOWN vendor coverage, unresolved other
//!   prerequisites, and incomplete probes do not uniquely establish HYBRID.
//! - Remaining non-eligible combinations: indeterminate, never guessed.
//!
//! Vendor coverage is independent of required lifecycle coverage; it is not
//! an additional EMBEDDED gate. An override's source and free-text reason do
//! not encode a target mode (positive fixture 07 supplies that mode separately).

use crate::{
    HostCapabilityConsistencyError, HostCapabilityConsistencyInputs,
    HostCapabilityFreshnessDisposition, HostCapabilityHookCoverageClass,
    HostCapabilityInactiveReason, HostCapabilityInactiveReasonInputs, HostCapabilityModeOverride,
    HostCapabilityNativePrerequisiteInputs, HostCapabilityNativePrerequisiteState,
    HostCapabilityProbeStatus, HostCapabilitySelectedMode,
    HostCapabilitySelectedModeConsistencyError, HostCapabilitySelectedModeConsistencyInputs,
    HostCapabilityStaleReason, assess_native_path_prerequisites, freshness_disposition,
    is_embedded_eligible, native_path_inactive_reason, validate_complete_probe_consistency,
    validate_selected_mode_consistency,
};

/// Already-observed facts only; nullable native facts retain their exact values.
/// The existing prerequisite input groups the nine capability/trust facts.
/// A supplied boolean or override grants no external authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityModeSelectionInputs {
    pub report_validity_proven_current: bool,
    pub probe_status: HostCapabilityProbeStatus,
    pub stale_reason: HostCapabilityStaleReason,
    pub native_prerequisites: HostCapabilityNativePrerequisiteInputs,
    pub hook_coverage_class: HostCapabilityHookCoverageClass,
    pub mode_override: Option<HostCapabilityModeOverride>,
}

/// Why the authority cannot uniquely choose HYBRID versus SUPERVISED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityModeSelectionIndeterminacy {
    /// Eligible native path has an override with no target-mode contract.
    OverrideHasNoTargetMode,
    /// Required coverage fails, but usable partial native coverage is unproven.
    CoverageDoesNotDetermineFallback,
    /// Remaining facts exclude EMBEDDED without a unique frozen fallback rule.
    NoUniqueFallbackRule,
}

/// A selection, an outstanding freshness step, or a bounded policy gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityModeSelectionOutcome {
    /// The emitted mode has passed the existing selected-mode validator.
    Selected {
        selected_mode: HostCapabilitySelectedMode,
        inactive_reason: HostCapabilityInactiveReason,
    },
    /// No mode selected; the caller must establish freshness through reprobe.
    ReprobeRequired,
    /// No mode selected; the derived reason is retained for review.
    Indeterminate {
        cause: HostCapabilityModeSelectionIndeterminacy,
        inactive_reason: HostCapabilityInactiveReason,
    },
}

/// Existing policy errors are retained without duplicating their rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityModeSelectionError {
    CompleteProbeConsistency(HostCapabilityConsistencyError),
    SelectedModeConsistency(HostCapabilitySelectedModeConsistencyError),
    /// Frozen `valid_host_report` requires FAILED to carry PROBE_FAILED.
    FailedProbeRequiresProbeFailedStaleReason,
    /// Frozen reports cannot claim an inactive reason of NONE in fallback.
    /// Contradictory supplied facts are rejected rather than rewritten UNKNOWN.
    ConservativeFallbackHasNoInactiveReason,
}

/// Derives a mode only where frozen authority uniquely determines it.
///
/// Eligibility, inactive reason, and final validation use the original facts. Unknowns
/// never become capability evidence. FAILED selects conservatively only after
/// freshness permits selection; supplied known facts still retain the existing
/// inactive-reason precedence, rather than being replaced with UNKNOWN.
pub fn select_host_capability_mode(
    inputs: &HostCapabilityModeSelectionInputs,
) -> Result<HostCapabilityModeSelectionOutcome, HostCapabilityModeSelectionError> {
    use HostCapabilityInactiveReason as Reason;
    use HostCapabilityModeSelectionIndeterminacy as Gap;
    use HostCapabilityModeSelectionOutcome as Outcome;
    use HostCapabilityNativePrerequisiteState as Native;
    use HostCapabilitySelectedMode as Mode;

    let facts = inputs.native_prerequisites;
    validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
        probe_status: inputs.probe_status,
        plugin_supported: facts.plugin_supported,
        plugin_installed: facts.plugin_installed,
        hooks_supported: facts.hooks_supported,
        hooks_configured: facts.hooks_configured,
        hooks_enabled: facts.hooks_enabled,
        hook_trust_required: facts.hook_trust_required,
        hooks_trusted: facts.hooks_trusted,
    })
    .map_err(HostCapabilityModeSelectionError::CompleteProbeConsistency)?;
    let freshness = freshness_disposition(inputs.report_validity_proven_current);
    let native = assess_native_path_prerequisites(facts);
    let inactive_reason = native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
        plugin_installed: facts.plugin_installed,
        hooks_supported: facts.hooks_supported,
        hooks_configured: facts.hooks_configured,
        hook_trust_required: facts.hook_trust_required,
        hooks_trusted: facts.hooks_trusted,
        hooks_enabled: facts.hooks_enabled,
        hooks_allowed_by_admin_policy: facts.hooks_allowed_by_admin_policy,
        required_hook_coverage_satisfied: facts.required_hook_coverage_satisfied,
        mode_override_present: inputs.mode_override.is_some(),
    });
    let indeterminate = |cause| Outcome::Indeterminate {
        cause,
        inactive_reason,
    };

    if inputs.probe_status == HostCapabilityProbeStatus::Failed
        && inputs.stale_reason != HostCapabilityStaleReason::ProbeFailed
    {
        return Err(HostCapabilityModeSelectionError::FailedProbeRequiresProbeFailedStaleReason);
    }

    if freshness == HostCapabilityFreshnessDisposition::ReprobeThenSelect
        || (inputs.stale_reason != HostCapabilityStaleReason::None
            && inputs.probe_status != HostCapabilityProbeStatus::Failed)
    {
        return Ok(Outcome::ReprobeRequired);
    }

    let selected_mode = if is_embedded_eligible(
        inputs.report_validity_proven_current,
        inputs.probe_status,
        inputs.stale_reason,
        native == Native::Satisfied,
    ) {
        if inputs.mode_override.is_some() {
            return Ok(indeterminate(Gap::OverrideHasNoTargetMode));
        }
        Mode::Embedded
    } else if inputs.probe_status == HostCapabilityProbeStatus::Failed || native == Native::Unknown
    {
        // Machine authority: unknown_or_failed = CONSERVATIVE_SUPERVISED.
        Mode::Supervised
    } else {
        match inactive_reason {
            // These reasons identify an unavailable path, not merely a gap in
            // lifecycle coverage. Precedence was already resolved by policy.
            Reason::PluginNotInstalled
            | Reason::HooksUnsupported
            | Reason::HooksNotConfigured
            | Reason::HooksUntrusted
            | Reason::HooksDisabled
            | Reason::HooksExcludedByAdminPolicy => Mode::Supervised,
            Reason::InsufficientCoverage => match inputs.hook_coverage_class {
                HostCapabilityHookCoverageClass::None => Mode::Supervised,
                HostCapabilityHookCoverageClass::Partial
                    if inputs.probe_status == HostCapabilityProbeStatus::Complete
                        // Counterfactual query: is coverage the ONLY blocker?
                        // This does not replace the original facts or native
                        // assessment used for eligibility/final validation.
                        // Reuse the policy, including conditional trust, rather
                        // than restating its truth table for the other fields.
                        && assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
                            required_hook_coverage_satisfied: Some(true),
                            ..facts
                        }) == Native::Satisfied =>
                {
                    Mode::Hybrid
                }
                _ => return Ok(indeterminate(Gap::CoverageDoesNotDetermineFallback)),
            },
            _ => return Ok(indeterminate(Gap::NoUniqueFallbackRule)),
        }
    };

    validate_selected_mode_consistency(&HostCapabilitySelectedModeConsistencyInputs {
        report_validity_proven_current: inputs.report_validity_proven_current,
        probe_status: inputs.probe_status,
        stale_reason: inputs.stale_reason,
        native_prerequisite_state: native,
        selected_mode,
        mode_override: inputs.mode_override.clone(),
        inactive_reason: Some(inactive_reason),
    })
    .map_err(HostCapabilityModeSelectionError::SelectedModeConsistency)?;

    if selected_mode != Mode::Embedded && inactive_reason == Reason::None {
        return Err(HostCapabilityModeSelectionError::ConservativeFallbackHasNoInactiveReason);
    }

    Ok(Outcome::Selected {
        selected_mode,
        inactive_reason,
    })
}
