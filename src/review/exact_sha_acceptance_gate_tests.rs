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
    ready: bool,
    blocker: Option<A3HandoffBlocker>,
    blocking_categories: Vec<String>,
}

fn core(sha: &str) -> WorkspaceCheckpointExecutedCheckCore {
    executed(sha, Source::WorkerExecution, 0, Some(false))
}
fn executed(
    sha: &str,
    source: Source,
    exit_code: i64,
    timed_out: Option<bool>,
) -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        source,
        vec!["check".into()],
        exit_code,
        CommitSha::parse(sha).unwrap(),
        timed_out,
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
    categorized(A4ReviewFindingCategory::Style, blocking)
}
fn categorized(category: A4ReviewFindingCategory, blocking: bool) -> A4ReviewFinding {
    A4ReviewFinding::new(
        "finding".into(),
        A4ReviewFindingSeverity::Low,
        category,
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
            ready: true,
            blocker: None,
            blocking_categories: vec!["CONTRACT_VIOLATION".into()],
        }
    }
    // Appends one current check with an explicit PASS/FAIL/absent result.
    fn push_check(
        &mut self,
        record: AcceptanceEvidenceRecord,
        source: Source,
        exit_code: i64,
        timed_out: Option<bool>,
        passing: Option<bool>,
    ) {
        let check = executed(CURRENT, source, exit_code, timed_out);
        match record {
            A3Handoff => self.worker_checks.push(A3HandoffCheck::new(
                check,
                passing.map(|pass| {
                    if pass {
                        A3HandoffCheckResult::Pass
                    } else {
                        A3HandoffCheckResult::Fail
                    }
                }),
            )),
            ReviewCapsule => {
                self.capsule_checks
                    .get_or_insert_with(Vec::new)
                    .push(ReviewCapsuleCheck::new(
                        check,
                        passing.map(|pass| {
                            if pass {
                                ReviewCapsuleCheckResult::Pass
                            } else {
                                ReviewCapsuleCheckResult::Fail
                            }
                        }),
                    ))
            }
            A4Review => {
                let mut checks = self
                    .reproduction
                    .as_ref()
                    .and_then(|r| r.checks())
                    .unwrap_or_default()
                    .to_vec();
                checks.push(
                    A4ReviewReproductionCheck::new(
                        source,
                        vec!["check".into()],
                        exit_code,
                        CURRENT.into(),
                        timed_out,
                        None,
                        passing.map(|pass| {
                            if pass {
                                A4ReviewReproductionCheckResult::Pass
                            } else {
                                A4ReviewReproductionCheckResult::Fail
                            }
                        }),
                    )
                    .unwrap(),
                );
                self.reproduction = Some(reproduction(Some(true), Some(checks)));
            }
        }
    }
    fn clear_checks(&mut self, record: AcceptanceEvidenceRecord) {
        match record {
            A3Handoff => self.worker_checks.clear(),
            ReviewCapsule => self.capsule_checks = Some(vec![]),
            A4Review => self.reproduction = Some(reproduction(Some(true), Some(vec![]))),
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
            self.ready,
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
            self.blocker.clone(),
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
            ReviewCapsuleSeverityPolicy::new(self.blocking_categories.clone(), None).unwrap(),
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

const CHANNELS: [AcceptanceEvidenceRecord; 3] = [A3Handoff, ReviewCapsule, A4Review];
fn role(record: AcceptanceEvidenceRecord) -> Source {
    if record == A4Review {
        Source::ReviewExecution
    } else {
        Source::WorkerExecution
    }
}
// Reproduction is checked both as required and as optional supplied evidence.
fn channel_cases() -> Vec<(AcceptanceEvidenceRecord, bool)> {
    vec![
        (A3Handoff, true),
        (ReviewCapsule, true),
        (A4Review, true),
        (A4Review, false),
    ]
}
#[test]
fn timed_out_pass_after_good_check_fails_in_every_channel_regardless_of_exit() {
    for (record, required) in channel_cases() {
        for exit_code in [0, 7] {
            let mut evidence = Evidence::valid();
            evidence.reproduction_required = required;
            evidence.push_check(record, role(record), exit_code, Some(true), Some(true));
            assert_eq!(
                evidence.evaluate(),
                Err(Error::EvidenceTimedOut {
                    record,
                    check_index: 1
                }),
                "{record:?} required={required} exit={exit_code}"
            );
        }
    }
}
#[test]
fn wrong_source_role_after_good_check_fails_in_every_channel() {
    for (record, required) in channel_cases() {
        for source in Source::ALL.into_iter().filter(|s| *s != role(record)) {
            let mut evidence = Evidence::valid();
            evidence.reproduction_required = required;
            evidence.push_check(record, source, 0, Some(false), Some(true));
            assert_eq!(
                evidence.evaluate(),
                Err(Error::EvidenceSourceMismatch {
                    record,
                    check_index: 1
                }),
                "{record:?} required={required} {source:?}"
            );
        }
    }
}
#[test]
fn absent_or_false_timeout_needs_explicit_pass_and_nonzero_exit_is_not_judged() {
    for record in CHANNELS {
        for timed_out in [None, Some(false)] {
            for exit_code in [0, 7] {
                for passing in [Some(true), Some(false), None] {
                    let mut evidence = Evidence::valid();
                    evidence.clear_checks(record);
                    evidence.push_check(record, role(record), exit_code, timed_out, passing);
                    assert_result(evidence.evaluate(), record, passing);
                }
            }
        }
    }
}
#[test]
fn handoff_must_be_ready_without_text_blocker() {
    let mut evidence = Evidence::valid();
    evidence.ready = false;
    assert_eq!(evidence.evaluate(), Err(Error::HandoffNotReadyForReview));
    evidence.ready = true;
    for text in ["blocked", ""] {
        evidence.blocker = Some(A3HandoffBlocker::Text(text.into()));
        assert_eq!(evidence.evaluate(), Err(Error::HandoffBlocked), "{text:?}");
    }
    for blocker in [None, Some(A3HandoffBlocker::ExplicitNull)] {
        evidence.blocker = blocker;
        assert!(evidence.evaluate().is_ok());
        assert_eq!(evidence.handoff().blocker(), evidence.blocker.as_ref());
    }
}
#[test]
fn every_floor_and_negative_test_category_blocks_despite_false_flag_and_capsule_policy() {
    let mandatory: Vec<_> = A4ReviewFindingCategory::ALL
        .into_iter()
        .filter(|c| {
            is_assurance_profile_blocking_floor(*c)
                || *c == A4ReviewFindingCategory::MissingRequiredNegativeTest
        })
        .collect();
    assert_eq!(mandatory.len(), 9);
    for policy in ["CONTRACT_VIOLATION", "UNKNOWN_POLICY_CATEGORY"] {
        for category in mandatory.iter().copied() {
            for verdict in [
                A4ReviewVerdict::PassWithNonblockingFindings,
                A4ReviewVerdict::Pass,
            ] {
                let mut evidence = Evidence::valid();
                evidence.blocking_categories = vec![policy.into()];
                evidence.verdict = verdict;
                evidence.nonblocking.push(categorized(category, false));
                assert_eq!(
                    evidence.evaluate(),
                    Err(Error::BlockingFindingsPresent),
                    "{policy} {category:?} {verdict:?}"
                );
            }
        }
    }
}
#[test]
fn capsule_blocking_category_requires_exact_string_and_preserves_unknowns() {
    let ordinary = A4ReviewFindingCategory::ALL.into_iter().filter(|c| {
        !is_assurance_profile_blocking_floor(*c)
            && *c != A4ReviewFindingCategory::MissingRequiredNegativeTest
    });
    for category in ordinary {
        let name = category.as_str();
        let mut evidence = Evidence::valid();
        evidence.verdict = A4ReviewVerdict::PassWithNonblockingFindings;
        evidence.nonblocking.push(categorized(category, false));
        assert!(evidence.evaluate().is_ok(), "{name}");
        evidence.blocking_categories = vec!["UNKNOWN_POLICY_CATEGORY".into(), name.into()];
        assert_eq!(
            evidence.evaluate(),
            Err(Error::BlockingFindingsPresent),
            "{name}"
        );
        for near in [name.to_lowercase(), format!("{name} "), format!(" {name}")] {
            evidence.blocking_categories = vec!["UNKNOWN_POLICY_CATEGORY".into(), near.clone()];
            assert!(evidence.evaluate().is_ok(), "{near:?}");
            assert_eq!(
                evidence.capsule().severity_policy().blocking_categories(),
                ["UNKNOWN_POLICY_CATEGORY".to_string(), near]
            );
        }
    }
}
#[test]
fn pass_with_any_finding_is_inconsistent_but_matching_nonblocking_verdict_passes() {
    let mut evidence = Evidence::valid();
    evidence.nonblocking.push(finding(false));
    assert_eq!(evidence.evaluate(), Err(Error::VerdictFindingsMismatch));
    evidence.verdict = A4ReviewVerdict::PassWithNonblockingFindings;
    assert!(evidence.evaluate().is_ok());
    evidence.nonblocking.clear();
    assert!(evidence.evaluate().is_ok());
}
#[test]
fn blocking_flag_outside_blocking_array_fails_under_both_passing_verdicts() {
    for verdict in [
        A4ReviewVerdict::Pass,
        A4ReviewVerdict::PassWithNonblockingFindings,
    ] {
        let mut evidence = Evidence::valid();
        evidence.verdict = verdict;
        evidence.nonblocking.push(finding(false));
        evidence.nonblocking.push(finding(true));
        assert_eq!(evidence.evaluate(), Err(Error::BlockingFindingsPresent));
    }
}
#[test]
fn new_vetoes_judge_only_the_current_pair_of_repair_history() {
    let mut old = Evidence::valid();
    old.final_sha = OLD;
    old.reviewed = OLD;
    old.verdict = A4ReviewVerdict::Reject;
    old.review_id = "old-review";
    old.ready = false;
    old.blocker = Some(A3HandoffBlocker::Text("blocked".into()));
    old.worker_checks = vec![A3HandoffCheck::new(
        executed(OLD, Source::ReviewExecution, 1, Some(true)),
        Some(A3HandoffCheckResult::Pass),
    )];
    old.nonblocking.push(categorized(
        A4ReviewFindingCategory::MissingRequiredNegativeTest,
        false,
    ));
    let current = Evidence::valid();
    let handoffs = [old.handoff(), current.handoff()];
    let reviews = [old.review(), current.review()];
    let result = evaluate(
        &handoffs,
        &reviews,
        Some(&current.capsule()),
        Some(CURRENT),
        Some(BASE),
        &[],
        1,
    );
    assert_eq!(result, current.evaluate());
    assert!(result.is_ok());
}
#[test]
fn presence_and_repair_errors_precede_new_vetoes() {
    let mut evidence = Evidence::valid();
    evidence.ready = false;
    evidence.verdict = A4ReviewVerdict::Reject;
    evidence.nonblocking.push(finding(false));
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RepairCycleNotClear(
            RepairCycleDisposition::RepairRequired
        ))
    );
    evidence.independent = false;
    assert_eq!(
        evidence.evaluate(),
        Err(Error::RepairHistory(
            RepairCycleControlError::IndependenceNotAttested { attempt_index: 0 }
        ))
    );
    assert_eq!(
        evaluate(
            &[evidence.handoff()],
            &[],
            Some(&evidence.capsule()),
            Some(CURRENT),
            Some(BASE),
            &[],
            0
        ),
        Err(Error::MissingA4Review)
    );
}
