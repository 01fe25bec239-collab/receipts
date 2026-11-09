use crate::dispositions::*;
use crate::test_repo::{clock, request};
use crate::{
    A4ReviewDimension, A4ReviewDimensionAssessment, A4ReviewDimensionReview, A4ReviewFinding,
    A4ReviewFindingCategory, A4ReviewFindingSeverity, A4ReviewNonTemporalCore, A4ReviewVerdict,
    AssuranceProfile,
};
use receipts_state::EventType;

const BASE: &str = "0000000000000000000000000000000000000000";

fn review(verdict: A4ReviewVerdict, ids: &[&str]) -> A4ReviewNonTemporalCore {
    let findings = ids
        .iter()
        .map(|id| {
            A4ReviewFinding::new(
                (*id).into(),
                A4ReviewFindingSeverity::Low,
                A4ReviewFindingCategory::Style,
                "d".into(),
                false,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        })
        .collect();
    A4ReviewNonTemporalCore::new(
        "review-1".into(),
        "task-1".into(),
        format!("{:040x}", 1),
        verdict,
        true,
        vec![],
        findings,
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

fn d(id: &str, kind: FindingDispositionKind) -> FindingDisposition {
    FindingDisposition {
        finding_id: id.into(),
        kind,
        decided_by: "a2".into(),
    }
}
fn fix(id: &str) -> FindingDisposition {
    d(
        id,
        FindingDispositionKind::FixNow {
            task_id: "t2".into(),
        },
    )
}

#[test]
fn every_nonblocking_finding_needs_exactly_one_complete_disposition() {
    use DispositionRefusal as R;
    let r = review(A4ReviewVerdict::PassWithNonblockingFindings, &["a", "b"]);
    assert_eq!(validate_dispositions(&r, &[fix("a"), fix("b")]), Ok(()));
    assert_eq!(
        validate_dispositions(&r, &[fix("a")]),
        Err(R::Missing(vec!["b".into()]))
    );
    assert_eq!(
        validate_dispositions(&r, &[fix("a"), fix("b"), fix("c")]),
        Err(R::UnknownFinding("c".into()))
    );
    assert_eq!(
        validate_dispositions(&r, &[fix("a"), fix("a"), fix("b")]),
        Err(R::DuplicateFinding("a".into()))
    );
    let bad = [
        (
            FindingDispositionKind::FixNow {
                task_id: " ".into(),
            },
            "task_id",
        ),
        (
            FindingDispositionKind::Defer {
                issue_id: "".into(),
                revisit_point: "p".into(),
            },
            "issue_id",
        ),
        (
            FindingDispositionKind::Defer {
                issue_id: "I".into(),
                revisit_point: "".into(),
            },
            "revisit_point",
        ),
        (
            FindingDispositionKind::Reject { reason: "".into() },
            "reason",
        ),
    ];
    for (kind, field) in bad {
        assert_eq!(
            validate_dispositions(&r, &[d("a", kind), fix("b")]),
            Err(R::EmptyField(field))
        );
    }
    let mut anon = fix("a");
    anon.decided_by = " ".into();
    assert_eq!(
        validate_dispositions(&r, &[anon, fix("b")]),
        Err(R::EmptyField("decided_by"))
    );
}

#[test]
fn only_a_pass_with_nonblocking_findings_review_is_dispositioned() {
    for verdict in [A4ReviewVerdict::Pass, A4ReviewVerdict::Reject] {
        let r = review(verdict, &[]);
        assert_eq!(
            validate_dispositions(&r, &[]),
            Err(DispositionRefusal::NotApplicable(verdict))
        );
    }
}

#[test]
fn disposition_events_are_finding_specific_and_deterministic() {
    let r = review(A4ReviewVerdict::PassWithNonblockingFindings, &["a", "b"]);
    let req = request(
        BASE,
        &format!("{:040x}", 1),
        AssuranceProfile::Standard,
        None,
    );
    let _ = clock();
    let ds = [
        d(
            "b",
            FindingDispositionKind::Reject {
                reason: "intended".into(),
            },
        ),
        fix("a"),
    ];
    let p = disposition_proposal("project", &req, &r, &ds, 1).unwrap();
    let types: Vec<_> = p.events.iter().map(|e| e.event_type()).collect();
    assert_eq!(types, [EventType::FindingDispositioned; 2]);
    // Order follows the review's findings, not the caller's.
    assert!(String::from_utf8_lossy(&p.content[0].1).contains("\"finding_id\":\"a\""));
    assert!(String::from_utf8_lossy(&p.content[1].1).contains("\"disposition\":\"REJECT\""));
    let again = disposition_proposal("project", &req, &r, &ds, 1).unwrap();
    assert_eq!(again.to_journal(), p.to_journal());
    assert_eq!(
        p.binding(),
        &disposition_claim_binding("project", &req, &r, &ds, 1).unwrap()
    );
    assert!(
        disposition_proposal("project", &req, &r, &[fix("a")], 1).is_err(),
        "an incomplete set records nothing"
    );
}

// ---- Recording through the sink contract.

#[test]
fn dispositions_publish_once_through_claim_seal_publish_and_a_new_generation_is_new_work() {
    use crate::record::{Recorded, record_proposal};
    use crate::test_repo::MemorySink;
    let (req, r) = (
        request(
            BASE,
            &format!("{:040x}", 1),
            AssuranceProfile::Standard,
            None,
        ),
        review(A4ReviewVerdict::PassWithNonblockingFindings, &["a", "b"]),
    );
    let ds = [fix("a"), fix("b")];
    let p = disposition_proposal("project", &req, &r, &ds, 1).unwrap();
    let mut sink = MemorySink::default();
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    // An exact retry completes (publication is idempotent) and adds nothing.
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    assert_eq!(sink.proposals.len(), 1);
    assert_eq!(sink.claims.len(), 1);
    // The same decisions under another generation are a separate claim.
    let g2 = disposition_proposal("project", &req, &r, &ds, 2).unwrap();
    assert_ne!(g2.claim_key(), p.claim_key());
    assert_eq!(g2.binding().scope().generation(), 2);
    assert_eq!(record_dispositions(&mut sink, &g2), Ok(Recorded::Published));
    assert_eq!(sink.claims.len(), 2);
    // Different decisions are a different scope even with the same review.
    let rejected = [
        fix("a"),
        d(
            "b",
            FindingDispositionKind::Reject {
                reason: "no".into(),
            },
        ),
    ];
    let other = disposition_proposal("project", &req, &r, &rejected, 1).unwrap();
    assert_ne!(other.claim_key(), p.claim_key());
    assert_eq!(
        other.binding().operation(),
        crate::ReviewOperation::Disposition
    );

    // A label of another operation settles nothing and claims nothing.
    let mut fresh = MemorySink::default();
    assert!(record_proposal(&mut fresh, &p, "REPAIR_ISSUED").is_err());
    assert!(fresh.claims.is_empty());
    // An unfinished, abandoned or terminal claim is never published over.
    for (case, mark) in [("in-flight", 0), ("abandoned", 1), ("terminal", 2)] {
        let mut blocked = MemorySink::default();
        match mark {
            0 => {
                blocked.claims.insert(p.claim_key().to_owned(), None);
            }
            1 => {
                blocked.abandoned.insert(p.claim_key().to_owned());
            }
            _ => {
                blocked.terminal.insert(p.claim_key().to_owned());
            }
        }
        assert!(record_dispositions(&mut blocked, &p).is_err(), "{case}");
        assert!(blocked.proposals.is_empty(), "{case}");
    }
    // A claim sealed with anything else conflicts.
    let mut conflicting = MemorySink::default();
    conflicting.claims.insert(
        p.claim_key().to_owned(),
        Some(crate::SealedReview::new(&other, "DISPOSITIONS", true, 0)),
    );
    assert!(record_dispositions(&mut conflicting, &p).is_err());
    assert!(conflicting.proposals.is_empty());
}
