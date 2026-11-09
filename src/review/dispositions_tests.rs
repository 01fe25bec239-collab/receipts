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
    let p = disposition_proposal("project", &req, &r, &ds).unwrap();
    let types: Vec<_> = p.events.iter().map(|e| e.event_type()).collect();
    assert_eq!(types, [EventType::FindingDispositioned; 2]);
    // Order follows the review's findings, not the caller's.
    assert!(String::from_utf8_lossy(&p.content[0].1).contains("\"finding_id\":\"a\""));
    assert!(String::from_utf8_lossy(&p.content[1].1).contains("\"disposition\":\"REJECT\""));
    let again = disposition_proposal("project", &req, &r, &ds).unwrap();
    assert_eq!(again.to_journal(), p.to_journal());
    assert_eq!(p.claim_key(), disposition_claim_key("project", "review-1"));
    assert!(
        disposition_proposal("project", &req, &r, &[fix("a")]).is_err(),
        "an incomplete set records nothing"
    );
}
