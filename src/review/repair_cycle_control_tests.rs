use crate::*;
use A4ReviewVerdict::{Pass, PassWithNonblockingFindings, Reject};
use DeterministicRepairCycleControlCore as Control;
use RepairCycleControlError as Error;
use RepairCycleDisposition::{NoRepairRequired, RepairBoundExhausted, RepairRequired};

fn handoff(sha: &str, ready: bool) -> A3HandoffNonTemporalCore {
    A3HandoffNonTemporalCore::new(
        "opaque-task".into(),
        "not-an-ordinal-R999".into(),
        "f".repeat(40),
        sha.into(),
        "not-a-real-branch/HEAD".into(),
        vec!["src/review/example.rs".into()],
        vec![],
        ready,
        None,
        None,
        None,
        None,
        Some(format!("implementation {sha}")),
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

fn finding(id: &str, blocking: bool) -> A4ReviewFinding {
    A4ReviewFinding::new(
        id.into(),
        A4ReviewFindingSeverity::Medium,
        A4ReviewFindingCategory::Correctness,
        format!("evidence for {id}"),
        blocking,
        Some("src/review/example.rs".into()),
        None,
        None,
        Some(A4ReviewFindingConfidence::High),
        Some(A4ReviewFindingSource::LlmReview),
    )
    .unwrap()
}

fn review(
    sha: &str,
    verdict: A4ReviewVerdict,
    independent: bool,
    blocking: Vec<A4ReviewFinding>,
) -> A4ReviewNonTemporalCore {
    A4ReviewNonTemporalCore::new(
        format!("review-{sha}-{}", verdict.as_str()),
        "opaque-task".into(),
        sha.into(),
        verdict,
        independent,
        blocking,
        vec![finding(&format!("nonblocking-{sha}"), false)],
        vec![A4ReviewDimensionReview::new(
            A4ReviewDimension::Correctness,
            A4ReviewDimensionAssessment::Satisfied,
            Some("recorded dimension".into()),
        )],
        None,
        None,
        None,
        None,
        Some(format!("rationale for {sha}")),
    )
    .unwrap()
}

fn history(
    count: usize,
    current_verdict: A4ReviewVerdict,
) -> (Vec<A3HandoffNonTemporalCore>, Vec<A4ReviewNonTemporalCore>) {
    // Deliberately non-sorted SHAs: sorting would destroy attempt order.
    let shas = ["d", "b", "a", "c", "e"];
    let mut handoffs = vec![];
    let mut reviews = vec![];
    for (index, digit) in shas.iter().take(count).enumerate() {
        let sha = digit.repeat(40);
        let verdict = if index + 1 == count {
            current_verdict
        } else {
            Reject
        };
        handoffs.push(handoff(&sha, true));
        let blocking = if verdict == Reject {
            vec![finding(&format!("rejected-{index}"), true)]
        } else {
            vec![]
        };
        reviews.push(review(&sha, verdict, true, blocking));
    }
    (handoffs, reviews)
}

fn assert_control(
    count: usize,
    verdict: A4ReviewVerdict,
    disposition: RepairCycleDisposition,
    next: Option<usize>,
) {
    let (handoffs, reviews) = history(count + 1, verdict);
    let result = Control::evaluate(&handoffs, &reviews, count).unwrap();
    assert_eq!(result.disposition(), disposition);
    assert_eq!(result.next_repair_ordinal(), next);
}

#[test]
fn initial_pass_requires_no_repair() {
    assert_control(0, Pass, NoRepairRequired, None);
}

#[test]
fn nonblocking_pass_requires_no_repair() {
    assert_control(0, PassWithNonblockingFindings, NoRepairRequired, None);
}

#[test]
fn initial_reject_requests_r1() {
    assert_eq!(DEFAULT_MAX_REPAIR_ATTEMPTS, 3);
    assert_control(0, Reject, RepairRequired, Some(1));
}

#[test]
fn r1_reject_requests_r2() {
    assert_control(1, Reject, RepairRequired, Some(2));
}

#[test]
fn r2_reject_requests_r3() {
    assert_control(2, Reject, RepairRequired, Some(3));
}

#[test]
fn r3_reject_exhausts_bound() {
    assert_control(3, Reject, RepairBoundExhausted, None);
}

#[test]
fn r3_passing_verdicts_require_no_repair() {
    for verdict in [Pass, PassWithNonblockingFindings] {
        assert_control(3, verdict, NoRepairRequired, None);
    }
}

#[test]
fn completed_r4_is_invalid_even_with_pass() {
    for verdict in A4ReviewVerdict::ALL {
        let (handoffs, reviews) = history(5, verdict);
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 4),
            Err(Error::RepairAttemptBoundExceeded)
        );
    }
}

#[test]
fn counts_above_three_fail_without_overflow_or_clamping() {
    let (handoffs, reviews) = history(4, Reject);
    for count in [4, 5, 100, usize::MAX] {
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, count),
            Err(Error::RepairAttemptBoundExceeded)
        );
    }
}

#[test]
fn current_sha_mismatch_voids_every_verdict() {
    for verdict in A4ReviewVerdict::ALL {
        let (handoffs, mut reviews) = history(1, verdict);
        // One differing character defeats exact equality, despite a 39-byte prefix.
        let sha = format!("{}e", "d".repeat(39));
        reviews[0] = review(&sha, verdict, true, vec![]);
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 0),
            Err(Error::ShaMismatch { attempt_index: 0 })
        );
    }
}

#[test]
fn every_prior_sha_is_checked() {
    for index in 0..3 {
        let (handoffs, mut reviews) = history(4, Pass);
        reviews[index] = review(&"e".repeat(40), Reject, true, vec![]);
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 3),
            Err(Error::ShaMismatch {
                attempt_index: index
            })
        );
    }
}

#[test]
fn current_false_independence_voids_every_verdict_at_every_count() {
    for count in 0..=3 {
        for verdict in A4ReviewVerdict::ALL {
            let (handoffs, mut reviews) = history(count + 1, verdict);
            reviews[count] = review(handoffs[count].final_sha().as_str(), verdict, false, vec![]);
            assert_eq!(
                Control::evaluate(&handoffs, &reviews, count),
                Err(Error::IndependenceNotAttested {
                    attempt_index: count
                })
            );
        }
    }
}

#[test]
fn every_prior_independence_attestation_is_checked() {
    for index in 0..3 {
        let (handoffs, mut reviews) = history(4, Pass);
        reviews[index] = review(handoffs[index].final_sha().as_str(), Reject, false, vec![]);
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 3),
            Err(Error::IndependenceNotAttested {
                attempt_index: index
            })
        );
    }
}

#[test]
fn stale_pass_cannot_authorize_repaired_sha() {
    let (handoffs, mut reviews) = history(2, Pass);
    reviews[1] = review(handoffs[0].final_sha().as_str(), Pass, true, vec![]);
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 1),
        Err(Error::ShaMismatch { attempt_index: 1 })
    );
}

#[test]
fn stale_reject_cannot_request_another_repair() {
    let (handoffs, mut reviews) = history(2, Reject);
    reviews[1] = reviews[0].clone();
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 1),
        Err(Error::ShaMismatch { attempt_index: 1 })
    );
}

#[test]
fn fresh_exact_review_of_repaired_sha_controls_current_result() {
    for count in 1..=3 {
        for verdict in [Pass, PassWithNonblockingFindings] {
            assert_control(count, verdict, NoRepairRequired, None);
        }
    }
}

fn assert_history_preserved(count: usize) {
    let (handoffs, reviews) = history(count + 1, Reject);
    let before_handoffs = handoffs.clone();
    let before_reviews = reviews.clone();
    let result = Control::evaluate(&handoffs, &reviews, count).unwrap();
    assert_eq!(handoffs, before_handoffs);
    assert_eq!(reviews, before_reviews);
    assert_eq!(reviews.len(), count + 1);
    for (index, (handoff, review)) in handoffs.iter().zip(&reviews).enumerate() {
        assert_eq!(handoff.final_sha(), before_handoffs[index].final_sha());
        assert_eq!(review, &before_reviews[index]);
        assert_eq!(
            review.blocking_findings()[0].finding_id(),
            format!("rejected-{index}")
        );
    }
    assert_eq!(
        result.next_repair_ordinal(),
        if count < 3 { Some(count + 1) } else { None }
    );
}

#[test]
fn r1_preserves_rejected_initial_sha_and_review() {
    assert_history_preserved(1);
}

#[test]
fn r2_preserves_both_rejected_pairs_in_order() {
    assert_history_preserved(2);
}

#[test]
fn r3_preserves_all_rejected_pairs_in_order() {
    assert_history_preserved(3);
}

#[test]
fn current_pass_does_not_replace_historical_rejects() {
    let (handoffs, reviews) = history(4, Pass);
    let before = (handoffs.clone(), reviews.clone());
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 3)
            .unwrap()
            .disposition(),
        NoRepairRequired
    );
    assert_eq!((handoffs, reviews), before);
    assert_eq!(
        before
            .1
            .iter()
            .map(A4ReviewNonTemporalCore::verdict)
            .collect::<Vec<_>>(),
        vec![Reject, Reject, Reject, Pass]
    );
}

#[test]
fn prior_pass_forbids_later_repair() {
    for index in 0..3 {
        let (handoffs, mut reviews) = history(4, Reject);
        reviews[index] = review(handoffs[index].final_sha().as_str(), Pass, true, vec![]);
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 3),
            Err(Error::PriorAttemptWasNotRejected {
                attempt_index: index
            })
        );
    }
}

#[test]
fn prior_nonblocking_pass_forbids_later_repair() {
    let (handoffs, mut reviews) = history(2, Reject);
    reviews[0] = review(
        handoffs[0].final_sha().as_str(),
        PassWithNonblockingFindings,
        true,
        vec![],
    );
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 1),
        Err(Error::PriorAttemptWasNotRejected { attempt_index: 0 })
    );
}

#[test]
fn unequal_history_lengths_fail_in_both_directions() {
    let (handoffs, reviews) = history(4, Reject);
    for handoff_count in 0..=4 {
        for review_count in 0..=4 {
            if handoff_count != review_count {
                assert_eq!(
                    Control::evaluate(&handoffs[..handoff_count], &reviews[..review_count], 3),
                    Err(Error::HistoryLengthMismatch)
                );
            }
        }
    }
}

#[test]
fn explicit_count_must_match_history_exactly() {
    for pair_count in 1..=5 {
        let (handoffs, reviews) = history(pair_count, Reject);
        for count in 0..=3 {
            if pair_count != count + 1 {
                assert_eq!(
                    Control::evaluate(&handoffs, &reviews, count),
                    Err(Error::RepairAttemptCountMismatch)
                );
            }
        }
    }
}

#[test]
fn empty_history_has_no_current_pair() {
    for count in 0..=3 {
        assert_eq!(
            Control::evaluate(&[], &[], count),
            Err(Error::EmptyAttemptHistory)
        );
    }
}

#[test]
fn repeated_valid_evaluations_are_identical() {
    for count in 0..=3 {
        for verdict in A4ReviewVerdict::ALL {
            let (handoffs, reviews) = history(count + 1, verdict);
            let expected = Control::evaluate(&handoffs, &reviews, count);
            assert!(expected.is_ok());
            for _ in 0..100 {
                assert_eq!(Control::evaluate(&handoffs, &reviews, count), expected);
            }
        }
    }
}

#[test]
fn repeated_invalid_evaluation_preserves_evidence_and_error() {
    let (handoffs, mut reviews) = history(3, Pass);
    reviews[2] = review(handoffs[0].final_sha().as_str(), Pass, false, vec![]);
    let before = (handoffs.clone(), reviews.clone());
    for _ in 0..100 {
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 2),
            Err(Error::ShaMismatch { attempt_index: 2 })
        );
    }
    assert_eq!((handoffs, reviews), before);
}

#[test]
fn readiness_and_textual_attempt_conventions_are_not_admission_rules() {
    let sha = "a".repeat(40);
    let handoffs = [handoff(&sha, false)];
    let reviews = [review(&sha, Pass, true, vec![])];
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 0)
            .unwrap()
            .disposition(),
        NoRepairRequired
    );
}

#[test]
fn no_global_sha_uniqueness_or_ancestry_rule_is_invented() {
    let sha = "a".repeat(40);
    let handoffs = vec![handoff(&sha, true); 4];
    let reviews = vec![review(&sha, Reject, true, vec![]); 4];
    assert_eq!(
        Control::evaluate(&handoffs, &reviews, 3)
            .unwrap()
            .disposition(),
        RepairBoundExhausted
    );
}

#[test]
fn blocking_findings_are_preserved_without_inventing_an_acceptance_gate() {
    // No frozen repair-cycle disposition exists for this schema-valid combination.
    // NO_REPAIR_REQUIRED does not close findings or confer A2/A1 acceptance.
    for verdict in [Pass, PassWithNonblockingFindings] {
        let sha = "a".repeat(40);
        let handoffs = [handoff(&sha, true)];
        let reviews = [review(&sha, verdict, true, vec![finding("unclosed", true)])];
        let before = reviews.clone();
        assert_eq!(
            Control::evaluate(&handoffs, &reviews, 0)
                .unwrap()
                .disposition(),
            NoRepairRequired
        );
        assert_eq!(reviews, before);
    }
}

#[test]
fn dispositions_are_exhaustive_control_meanings() {
    fn canonical(value: RepairCycleDisposition) -> &'static str {
        match value {
            NoRepairRequired => "NO_REPAIR_REQUIRED",
            RepairRequired => "REPAIR_REQUIRED",
            RepairBoundExhausted => "REPAIR_BOUND_EXHAUSTED",
        }
    }
    assert_eq!(canonical(NoRepairRequired), "NO_REPAIR_REQUIRED");
    assert_eq!(canonical(RepairRequired), "REPAIR_REQUIRED");
    assert_eq!(canonical(RepairBoundExhausted), "REPAIR_BOUND_EXHAUSTED");
}
