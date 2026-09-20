use crate::*;
use receipts_workspace_execution::WorkspaceCheckpointCheckSource;

fn core(verdict: A4ReviewVerdict) -> A4ReviewNonTemporalCore {
    let finding = |id: &str, blocking| {
        A4ReviewFinding::new(
            id.into(),
            A4ReviewFindingSeverity::Critical,
            A4ReviewFindingCategory::Correctness,
            " caller supplied evidence ".into(),
            blocking,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    };
    A4ReviewNonTemporalCore::new(
        " review 界 ".into(),
        " task 界 ".into(),
        "a".repeat(40),
        verdict,
        false,
        vec![finding("blocking partition", false)],
        vec![finding("nonblocking partition", true)],
        vec![A4ReviewDimensionReview::new(
            A4ReviewDimension::Correctness,
            A4ReviewDimensionAssessment::NotSatisfied,
            Some(" supplied note ".into()),
        )],
        Some(
            A4ReviewReviewer::new(
                Some("provider".into()),
                Some("model".into()),
                Some("runtime".into()),
                Some("session".into()),
            )
            .unwrap(),
        ),
        Some(A4ReviewReproduction::new(
            Some(false),
            Some(vec![
                A4ReviewReproductionCheck::new(
                    WorkspaceCheckpointCheckSource::ReviewExecution,
                    vec!["evidence-only-command".into(), " argument ".into()],
                    1,
                    "b".repeat(40),
                    Some(true),
                    None,
                    Some(A4ReviewReproductionCheckResult::Pass),
                )
                .unwrap(),
            ]),
            A4ReviewReproductionLimitation::Text(" limitation ".into()),
        )),
        Some(vec![" opaque/path ".into()]),
        Some(A4ReviewRecommendedAction::RepairRequired),
        Some(" caller rationale \n".into()),
    )
    .unwrap()
}

#[test]
fn absence_remains_distinct_from_presence_without_defaults() {
    let supplied_core = core(A4ReviewVerdict::Pass);
    assert_eq!(supplied_core.review_id(), " review 界 ");
    let absent = A4Review::new(supplied_core.clone(), None);
    let timestamp = ReviewDateTimeV1::try_new("2026-09-08T00:00:00Z").unwrap();
    let present = A4Review::new(supplied_core.clone(), Some(timestamp.clone()));
    let _: &A4ReviewNonTemporalCore = absent.core();
    let _: Option<&ReviewDateTimeV1> = present.reviewed_at();
    let _: &crate::a4_review::A4Review = &present;
    assert_eq!(absent.core(), &supplied_core);
    assert_eq!(present.core(), &supplied_core);
    assert_eq!(absent.reviewed_at(), None);
    assert_eq!(absent.clone().reviewed_at(), None);
    assert_eq!(present.reviewed_at(), Some(&timestamp));
    assert_ne!(absent.reviewed_at(), present.reviewed_at());
}

#[test]
fn present_values_preserve_every_accepted_lexical_distinction() {
    let long = format!("2026-09-08T00:00:00.{}Z", "1234567890".repeat(10_000));
    for input in [
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-08:00",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.123456789123456789Z",
        "2026-09-08T00:00:00.100000000Z",
        &long,
    ] {
        let timestamp = ReviewDateTimeV1::try_new(input).unwrap();
        let allocation = timestamp.as_str().as_ptr();
        let value = A4Review::new(core(A4ReviewVerdict::Pass), Some(timestamp));
        assert_eq!(value.reviewed_at().unwrap().as_str(), input);
        assert_eq!(value.reviewed_at().unwrap().as_str().as_ptr(), allocation);
        assert_eq!(value.clone().reviewed_at().unwrap().as_str(), input);
    }
}

#[test]
fn invalid_datetime_fails_before_composition() {
    for input in [
        "",
        "null",
        "2026-02-30T00:00:00Z",
        "2026-09-08 00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:00:00",
        " 2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z ",
    ] {
        let result = ReviewDateTimeV1::try_new(input)
            .map(|timestamp| A4Review::new(core(A4ReviewVerdict::Pass), Some(timestamp)));
        assert!(result.is_err(), "accepted invalid timestamp: {input}");
    }
}

#[test]
fn all_core_evidence_is_preserved_without_verdict_or_findings_inference() {
    for verdict in A4ReviewVerdict::ALL {
        let supplied = core(verdict);
        for timestamp in [
            None,
            Some(ReviewDateTimeV1::try_new("2026-09-08t00:00:00-00:00").unwrap()),
        ] {
            let moved = supplied.clone();
            let allocation = moved.review_id().as_ptr();
            let value = A4Review::new(moved, timestamp);
            let stored = value.core();
            assert_eq!(stored, &supplied);
            assert_eq!(stored.review_id().as_ptr(), allocation);
            assert_eq!(stored.review_id(), supplied.review_id());
            assert_eq!(stored.task_id(), supplied.task_id());
            assert_eq!(stored.reviewed_sha(), supplied.reviewed_sha());
            assert_eq!(stored.verdict(), verdict);
            assert_eq!(
                stored.independence_attested(),
                supplied.independence_attested()
            );
            assert_eq!(stored.blocking_findings(), supplied.blocking_findings());
            assert_eq!(
                stored.nonblocking_findings(),
                supplied.nonblocking_findings()
            );
            assert_eq!(stored.dimensions(), supplied.dimensions());
            assert_eq!(stored.reviewer(), supplied.reviewer());
            assert_eq!(stored.reproduction(), supplied.reproduction());
            assert_eq!(
                stored.unauthorized_file_changes(),
                supplied.unauthorized_file_changes()
            );
            assert_eq!(stored.recommended_action(), supplied.recommended_action());
            assert_eq!(stored.rationale(), supplied.rationale());
        }
    }
}
