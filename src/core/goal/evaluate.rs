//! Deterministic criterion execution. Semantic criteria remain explicit gaps.
use super::GoalEvaluationState;
use crate::capsules::{Criterion, CriterionKind};

/// Trusted runner executes the declared argv against the exact integrated SHA.
/// Missing evidence, timeout and execution errors must return false.
pub trait CriterionCheckPortV1 {
    fn check(&mut self, criterion: &Criterion, integrated_sha: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalAssessmentV1 {
    pub(super) state: GoalEvaluationState,
    pub(super) gaps: Vec<String>,
}

impl GoalAssessmentV1 {
    pub fn state(&self) -> GoalEvaluationState {
        self.state
    }
    pub fn gaps(&self) -> &[String] {
        &self.gaps
    }
    pub fn incomplete(gaps: Vec<String>) -> Self {
        Self {
            state: GoalEvaluationState::Incomplete,
            gaps,
        }
    }
}

/// Mechanical facts are derived by trusted composition from authoritative State.
#[derive(Debug, Clone, Default)]
pub struct GoalCompletionFactsV1 {
    pub required_work_integrated: bool,
    pub no_blocking_tasks_or_findings: bool,
    pub security_review_complete: bool,
    pub dependencies_resolved: bool,
    pub epoch_reconciled: bool,
    pub integrated_sha: Option<String>,
}

pub fn evaluate_required_criteria_v1(
    criteria: &[Criterion],
    facts: &GoalCompletionFactsV1,
    checks: &mut impl CriterionCheckPortV1,
) -> GoalAssessmentV1 {
    let mut gaps = Vec::new();
    for (ok, name) in [
        (
            facts.required_work_integrated,
            "required work not integrated",
        ),
        (
            facts.no_blocking_tasks_or_findings,
            "blocking tasks or findings",
        ),
        (
            facts.security_review_complete,
            "security review unavailable",
        ),
        (facts.dependencies_resolved, "unresolved dependencies"),
        (facts.epoch_reconciled, "epoch not reconciled"),
    ] {
        if !ok {
            gaps.push(name.into());
        }
    }
    let sha = facts.integrated_sha.as_deref().filter(|s| {
        s.len() == 40
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    if sha.is_none() {
        gaps.push("current integrated SHA unavailable".into());
    }
    if criteria.is_empty() {
        gaps.push("acceptance criteria unavailable".into());
    }
    let mut semantic = false;
    // Do not execute checks until the mechanical prerequisites hold.
    if gaps.is_empty() {
        for criterion in criteria {
            if criterion.kind() != CriterionKind::Deterministic {
                semantic = true;
                gaps.push(format!(
                    "{}: semantic verification required",
                    criterion.id()
                ));
            } else if criterion.check_command().is_none_or(<[String]>::is_empty)
                || !checks.check(criterion, sha.expect("validated SHA"))
            {
                gaps.push(format!("{}: declared check unverified", criterion.id()));
            }
        }
    }
    GoalAssessmentV1 {
        state: if gaps.is_empty() {
            GoalEvaluationState::Complete
        } else if semantic {
            GoalEvaluationState::HumanRequired
        } else {
            GoalEvaluationState::Incomplete
        },
        gaps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Checks {
        calls: usize,
        pass: bool,
    }
    impl CriterionCheckPortV1 for Checks {
        fn check(&mut self, _: &Criterion, _: &str) -> bool {
            self.calls += 1;
            self.pass
        }
    }
    fn facts() -> GoalCompletionFactsV1 {
        GoalCompletionFactsV1 {
            required_work_integrated: true,
            no_blocking_tasks_or_findings: true,
            security_review_complete: true,
            dependencies_resolved: true,
            epoch_reconciled: true,
            integrated_sha: Some("a".repeat(40)),
        }
    }
    fn criterion(kind: CriterionKind, command: Option<Vec<String>>) -> Criterion {
        Criterion::try_new("c1".into(), "required behavior".into(), kind, command, None).unwrap()
    }
    #[test]
    fn completion_requires_executed_declared_checks() {
        let criteria = [criterion(
            CriterionKind::Deterministic,
            Some(vec!["test".into()]),
        )];
        let mut checks = Checks {
            calls: 0,
            pass: true,
        };
        assert_eq!(
            evaluate_required_criteria_v1(&criteria, &facts(), &mut checks).state,
            GoalEvaluationState::Complete
        );
        assert_eq!(checks.calls, 1);
        checks.pass = false;
        assert_eq!(
            evaluate_required_criteria_v1(&criteria, &facts(), &mut checks).state,
            GoalEvaluationState::Incomplete
        );
    }
    #[test]
    fn empty_semantic_and_missing_evidence_never_complete() {
        let mut checks = Checks {
            calls: 0,
            pass: true,
        };
        assert_eq!(
            evaluate_required_criteria_v1(&[], &facts(), &mut checks).state,
            GoalEvaluationState::Incomplete
        );
        let semantic = [criterion(CriterionKind::Semantic, None)];
        assert_eq!(
            evaluate_required_criteria_v1(&semantic, &facts(), &mut checks).state,
            GoalEvaluationState::HumanRequired
        );
        let deterministic = [criterion(CriterionKind::Deterministic, None)];
        assert_eq!(
            evaluate_required_criteria_v1(&deterministic, &facts(), &mut checks).state,
            GoalEvaluationState::Incomplete
        );
        let mut missing = facts();
        missing.epoch_reconciled = false;
        assert_eq!(
            evaluate_required_criteria_v1(&semantic, &missing, &mut checks).state,
            GoalEvaluationState::Incomplete
        );
        assert_eq!(checks.calls, 0);
    }
}
