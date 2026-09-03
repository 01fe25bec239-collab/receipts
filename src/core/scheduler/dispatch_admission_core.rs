//! Bounded core value object for the frozen `DispatchAdmissionDecision`
//! contract.
//!
//! This slice implements **only** [`DispatchAdmissionDecisionCore`]: the nine
//! stored fields named by the task authorization plus the single authorized
//! structural invariant (`ALLOW` requires failing axis `NONE`; `DENY`
//! requires a non-`NONE` failing axis). It deliberately contains:
//!
//! * no `DispatchAdmissionDecision` aggregate, `axis_results`, or `decided_at`;
//! * no per-axis record objects, timestamps, or date parsing;
//! * no admission composition, precedence, derivation, or execution;
//! * no denial-reason-to-axis mapping (`denial_reason` is storage only);
//! * no provider/runtime pair or presence rules;
//! * no provider-policy, availability, safety, quality-floor, or routing
//!   behavior;
//! * no scheduler, graph, persistence, serialization, or I/O behavior.
//!
//! All length checks count Unicode scalar values (`str::chars().count()`),
//! never UTF-8 bytes (`str::len()`). Accepted strings are stored
//! byte-for-byte: no trimming, normalization, or case folding.

use super::dispatch_admission_vocabulary::{
    DispatchAdmissionDenialReason, DispatchAdmissionFailingAxis, DispatchAdmissionOutcome,
};

/// Maximum character count for the bounded identifier fields.
const BOUNDED_ID_MAX_CHARACTERS: usize = 200;

/// Typed construction error for [`DispatchAdmissionDecisionCore`].
///
/// Limited to exactly the validations this slice owns: bounded identifier
/// lengths and the outcome/failing-axis structural invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAdmissionDecisionCoreError {
    /// `decision_id` fell outside `1..=200` characters.
    DecisionIdLengthOutOfRange {
        /// The rejected character count (`value.chars().count()`).
        character_count: usize,
    },
    /// `node_id` fell outside `1..=200` characters.
    NodeIdLengthOutOfRange {
        /// The rejected character count (`value.chars().count()`).
        character_count: usize,
    },
    /// A supplied `graph_id` fell outside `1..=200` characters.
    GraphIdLengthOutOfRange {
        /// The rejected character count (`value.chars().count()`).
        character_count: usize,
    },
    /// `ALLOW` requires failing axis `NONE`; another axis was supplied.
    AllowRequiresNoFailingAxis {
        /// The rejected non-`NONE` failing axis, echoed back verbatim.
        failing_axis: DispatchAdmissionFailingAxis,
    },
    /// `DENY` requires a non-`NONE` failing axis; `NONE` was supplied.
    DenyRequiresFailingAxis,
}

impl std::fmt::Display for DispatchAdmissionDecisionCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DecisionIdLengthOutOfRange { character_count } => write!(
                f,
                "decision_id length out of range: {character_count} characters, expected 1..=200"
            ),
            Self::NodeIdLengthOutOfRange { character_count } => write!(
                f,
                "node_id length out of range: {character_count} characters, expected 1..=200"
            ),
            Self::GraphIdLengthOutOfRange { character_count } => write!(
                f,
                "graph_id length out of range: {character_count} characters, expected 1..=200"
            ),
            Self::AllowRequiresNoFailingAxis { failing_axis } => write!(
                f,
                "ALLOW requires failing axis NONE, got {}",
                failing_axis.as_str()
            ),
            Self::DenyRequiresFailingAxis => {
                write!(f, "DENY requires a non-NONE failing axis, got NONE")
            }
        }
    }
}

impl std::error::Error for DispatchAdmissionDecisionCoreError {}

/// Bounded core value object for a dispatch admission decision.
///
/// All fields are private. Construction runs through [`Self::try_new`], which
/// enforces the bounded identifier lengths and the outcome/failing-axis
/// invariant; afterwards the object is read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionDecisionCore {
    decision_id: String,
    node_id: String,
    graph_id: Option<String>,
    capability_id: Option<String>,
    outcome: DispatchAdmissionOutcome,
    failing_axis: DispatchAdmissionFailingAxis,
    denial_reason: Option<DispatchAdmissionDenialReason>,
    selected_provider: Option<String>,
    selected_runtime: Option<String>,
}

impl DispatchAdmissionDecisionCore {
    /// Constructs a core decision, validating the owned constraints.
    ///
    /// Validation sequence: `decision_id` length, `node_id` length, supplied
    /// `graph_id` length, then the outcome/failing-axis invariant. Every other
    /// field is stored exactly as supplied.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        decision_id: String,
        node_id: String,
        graph_id: Option<String>,
        capability_id: Option<String>,
        outcome: DispatchAdmissionOutcome,
        failing_axis: DispatchAdmissionFailingAxis,
        denial_reason: Option<DispatchAdmissionDenialReason>,
        selected_provider: Option<String>,
        selected_runtime: Option<String>,
    ) -> Result<Self, DispatchAdmissionDecisionCoreError> {
        let decision_id_characters = decision_id.chars().count();
        if !(1..=BOUNDED_ID_MAX_CHARACTERS).contains(&decision_id_characters) {
            return Err(
                DispatchAdmissionDecisionCoreError::DecisionIdLengthOutOfRange {
                    character_count: decision_id_characters,
                },
            );
        }

        let node_id_characters = node_id.chars().count();
        if !(1..=BOUNDED_ID_MAX_CHARACTERS).contains(&node_id_characters) {
            return Err(DispatchAdmissionDecisionCoreError::NodeIdLengthOutOfRange {
                character_count: node_id_characters,
            });
        }

        if let Some(graph_id_value) = graph_id.as_ref() {
            let graph_id_characters = graph_id_value.chars().count();
            if !(1..=BOUNDED_ID_MAX_CHARACTERS).contains(&graph_id_characters) {
                return Err(
                    DispatchAdmissionDecisionCoreError::GraphIdLengthOutOfRange {
                        character_count: graph_id_characters,
                    },
                );
            }
        }

        match (outcome, failing_axis) {
            (DispatchAdmissionOutcome::Allow, DispatchAdmissionFailingAxis::None) => {}
            (DispatchAdmissionOutcome::Allow, axis) => {
                return Err(
                    DispatchAdmissionDecisionCoreError::AllowRequiresNoFailingAxis {
                        failing_axis: axis,
                    },
                );
            }
            (DispatchAdmissionOutcome::Deny, DispatchAdmissionFailingAxis::None) => {
                return Err(DispatchAdmissionDecisionCoreError::DenyRequiresFailingAxis);
            }
            (DispatchAdmissionOutcome::Deny, _) => {}
        }

        Ok(Self {
            decision_id,
            node_id,
            graph_id,
            capability_id,
            outcome,
            failing_axis,
            denial_reason,
            selected_provider,
            selected_runtime,
        })
    }

    /// The dispatch admission decision identifier, verbatim.
    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    /// The graph node requesting dispatch, verbatim.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// The graph identifier, when the property is present.
    pub fn graph_id(&self) -> Option<&str> {
        self.graph_id.as_deref()
    }

    /// The required product capability, when present. Unconstrained storage.
    pub fn capability_id(&self) -> Option<&str> {
        self.capability_id.as_deref()
    }

    /// The admission outcome.
    pub fn outcome(&self) -> DispatchAdmissionOutcome {
        self.outcome
    }

    /// The single failing axis (`NONE` only for `ALLOW`).
    pub fn failing_axis(&self) -> DispatchAdmissionFailingAxis {
        self.failing_axis
    }

    /// The stored denial reason, if any. Storage only: no mapping to
    /// outcome or failing axis is enforced.
    pub fn denial_reason(&self) -> Option<DispatchAdmissionDenialReason> {
        self.denial_reason
    }

    /// The selected provider reference, when present. Unconstrained storage.
    pub fn selected_provider(&self) -> Option<&str> {
        self.selected_provider.as_deref()
    }

    /// The selected runtime reference, when present. Unconstrained storage.
    pub fn selected_runtime(&self) -> Option<&str> {
        self.selected_runtime.as_deref()
    }
}
