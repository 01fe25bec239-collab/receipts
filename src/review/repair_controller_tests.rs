use crate::repair_controller::*;
use crate::test_repo::{TempDir, clock, request};
use crate::wire::encode_a4_review;
use crate::{
    A3HandoffCheck, A3HandoffCheckResult, A3HandoffNonTemporalCore, A4Review, A4ReviewDimension,
    A4ReviewDimensionAssessment, A4ReviewDimensionReview, A4ReviewFinding, A4ReviewFindingCategory,
    A4ReviewFindingSeverity, A4ReviewNonTemporalCore, A4ReviewVerdict, AssuranceProfile,
    RepairCycleControlError, SafetyInterruption, SafetyInterruptionNonTemporalCore,
    SafetyInterruptionState,
};
use receipts_state::{
    DecisionRecordKind, DecisionRecordV1, EventType, StateReader, StateWriteOperationV1,
    StateWriteSubjectV1, TrustedCoreStore,
};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
};

const BASE: &str = "0000000000000000000000000000000000000000";

fn sha_n(n: usize) -> String {
    format!("{:040x}", n + 1)
}

fn finding(id: &str, category: A4ReviewFindingCategory, blocking: bool) -> A4ReviewFinding {
    A4ReviewFinding::new(
        id.into(),
        A4ReviewFindingSeverity::Medium,
        category,
        "d".into(),
        blocking,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}

fn review(n: usize, verdict: A4ReviewVerdict) -> A4ReviewNonTemporalCore {
    let (blocking, nonblocking) = match verdict {
        A4ReviewVerdict::Reject => (
            vec![finding("b1", A4ReviewFindingCategory::Correctness, true)],
            vec![
                finding("n-style", A4ReviewFindingCategory::Style, false),
                finding("n-floor", A4ReviewFindingCategory::ContractViolation, false),
            ],
        ),
        _ => (vec![], vec![]),
    };
    A4ReviewNonTemporalCore::new(
        format!("review-{n}"),
        "task-1".into(),
        sha_n(n),
        verdict,
        true,
        blocking,
        nonblocking,
        vec![A4ReviewDimensionReview::new(
            A4ReviewDimension::Correctness,
            A4ReviewDimensionAssessment::Satisfied,
            None,
        )],
        None,
        None,
        Some(vec![]),
        None,
        None,
    )
    .unwrap()
}

fn handoff(n: usize) -> A3HandoffNonTemporalCore {
    let check = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec!["true".into()],
            0,
            CommitSha::parse(&sha_n(n)).unwrap(),
            None,
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    A3HandoffNonTemporalCore::new(
        "task-1".into(),
        format!("attempt-{n}"),
        BASE.into(),
        sha_n(n),
        "b".into(),
        vec![],
        vec![check],
        true,
        None,
        None,
        None,
        None,
        None,
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

fn decide(
    n_rejections: usize,
    last: A4ReviewVerdict,
    interruption: Option<&SafetyInterruption>,
) -> Result<RepairOutcome, RepairCycleControlError> {
    let handoffs: Vec<_> = (0..n_rejections).map(handoff).collect();
    let mut reviews: Vec<_> = (0..n_rejections - 1)
        .map(|n| review(n, A4ReviewVerdict::Reject))
        .collect();
    reviews.push(review(n_rejections - 1, last));
    let req = request(BASE, &sha_n(0), AssuranceProfile::Standard, None);
    RepairController::decide(&RepairInput {
        project_id: "project",
        request: &req,
        handoffs: &handoffs,
        reviews: &reviews,
        completed_repair_attempts: n_rejections - 1,
        interruption,
    })
}

#[test]
fn bounded_repair_terminates_with_ladder() {
    let mut issued = Vec::new();
    for n in 1..=4 {
        match decide(n, A4ReviewVerdict::Reject, None).unwrap().decision {
            RepairDecision::Issue(r) => {
                assert_eq!(r.attempt_n, n);
                assert_eq!(r.bound, 3);
                issued.push(r.tier);
            }
            RepairDecision::HumanRequired { reason, .. } => {
                assert_eq!(n, 4);
                assert_eq!(reason, RepairEscalation::BoundExhausted);
            }
            RepairDecision::NotRequired => panic!("reject never needs no repair"),
        }
    }
    assert_eq!(
        issued,
        [
            RepairTier::SameTier,
            RepairTier::FrontierFloor,
            RepairTier::FrontierFloor
        ]
    );
    // A fifth pair is outside the bound and is invalid evidence, not a repair.
    let handoffs: Vec<_> = (0..5).map(handoff).collect();
    let reviews: Vec<_> = (0..5).map(|n| review(n, A4ReviewVerdict::Reject)).collect();
    let req = request(BASE, &sha_n(0), AssuranceProfile::Standard, None);
    assert_eq!(
        RepairController::decide(&RepairInput {
            project_id: "project",
            request: &req,
            handoffs: &handoffs,
            reviews: &reviews,
            completed_repair_attempts: 4,
            interruption: None
        })
        .err(),
        Some(RepairCycleControlError::RepairAttemptBoundExceeded)
    );
}

#[test]
fn bound_exceeded_escalates_human_required() {
    let o = decide(4, A4ReviewVerdict::Reject, None).unwrap();
    let RepairDecision::HumanRequired {
        reason,
        rejected_shas,
    } = &o.decision
    else {
        panic!("{:?}", o.decision)
    };
    assert_eq!(*reason, RepairEscalation::BoundExhausted);
    assert_eq!(rejected_shas.len(), 4);
    let events: Vec<_> = o
        .proposal
        .unwrap()
        .events
        .iter()
        .map(|e| e.event_type())
        .collect();
    assert_eq!(
        events,
        [EventType::RepairLimitReached, EventType::HumanRequired]
    );
}

#[test]
fn repair_request_lists_blocking_findings_first() {
    let RepairDecision::Issue(r) = decide(1, A4ReviewVerdict::Reject, None).unwrap().decision
    else {
        panic!()
    };
    let ids: Vec<_> = r
        .findings
        .iter()
        .map(|f| f.finding_id().to_owned())
        .collect();
    assert_eq!(ids, ["b1", "n-floor", "n-style"]);
    assert_eq!(r.parent_attempt_id, "attempt-0");
    assert_eq!(r.rejected_sha.as_str(), sha_n(0));
    let json = repair_request_json(&r);
    assert!(json.find("\"b1\"").unwrap() < json.find("\"n-style\"").unwrap());
}

#[test]
fn passing_review_needs_no_repair() {
    let o = decide(2, A4ReviewVerdict::PassWithNonblockingFindings, None).unwrap();
    assert_eq!(o.decision, RepairDecision::NotRequired);
    assert!(o.proposal.unwrap().events.is_empty());
}

#[test]
fn policy_refusal_is_never_repaired_or_shopped() {
    let interruption = SafetyInterruption::new(
        SafetyInterruptionNonTemporalCore::new(
            "i".into(),
            "task-1".into(),
            "attempt-0".into(),
            "provider-b".into(),
            None,
            SafetyInterruptionState::PolicyBlocked,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        clock(),
    );
    let o = decide(1, A4ReviewVerdict::Reject, Some(&interruption)).unwrap();
    assert!(matches!(
        o.decision,
        RepairDecision::HumanRequired {
            reason: RepairEscalation::SafetyInterruption,
            ..
        }
    ));
    let events: Vec<_> = o
        .proposal
        .unwrap()
        .events
        .iter()
        .map(|e| e.event_type())
        .collect();
    assert_eq!(events, [EventType::HumanRequired]);
}

#[test]
fn rejected_sha_record_never_deleted() {
    use std::os::unix::fs::DirBuilderExt;
    let dir = TempDir::new("repair-state");
    let state_dir = dir.path().join("s");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&state_dir)
        .unwrap();
    let db = state_dir.join("state.db");
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let admit = |s: &TrustedCoreStore| {
        s.admit(StateWriteSubjectV1 {
            operation: StateWriteOperationV1::DecisionRecord,
            project_id: "project".into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        })
    };
    // Three rejections, then a pass at a new SHA.
    for n in 0..3 {
        let json = encode_a4_review(&A4Review::new(review(n, A4ReviewVerdict::Reject), None));
        let rec = DecisionRecordV1::parse(
            "project",
            DecisionRecordKind::A4Review,
            &format!("review-{n}"),
            &json,
        )
        .unwrap();
        store.append_decision_record(admit(&store), &rec).unwrap();
    }
    // Replacing a rejected review with a PASS under the same identity is refused.
    let forged = encode_a4_review(&A4Review::new(review(0, A4ReviewVerdict::Pass), None));
    let rec = DecisionRecordV1::parse("project", DecisionRecordKind::A4Review, "review-0", &forged)
        .unwrap();
    assert!(store.append_decision_record(admit(&store), &rec).is_err());
    let pass = encode_a4_review(&A4Review::new(review(3, A4ReviewVerdict::Pass), None));
    let rec = DecisionRecordV1::parse("project", DecisionRecordKind::A4Review, "review-3", &pass)
        .unwrap();
    store.append_decision_record(admit(&store), &rec).unwrap();
    drop(store);
    let reader = StateReader::open_existing(&db).unwrap();
    let stored = reader
        .list_decision_records("project", DecisionRecordKind::A4Review, Some("task-1"))
        .unwrap();
    let rejected: Vec<_> = stored
        .iter()
        .filter(|r| r.record_json().contains("\"verdict\":\"REJECT\""))
        .map(|r| r.subject_sha().unwrap().to_owned())
        .collect();
    assert_eq!(rejected, [sha_n(0), sha_n(1), sha_n(2)]);
    // The repair request carries every rejected SHA forward, in order.
    let RepairDecision::Issue(r) = decide(3, A4ReviewVerdict::Reject, None).unwrap().decision
    else {
        panic!()
    };
    let carried: Vec<_> = r
        .rejected_shas
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(carried, [sha_n(0), sha_n(1), sha_n(2)]);
}
