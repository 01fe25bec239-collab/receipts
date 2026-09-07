//! Deterministic, non-temporal projection of `GoalEvaluation.schema.json`.
//!
//! `GLOBAL_GOAL_EVALUATOR.md` requires deterministic gates before semantic
//! evaluation. This module validates supplied layer results and state; it does
//! not derive a state from prose or collect evidence. Callers must report missing
//! required evidence and blocking facts as deterministic failures.
//!
//! This is not the complete physical GoalEvaluation or completion authority:
//! `evaluated_at`, semantic evaluation, gaps, and convergence are deferred.
//! In particular, accepting a caller-supplied COMPLETE state only establishes
//! deterministic consistency, not that human-language criteria are satisfied.
//! A full evaluator must still establish semantic satisfaction before completion.
//! No model, routing, clock, persistence, or external operations are involved.

/// Closed GoalEvaluation state vocabulary, owned independently of graph states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GoalEvaluationState {
    Complete,
    Incomplete,
    Blocked,
    HumanRequired,
}

impl GoalEvaluationState {
    pub const ALL: [Self; 4] = [
        Self::Complete,
        Self::Incomplete,
        Self::Blocked,
        Self::HumanRequired,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "COMPLETE",
            Self::Incomplete => "INCOMPLETE",
            Self::Blocked => "BLOCKED",
            Self::HumanRequired => "HUMAN_REQUIRED",
        }
    }
}

/// Closed result vocabulary belonging to GoalEvaluation deterministic conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeterministicConditionResult {
    Pass,
    Fail,
    NotApplicable,
}

impl DeterministicConditionResult {
    pub const ALL: [Self; 3] = [Self::Pass, Self::Fail, Self::NotApplicable];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

/// Exact condition text and supplied result. Empty text is permitted by schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicCondition {
    pub condition: String,
    pub result: DeterministicConditionResult,
}

/// Construction failures; rejected condition text is preserved verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEvaluationCoreError {
    IdentifierLength {
        field: &'static str,
        character_count: usize,
    },
    PassedWithFailedCondition {
        index: usize,
        condition: String,
    },
    CompleteWithFailedLayer,
    InvalidIntegratedSha,
    NegativeContextEpoch {
        value: i64,
    },
}

impl std::fmt::Display for GoalEvaluationCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentifierLength {
                field,
                character_count,
            } => write!(
                f,
                "{field} has {character_count} Unicode scalar values; expected 1..=200"
            ),
            Self::PassedWithFailedCondition { index, condition } => write!(
                f,
                "deterministic success contradicts FAIL at conditions[{index}]: {condition:?}"
            ),
            Self::CompleteWithFailedLayer => {
                f.write_str("COMPLETE requires a passed deterministic layer")
            }
            Self::InvalidIntegratedSha => {
                f.write_str("integrated_sha must be exactly 40 lowercase hexadecimal characters")
            }
            Self::NegativeContextEpoch { value } => {
                write!(f, "context_epoch must be >= 0, got {value}")
            }
        }
    }
}

impl std::error::Error for GoalEvaluationCoreError {}

/// Immutable supplied deterministic layer. Conditions retain order and duplicates.
/// Missing conditions and an explicitly empty list remain distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicLayer {
    passed: bool,
    conditions: Option<Vec<DeterministicCondition>>,
}

impl DeterministicLayer {
    /// Rejects success contradicting any FAIL; reports the first failing entry.
    /// A false aggregate is retained even without listed failures: the optional
    /// list need not contain all required evidence. PASS/NOT_APPLICABLE entries
    /// never override a supplied false aggregate.
    pub fn try_new(
        passed: bool,
        conditions: Option<Vec<DeterministicCondition>>,
    ) -> Result<Self, GoalEvaluationCoreError> {
        if passed
            && let Some((index, condition)) = conditions.as_ref().and_then(|conditions| {
                conditions
                    .iter()
                    .enumerate()
                    .find(|(_, condition)| condition.result == DeterministicConditionResult::Fail)
            })
        {
            return Err(GoalEvaluationCoreError::PassedWithFailedCondition {
                index,
                condition: condition.condition.clone(),
            });
        }
        Ok(Self { passed, conditions })
    }

    pub fn passed(&self) -> bool {
        self.passed
    }

    pub fn conditions(&self) -> Option<&[DeterministicCondition]> {
        self.conditions.as_deref()
    }
}

/// Validated, non-temporal deterministic core, not a full completion decision.
///
/// State is supplied by an authoritative caller, never inferred from condition
/// text. COMPLETE requires deterministic success, but this necessary condition
/// does not prove semantic satisfaction. There is deliberately no `is_complete`
/// decision method or synthetic semantic result.
///
/// Validated fields cannot be changed afterwards:
/// ```compile_fail
/// use receipts_orchestration::goal::DeterministicGoalEvaluationCore;
/// fn invalidate(core: &mut DeterministicGoalEvaluationCore) {
///     core.context_epoch = Some(-1);
/// }
/// ```
/// Nor can a condition be changed through the read-only layer:
/// ```compile_fail
/// use receipts_orchestration::goal::{DeterministicLayer, DeterministicConditionResult};
/// fn invalidate(layer: &mut DeterministicLayer) {
///     layer.conditions().unwrap()[0].result = DeterministicConditionResult::Fail;
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicGoalEvaluationCore {
    evaluation_id: String,
    goal_id: String,
    project_id: Option<String>,
    state: GoalEvaluationState,
    deterministic_layer: DeterministicLayer,
    integrated_sha: Option<String>,
    context_epoch: Option<i64>,
}

impl DeterministicGoalEvaluationCore {
    /// Validates identifiers, SHA syntax, nonnegative epoch, then state/layer
    /// consistency. Stores all accepted input exactly, without reconciliation.
    pub fn try_new(
        evaluation_id: String,
        goal_id: String,
        project_id: Option<String>,
        state: GoalEvaluationState,
        deterministic_layer: DeterministicLayer,
        integrated_sha: Option<String>,
        context_epoch: Option<i64>,
    ) -> Result<Self, GoalEvaluationCoreError> {
        for (field, value) in [
            ("evaluation_id", Some(evaluation_id.as_str())),
            ("goal_id", Some(goal_id.as_str())),
            ("project_id", project_id.as_deref()),
        ] {
            if let Some(value) = value {
                let character_count = value.chars().count();
                if !(1..=200).contains(&character_count) {
                    return Err(GoalEvaluationCoreError::IdentifierLength {
                        field,
                        character_count,
                    });
                }
            }
        }
        if let Some(sha) = &integrated_sha
            && (sha.len() != 40 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        {
            return Err(GoalEvaluationCoreError::InvalidIntegratedSha);
        }
        if let Some(value) = context_epoch
            && value < 0
        {
            return Err(GoalEvaluationCoreError::NegativeContextEpoch { value });
        }
        if state == GoalEvaluationState::Complete && !deterministic_layer.passed() {
            return Err(GoalEvaluationCoreError::CompleteWithFailedLayer);
        }
        Ok(Self {
            evaluation_id,
            goal_id,
            project_id,
            state,
            deterministic_layer,
            integrated_sha,
            context_epoch,
        })
    }

    pub fn evaluation_id(&self) -> &str {
        &self.evaluation_id
    }

    pub fn goal_id(&self) -> &str {
        &self.goal_id
    }

    pub fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }

    /// The caller-supplied state; deterministic consistency is all this core
    /// validates. This accessor does not certify semantic completion.
    pub fn state(&self) -> GoalEvaluationState {
        self.state
    }

    pub fn deterministic_layer(&self) -> &DeterministicLayer {
        &self.deterministic_layer
    }

    pub fn integrated_sha(&self) -> Option<&str> {
        self.integrated_sha.as_deref()
    }

    pub fn context_epoch(&self) -> Option<i64> {
        self.context_epoch
    }
}

#[cfg(test)]
mod tests;
