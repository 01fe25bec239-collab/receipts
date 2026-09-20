//! Passive entitlement-axis evidence: may OUR product capability execute?
//! No evaluation, dispatch, persistence, or wire serialization is provided.

use crate::graph::CapabilityName;
use crate::orchestration::OrchestrationDateTimeV1;
use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};

/// Exactly the frozen entitlement-only outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureAdmissionOutcome {
    Allow,
    LockedRequiresPro,
    EntitlementUnknown,
    EntitlementExpired,
}

impl FeatureAdmissionOutcome {
    pub const ALL: [Self; 4] = [
        Self::Allow,
        Self::LockedRequiresPro,
        Self::EntitlementUnknown,
        Self::EntitlementExpired,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            Self::EntitlementUnknown => "ENTITLEMENT_UNKNOWN",
            Self::EntitlementExpired => "ENTITLEMENT_EXPIRED",
        }
    }
}

/// Present upgrade evidence; the enclosing `Option` represents absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureAdmissionUpgradeInfo {
    Null,
    String(String),
}

/// The only structural validation owned by this record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureAdmissionDecisionError {
    DecisionIdLengthOutOfRange { character_count: usize },
}

/// Immutable, caller-supplied evidence, without consistency evaluation.
/// Capability and timestamp must already be validated canonical values.
///
/// A raw capability cannot bypass its canonical type:
/// ```compile_fail
/// use receipts_orchestration::orchestration::{FeatureAdmissionDecision, FeatureAdmissionOutcome,
///     OrchestrationDateTimeV1};
/// use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};
/// FeatureAdmissionDecision::try_new("id", "malformed".to_owned(),
///     FeatureAdmissionOutcome::Allow, ProductEntitlementState::Free,
///     ActivationStateKind::NeverActivated, None, None, None, None,
///     OrchestrationDateTimeV1::try_new("2026-09-20T00:00:00Z").unwrap());
/// ```
/// The timestamp cannot be absent:
/// ```compile_fail
/// use receipts_orchestration::{graph::CapabilityName, orchestration::{
///     FeatureAdmissionDecision, FeatureAdmissionOutcome}};
/// use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};
/// FeatureAdmissionDecision::try_new("id", CapabilityName::new("graph.core").unwrap(),
///     FeatureAdmissionOutcome::Allow, ProductEntitlementState::Free,
///     ActivationStateKind::NeverActivated, None, None, None, None, None);
/// ```
/// Nor can it be omitted:
/// ```compile_fail
/// use receipts_orchestration::{graph::CapabilityName, orchestration::{
///     FeatureAdmissionDecision, FeatureAdmissionOutcome}};
/// use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};
/// FeatureAdmissionDecision::try_new("id", CapabilityName::new("graph.core").unwrap(),
///     FeatureAdmissionOutcome::Allow, ProductEntitlementState::Free,
///     ActivationStateKind::NeverActivated, None, None, None, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureAdmissionDecision {
    decision_id: String,
    capability_id: CapabilityName,
    outcome: FeatureAdmissionOutcome,
    entitlement_state: ProductEntitlementState,
    activation_state: ActivationStateKind,
    tier_id: Option<String>,
    reason: Option<String>,
    upgrade_info: Option<FeatureAdmissionUpgradeInfo>,
    dispatch_permitted: Option<bool>,
    decided_at: OrchestrationDateTimeV1,
}

impl FeatureAdmissionDecision {
    /// Validates only the decision ID's 1..=200 Unicode scalar value bound.
    /// All supplied values are stored exactly, even when semantically unusual.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        decision_id: impl Into<String>,
        capability_id: CapabilityName,
        outcome: FeatureAdmissionOutcome,
        entitlement_state: ProductEntitlementState,
        activation_state: ActivationStateKind,
        tier_id: Option<String>,
        reason: Option<String>,
        upgrade_info: Option<FeatureAdmissionUpgradeInfo>,
        dispatch_permitted: Option<bool>,
        decided_at: OrchestrationDateTimeV1,
    ) -> Result<Self, FeatureAdmissionDecisionError> {
        let decision_id = decision_id.into();
        let character_count = decision_id.chars().count();
        if !(1..=200).contains(&character_count) {
            return Err(FeatureAdmissionDecisionError::DecisionIdLengthOutOfRange {
                character_count,
            });
        }
        Ok(Self {
            decision_id,
            capability_id,
            outcome,
            entitlement_state,
            activation_state,
            tier_id,
            reason,
            upgrade_info,
            dispatch_permitted,
            decided_at,
        })
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    pub fn capability_id(&self) -> &CapabilityName {
        &self.capability_id
    }

    pub fn outcome(&self) -> FeatureAdmissionOutcome {
        self.outcome
    }

    pub fn entitlement_state(&self) -> ProductEntitlementState {
        self.entitlement_state
    }

    pub fn activation_state(&self) -> ActivationStateKind {
        self.activation_state
    }

    pub fn tier_id(&self) -> Option<&str> {
        self.tier_id.as_deref()
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn upgrade_info(&self) -> Option<&FeatureAdmissionUpgradeInfo> {
        self.upgrade_info.as_ref()
    }

    pub fn dispatch_permitted(&self) -> Option<bool> {
        self.dispatch_permitted
    }

    pub fn decided_at(&self) -> &OrchestrationDateTimeV1 {
        &self.decided_at
    }
}
