use crate::*;
use AcceptanceEvidenceRecord::{A3Handoff, A4Review, ReviewCapsule};
use ExactShaAcceptanceGateError as Error;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource as Source, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};

const BASE: &str = "ffffffffffffffffffffffffffffffffffffffff";
const CURRENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// Mutable test data is rebuilt through the public immutable record constructors.
struct Evidence {
    handoff_task: &'static str,
    review_task: &'static str,
    capsule_task: &'static str,
    handoff_attempt: &'static str,
    capsule_attempt: &'static str,
    review_id: &'static str,
    capsule_review: &'static str,
    start: &'static str,
    baseline: &'static str,
    final_sha: &'static str,
    implementation: &'static str,
    reviewed: &'static str,
    worker_checks: Vec<A3HandoffCheck>,
    capsule_checks: Option<Vec<ReviewCapsuleCheck>>,
    reproduction: Option<A4ReviewReproduction>,
    reproduction_required: bool,
    verdict: A4ReviewVerdict,
    independent: bool,
    blocking: Vec<A4ReviewFinding>,
    nonblocking: Vec<A4ReviewFinding>,
    writes: Option<Vec<String>>,
}

fn core(sha: &str) -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        Source::WorkerExecution,
        vec!["check".into()],
        0,
        CommitSha::parse(sha).unwrap(),
        Some(false),
        None,
    )
    .unwrap()
}
fn reproduced(
    sha: &str,
    result: Option<A4ReviewReproductionCheckResult>,
) -> A4ReviewReproductionCheck {
    A4ReviewReproductionCheck::new(
        Source::ReviewExecution,
        vec!["check".into()],
        0,
        sha.into(),
        Some(false),
        None,
        result,
    )
    .unwrap()
}
fn reproduction(
    performed: Option<bool>,
    checks: Option<Vec<A4ReviewReproductionCheck>>,
) -> A4ReviewReproduction {
    A4ReviewReproduction::new(performed, checks, A4ReviewReproductionLimitation::Omitted)
}
fn finding(blocking: bool) -> A4ReviewFinding {
    A4ReviewFinding::new(
        "finding".into(),
        A4ReviewFindingSeverity::Low,
        A4ReviewFindingCategory::Style,
        "resolved according to prose".into(),
        blocking,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}
impl Evidence {
    fn valid() -> Self {
        Self {
            handoff_task: "task",
            review_task: "task",
            capsule_task: "task",
            handoff_attempt: "attempt",
            capsule_attempt: "attempt",
            review_id: "review",
            capsule_review: "review",
            start: BASE,
            baseline: BASE,
            final_sha: CURRENT,
            implementation: CURRENT,
            reviewed: CURRENT,
            worker_checks: vec![A3HandoffCheck::new(
                core(CURRENT),
                Some(A3HandoffCheckResult::Pass),
            )],
            capsule_checks: Some(vec![ReviewCapsuleCheck::new(
                core(CURRENT),
                Some(ReviewCapsuleCheckResult::Pass),
            )]),
            reproduction: Some(reproduction(
                Some(true),
                Some(vec![reproduced(
                    CURRENT,
                    Some(A4ReviewReproductionCheckResult::Pass),
                )]),
            )),
            reproduction_required: true,
            verdict: A4ReviewVerdict::Pass,
            independent: true,
            blocking: vec![],
            nonblocking: vec![],
            writes: Some(vec![]),
        }
    }
    fn handoff(&self) -> A3HandoffNonTemporalCore {
        A3HandoffNonTemporalCore::new(
            self.handoff_task.into(),
            self.handoff_attempt.into(),
            self.start.into(),
            self.final_sha.into(),
            "build/review-integration-a3-007-exact-sha-staleness-gate".into(),
            vec!["src/review/example.rs".into()],
            self.worker_checks.clone(),
            true,
            None,
            None,
            None,
            None,
            Some("reviewed exact SHA".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    }
    fn review(&self) -> A4ReviewNonTemporalCore {
        A4ReviewNonTemporalCore::new(
            self.review_id.into(),
            self.review_task.into(),
            self.reviewed.into(),
            self.verdict,
            self.independent,
            self.blocking.clone(),
            self.nonblocking.clone(),
            vec![A4ReviewDimensionReview::new(
                A4ReviewDimension::Correctness,
                A4ReviewDimensionAssessment::Satisfied,
                None,
            )],
            None,
            self.reproduction.clone(),
            self.writes.clone(),
            None,
            Some("reviewed exact SHA".into()),
        )
        .unwrap()
    }
    fn capsule(&self) -> ReviewCapsuleNonTemporalCore {
        ReviewCapsuleNonTemporalCore::new(
            self.capsule_review.into(),
            self.capsule_task.into(),
            self.capsule_attempt.into(),
            self.baseline.into(),
            self.implementation.into(),
            "objective".into(),
            vec![
                ReviewCapsuleCriterion::new(
                    "criterion".into(),
                    "check".into(),
                    ReviewCapsuleCriterionKind::Deterministic,
                    Some(vec!["check".into()]),
                    None,
                )
                .unwrap(),
            ],
            None,
            None,
            None,
            WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::ArtifactId, "diff", None, None)
                .unwrap(),
            vec!["src/review/**".into()],
            self.capsule_checks.clone(),
            None,
            ReviewCapsuleReviewScope::Full,
            ReviewCapsuleSeverityPolicy::new(vec!["CONTRACT_VIOLATION".into()], None).unwrap(),
            self.reproduction_required,
            None,
            0,
        )
        .unwrap()
    }
    fn evaluate(&self) -> Result<ExactShaAcceptanceGatePass, Error> {
        evaluate(
            &[self.handoff()],
            &[self.review()],
            Some(&self.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            0,
        )
    }
}
fn evaluate(
    handoffs: &[A3HandoffNonTemporalCore],
    reviews: &[A4ReviewNonTemporalCore],
    capsule: Option<&ReviewCapsuleNonTemporalCore>,
    candidate: Option<&str>,
    baseline: Option<&str>,
    dependencies: &[DependencyShaFreshnessLink],
    repairs: usize,
) -> Result<ExactShaAcceptanceGatePass, Error> {
    ExactShaAcceptanceGate::evaluate(ExactShaAcceptanceGateInput {
        handoffs,
        reviews,
        capsule,
        accepted_candidate_sha: candidate,
        current_baseline_sha: baseline,
        dependency_freshness: dependencies,
        completed_repair_attempts: repairs,
    })
}

#[test]
fn complete_exact_evidence_passes_deterministically() {
    let evidence = Evidence::valid();
    let expected = evidence.evaluate().unwrap();
    assert_eq!(expected.candidate_sha().as_str(), CURRENT);
    for _ in 0..100 {
        assert_eq!(evidence.evaluate(), Ok(expected.clone()));
    }
}
#[test]
fn nonblocking_verdict_and_finding_pass() {
    let mut evidence = Evidence::valid();
    evidence.verdict = A4ReviewVerdict::PassWithNonblockingFindings;
    evidence.nonblocking.push(finding(false));
    assert!(evidence.evaluate().is_ok());
}
#[test]
fn fresh_repair_history_passes_only_with_current_pair_and_capsule() {
    let mut old = Evidence::valid();
    old.final_sha = OLD;
    old.reviewed = OLD;
    old.verdict = A4ReviewVerdict::Reject;
    old.handoff_attempt = "initial";
    old.capsule_attempt = "initial";
    old.implementation = OLD;
    old.review_id = "old-review";
    old.capsule_review = "old-review";
    old.blocking.push(finding(true));
    let current = Evidence::valid();
    let handoffs = [old.handoff(), current.handoff()];
    let reviews = [old.review(), current.review()];
    let snapshot = (handoffs.clone(), reviews.clone());
    assert_eq!(
        evaluate(
            &handoffs,
            &reviews,
            Some(&current.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            1
        ),
        current.evaluate()
    );
    assert_eq!((handoffs.clone(), reviews.clone()), snapshot);
    assert_eq!(
        evaluate(
            &handoffs,
            &reviews,
            Some(&old.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            1
        ),
        Err(Error::AttemptIdMismatch)
    );
}
#[test]
fn a4_sha_mismatch_is_rejected_by_reused_history_layer() {
    let mut evidence = Evidence::valid();
    evidence.reviewed = OLD;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RepairHistory(RepairCycleControlError::ShaMismatch {
            attempt_index: 0
        }))
    );
}
#[test]
fn handoff_final_sha_mismatch_is_rejected() {
    let mut evidence = Evidence::valid();
    evidence.final_sha = OLD;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RepairHistory(RepairCycleControlError::ShaMismatch {
            attempt_index: 0
        }))
    );
}
#[test]
fn capsule_implementation_sha_mismatch_is_rejected() {
    let mut evidence = Evidence::valid();
    evidence.implementation = OLD;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::CandidateShaMismatch(ReviewCapsule))
    );
}
#[test]
fn post_review_advance_cannot_be_rescued_by_branch_or_prose() {
    let mut evidence = Evidence::valid();
    evidence.final_sha = OLD;
    evidence.reviewed = OLD;
    evidence.implementation = OLD;
    assert_eq!(
        evidence.handoff().branch(),
        "build/review-integration-a3-007-exact-sha-staleness-gate"
    );
    assert_eq!(evidence.review().rationale(), Some("reviewed exact SHA"));
    assert_eq!(
        evidence.evaluate(),
        Err(Error::CandidateShaMismatch(A3Handoff))
    );
}
#[test]
fn whole_tree_baseline_advance_is_stale_even_when_candidate_is_exact() {
    let evidence = Evidence::valid();
    assert_eq!(
        evaluate(
            &[evidence.handoff()],
            &[evidence.review()],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(OLD),
            &[],
            0
        ),
        Err(Error::BaselineShaMismatch(A3Handoff))
    );
}
#[test]
fn handoff_start_mismatch_is_stale() {
    let mut evidence = Evidence::valid();
    evidence.start = OLD;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::BaselineShaMismatch(A3Handoff))
    );
}
#[test]
fn capsule_baseline_mismatch_is_stale() {
    let mut evidence = Evidence::valid();
    evidence.baseline = OLD;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::BaselineShaMismatch(ReviewCapsule))
    );
}
#[test]
fn missing_records_and_caller_identities_fail_closed() {
    let evidence = Evidence::valid();
    let h = [evidence.handoff()];
    let r = [evidence.review()];
    let c = evidence.capsule();
    assert_eq!(
        evaluate(&[], &r, Some(&c), Some(CURRENT), Some(BASE), &[], 0),
        Err(Error::MissingA3Handoff)
    );
    assert_eq!(
        evaluate(&h, &[], Some(&c), Some(CURRENT), Some(BASE), &[], 0),
        Err(Error::MissingA4Review)
    );
    assert_eq!(
        evaluate(&h, &r, None, Some(CURRENT), Some(BASE), &[], 0),
        Err(Error::MissingReviewCapsule)
    );
    assert_eq!(
        evaluate(&h, &r, Some(&c), None, Some(BASE), &[], 0),
        Err(Error::MissingCandidateSha)
    );
    assert_eq!(
        evaluate(&h, &r, Some(&c), Some(CURRENT), None, &[], 0),
        Err(Error::MissingCurrentBaselineSha)
    );
}
#[test]
fn false_independence_is_rejected_by_history_validation() {
    let mut evidence = Evidence::valid();
    evidence.independent = false;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RepairHistory(
            RepairCycleControlError::IndependenceNotAttested { attempt_index: 0 }
        ))
    );
}
#[test]
fn reject_repair_required_and_bound_exhausted_never_pass() {
    let mut evidence = Evidence::valid();
    evidence.verdict = A4ReviewVerdict::Reject;
    for repairs in 0..=3 {
        let disposition = if repairs == 3 {
            RepairCycleDisposition::RepairBoundExhausted
        } else {
            RepairCycleDisposition::RepairRequired
        };
        assert_eq!(
            evaluate(
                &vec![evidence.handoff(); repairs + 1],
                &vec![evidence.review(); repairs + 1],
                Some(&evidence.capsule()),
                Some(CURRENT),
                Some(BASE),
                &[],
                repairs
            ),
            Err(Error::RepairCycleNotClear(disposition))
        );
    }
}
#[test]
fn invalid_history_is_not_bypassed_by_a_current_pass() {
    let evidence = Evidence::valid();
    assert_eq!(
        evaluate(
            &[evidence.handoff(), evidence.handoff()],
            &[evidence.review(), evidence.review()],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            1
        ),
        Err(Error::RepairHistory(
            RepairCycleControlError::PriorAttemptWasNotRejected { attempt_index: 0 }
        ))
    );
    assert_eq!(
        evaluate(
            &[evidence.handoff()],
            &[evidence.review()],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            1
        ),
        Err(Error::RepairHistory(
            RepairCycleControlError::RepairAttemptCountMismatch
        ))
    );
}
#[test]
fn blocking_array_rejects_both_passing_verdicts_even_with_false_flags() {
    for verdict in [
        A4ReviewVerdict::Pass,
        A4ReviewVerdict::PassWithNonblockingFindings,
    ] {
        for blocking in [false, true] {
            let mut evidence = Evidence::valid();
            evidence.verdict = verdict;
            evidence.blocking.push(finding(blocking));
            assert_eq!(evidence.evaluate(), Err(Error::BlockingFindingsPresent));
        }
    }
}
#[test]
fn blocking_flag_outside_blocking_array_fails() {
    let mut evidence = Evidence::valid();
    evidence.nonblocking.push(finding(true));
    assert_eq!(evidence.evaluate(), Err(Error::BlockingFindingsPresent));
}
#[test]
fn write_scope_requires_explicit_empty_evidence() {
    let mut evidence = Evidence::valid();
    evidence.writes = None;
    assert_eq!(evidence.evaluate(), Err(Error::WriteScopeEvidenceMissing));
    evidence.writes = Some(vec!["src/core/unauthorized.rs".into()]);
    assert_eq!(
        evidence.evaluate(),
        Err(Error::UnauthorizedFileChangesPresent)
    );
    evidence.writes = Some(vec![]);
    assert!(evidence.evaluate().is_ok());
}
#[test]
fn stale_worker_check_after_fresh_check_fails() {
    let mut evidence = Evidence::valid();
    evidence.worker_checks.push(A3HandoffCheck::new(
        core(OLD),
        Some(A3HandoffCheckResult::Pass),
    ));
    assert_eq!(
        evidence.evaluate(),
        Err(Error::EvidenceShaMismatch {
            record: A3Handoff,
            check_index: 1
        })
    );
}
#[test]
fn stale_capsule_check_after_fresh_check_fails() {
    let mut evidence = Evidence::valid();
    evidence
        .capsule_checks
        .as_mut()
        .unwrap()
        .push(ReviewCapsuleCheck::new(
            core(OLD),
            Some(ReviewCapsuleCheckResult::Pass),
        ));
    assert_eq!(
        evidence.evaluate(),
        Err(Error::EvidenceShaMismatch {
            record: ReviewCapsule,
            check_index: 1
        })
    );
}
#[test]
fn stale_reproduction_check_fails_even_if_optional() {
    for required in [true, false] {
        let mut evidence = Evidence::valid();
        evidence.reproduction_required = required;
        evidence.reproduction = Some(reproduction(
            Some(true),
            Some(vec![
                reproduced(CURRENT, Some(A4ReviewReproductionCheckResult::Pass)),
                reproduced(OLD, Some(A4ReviewReproductionCheckResult::Pass)),
            ]),
        ));
        assert_eq!(
            evidence.evaluate(),
            Err(Error::EvidenceShaMismatch {
                record: A4Review,
                check_index: 1
            })
        );
    }
}
#[test]
fn every_nonpassing_worker_result_and_absent_result_fails_despite_exit_zero() {
    for result in A3HandoffCheckResult::ALL
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let mut evidence = Evidence::valid();
        evidence.worker_checks[0] = A3HandoffCheck::new(core(CURRENT), result);
        assert_result(
            evidence.evaluate(),
            A3Handoff,
            result.map(|r| r == A3HandoffCheckResult::Pass),
        );
    }
}
#[test]
fn every_nonpassing_capsule_result_and_absent_result_fails_despite_exit_zero() {
    for result in ReviewCapsuleCheckResult::ALL
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let mut evidence = Evidence::valid();
        evidence.capsule_checks = Some(vec![ReviewCapsuleCheck::new(core(CURRENT), result)]);
        assert_result(
            evidence.evaluate(),
            ReviewCapsule,
            result.map(|r| r == ReviewCapsuleCheckResult::Pass),
        );
    }
}
#[test]
fn every_nonpassing_reproduction_result_and_absent_result_fails_despite_exit_zero() {
    for result in A4ReviewReproductionCheckResult::ALL
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let mut evidence = Evidence::valid();
        evidence.reproduction = Some(reproduction(
            Some(true),
            Some(vec![reproduced(CURRENT, result)]),
        ));
        assert_result(
            evidence.evaluate(),
            A4Review,
            result.map(|r| r == A4ReviewReproductionCheckResult::Pass),
        );
    }
}
fn assert_result(
    actual: Result<ExactShaAcceptanceGatePass, Error>,
    record: AcceptanceEvidenceRecord,
    passing: Option<bool>,
) {
    match passing {
        Some(true) => assert!(actual.is_ok()),
        Some(false) => assert_eq!(
            actual,
            Err(Error::EvidenceResultNotPassing {
                record,
                check_index: 0
            })
        ),
        None => assert_eq!(
            actual,
            Err(Error::EvidenceResultMissing {
                record,
                check_index: 0
            })
        ),
    }
}
#[test]
fn required_worker_checks_cannot_be_empty() {
    let mut evidence = Evidence::valid();
    evidence.worker_checks.clear();
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RequiredChecksMissing(A3Handoff))
    );
}
#[test]
fn capsule_checks_are_optional_but_every_supplied_record_is_required_support() {
    for checks in [None, Some(vec![])] {
        let mut evidence = Evidence::valid();
        evidence.capsule_checks = checks;
        assert!(evidence.evaluate().is_ok());
    }
}
#[test]
fn required_reproduction_cannot_be_absent() {
    let mut evidence = Evidence::valid();
    evidence.reproduction = None;
    assert_eq!(evidence.evaluate(), Err(Error::RequiredReproductionMissing));
    evidence.reproduction_required = false;
    assert!(evidence.evaluate().is_ok());
}
#[test]
fn required_reproduction_needs_performed_true() {
    for performed in [None, Some(false)] {
        let mut evidence = Evidence::valid();
        evidence.reproduction = Some(reproduction(
            performed,
            Some(vec![reproduced(
                CURRENT,
                Some(A4ReviewReproductionCheckResult::Pass),
            )]),
        ));
        assert_eq!(evidence.evaluate(), Err(Error::ReproductionNotPerformed));
        evidence.reproduction_required = false;
        assert_eq!(evidence.evaluate(), Err(Error::ReproductionNotPerformed));
    }
}
#[test]
fn required_reproduction_needs_nonempty_checks() {
    for checks in [None, Some(vec![])] {
        let mut evidence = Evidence::valid();
        evidence.reproduction = Some(reproduction(Some(true), checks));
        assert_eq!(
            evidence.evaluate(),
            Err(Error::RequiredChecksMissing(A4Review))
        );
    }
}
#[test]
fn handoff_task_mismatch_fails() {
    let mut evidence = Evidence::valid();
    evidence.handoff_task = "other";
    assert_eq!(evidence.evaluate(), Err(Error::TaskIdMismatch(A3Handoff)));
}
#[test]
fn review_task_mismatch_fails() {
    let mut evidence = Evidence::valid();
    evidence.review_task = "other";
    assert_eq!(evidence.evaluate(), Err(Error::TaskIdMismatch(A4Review)));
}
#[test]
fn capsule_task_mismatch_fails() {
    let mut evidence = Evidence::valid();
    evidence.capsule_task = "other";
    assert_eq!(evidence.evaluate(), Err(Error::TaskIdMismatch(A3Handoff)));
}
#[test]
fn attempt_mismatch_fails() {
    let mut evidence = Evidence::valid();
    evidence.capsule_attempt = "attempt ";
    assert_eq!(evidence.evaluate(), Err(Error::AttemptIdMismatch));
}
#[test]
fn review_id_mismatch_fails() {
    let mut evidence = Evidence::valid();
    evidence.capsule_review = "review ";
    assert_eq!(evidence.evaluate(), Err(Error::ReviewIdMismatch));
}
#[test]
fn dependency_links_are_exact_and_all_supplied_links_are_checked() {
    let evidence = Evidence::valid();
    let mut links = vec![DependencyShaFreshnessLink {
        evidence_whole_tree_sha: CommitSha::parse(OLD).unwrap(),
        current_authoritative_sha: CommitSha::parse(OLD).unwrap(),
    }];
    assert!(
        evaluate(
            &[evidence.handoff()],
            &[evidence.review()],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(BASE),
            &links,
            0
        )
        .is_ok()
    );
    links.push(DependencyShaFreshnessLink {
        evidence_whole_tree_sha: CommitSha::parse(OLD).unwrap(),
        current_authoritative_sha: CommitSha::parse(CURRENT).unwrap(),
    });
    assert_eq!(
        evaluate(
            &[evidence.handoff()],
            &[evidence.review()],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(BASE),
            &links,
            0
        ),
        Err(Error::DependencyEvidenceStale { link_index: 1 })
    );
}
fn malformed_shas() -> Vec<String> {
    vec![
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        format!(" {CURRENT}"),
        format!("{CURRENT} "),
        "HEAD".into(),
        "main".into(),
        "refs/heads/main".into(),
        "abcdef0".into(),
        "ａ".repeat(40),
    ]
}
#[test]
fn malformed_candidate_inputs_fail_without_normalization() {
    let evidence = Evidence::valid();
    for sha in malformed_shas() {
        assert_eq!(
            evaluate(
                &[evidence.handoff()],
                &[evidence.review()],
                Some(&evidence.capsule()),
                Some(&sha),
                Some(BASE),
                &[],
                0
            ),
            Err(Error::MalformedCandidateSha),
            "{sha:?}"
        );
    }
}
#[test]
fn malformed_baseline_inputs_fail_without_normalization() {
    let evidence = Evidence::valid();
    for sha in malformed_shas() {
        assert_eq!(
            evaluate(
                &[evidence.handoff()],
                &[evidence.review()],
                Some(&evidence.capsule()),
                Some(CURRENT),
                Some(&sha),
                &[],
                0
            ),
            Err(Error::MalformedCurrentBaselineSha),
            "{sha:?}"
        );
    }
}
