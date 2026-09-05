use crate::*;
use receipts_workspace_execution::{
    WorkspaceCheckpointCheckSource, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};
use std::num::NonZeroU64;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
fn dimension() -> A4ReviewDimensionReview {
    A4ReviewDimensionReview::new(
        A4ReviewDimension::Correctness,
        A4ReviewDimensionAssessment::NotSatisfied,
        Some(String::new()),
    )
}
fn core(
    review: &str,
    task: &str,
    sha: &str,
    dimensions: Vec<A4ReviewDimensionReview>,
) -> Result<A4ReviewNonTemporalCore, A4ReviewConstructionError> {
    A4ReviewNonTemporalCore::new(
        review.into(),
        task.into(),
        sha.into(),
        A4ReviewVerdict::Pass,
        false,
        vec![],
        vec![],
        dimensions,
        None,
        None,
        None,
        None,
        None,
    )
}
fn finding(id: &str, description: &str) -> Result<A4ReviewFinding, A4ReviewConstructionError> {
    A4ReviewFinding::new(
        id.into(),
        A4ReviewFindingSeverity::Info,
        A4ReviewFindingCategory::Correctness,
        description.into(),
        false,
        None,
        None,
        None,
        None,
        None,
    )
}
fn check(
    sha: &str,
    argv: Vec<String>,
) -> Result<A4ReviewReproductionCheck, A4ReviewConstructionError> {
    A4ReviewReproductionCheck::new(
        WorkspaceCheckpointCheckSource::ReviewExecution,
        argv,
        0,
        sha.into(),
        None,
        None,
        None,
    )
}
fn malformed_shas() -> Vec<String> {
    vec![
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        format!(" {SHA}"),
        format!("{SHA} "),
        "HEAD".into(),
        "main".into(),
        "refs/heads/main".into(),
        "abcdef0".into(),
        "ａ".repeat(40),
        format!("{}\n", "a".repeat(39)),
        String::new(),
    ]
}
#[test]
fn exact_sha_validation_on_both_records() {
    assert_eq!(
        core("r", "t", SHA, vec![dimension()])
            .unwrap()
            .reviewed_sha(),
        SHA
    );
    assert_eq!(check(SHA, vec!["".into()]).unwrap().code_sha(), SHA);
    for sha in malformed_shas() {
        assert_eq!(
            core("r", "t", &sha, vec![dimension()]),
            Err(A4ReviewConstructionError::MalformedReviewedSha),
            "{sha:?}"
        );
        assert_eq!(
            check(&sha, vec!["tool".into()]),
            Err(A4ReviewConstructionError::MalformedReproductionCodeSha),
            "{sha:?}"
        );
    }
}
#[test]
fn unicode_id_boundaries_and_exact_preservation() {
    for id in ["界".into(), "界".repeat(200), " e\u{301} \n".into()] {
        let review = core(&id, &id, SHA, vec![dimension()]).unwrap();
        assert_eq!(review.review_id(), id);
        assert_eq!(review.task_id(), id);
        assert_eq!(finding(&id, "text").unwrap().finding_id(), id);
    }
    for (id, re, te, fe) in [
        (
            String::new(),
            A4ReviewConstructionError::EmptyReviewId,
            A4ReviewConstructionError::EmptyTaskId,
            A4ReviewConstructionError::EmptyFindingId,
        ),
        (
            "界".repeat(201),
            A4ReviewConstructionError::ReviewIdTooLong,
            A4ReviewConstructionError::TaskIdTooLong,
            A4ReviewConstructionError::FindingIdTooLong,
        ),
    ] {
        assert_eq!(core(&id, "t", SHA, vec![dimension()]), Err(re));
        assert_eq!(core("r", &id, SHA, vec![dimension()]), Err(te));
        assert_eq!(finding(&id, "text"), Err(fe));
    }
}
#[test]
fn reviewer_opaque_identity_and_optionality() {
    let empty = A4ReviewReviewer::new(None, None, None, None).unwrap();
    assert_eq!(
        (
            empty.provider_id(),
            empty.model_id(),
            empty.runtime_id(),
            empty.session_ref()
        ),
        (None, None, None, None)
    );
    let reviewer = A4ReviewReviewer::new(
        Some(" future-provider 界 ".into()),
        Some(" future-model 界 ".into()),
        Some(" future-runtime 界 ".into()),
        Some(String::new()),
    )
    .unwrap();
    assert_eq!(reviewer.provider_id(), Some(" future-provider 界 "));
    assert_eq!(reviewer.model_id(), Some(" future-model 界 "));
    assert_eq!(reviewer.runtime_id(), Some(" future-runtime 界 "));
    assert_eq!(reviewer.session_ref(), Some(""));
    assert_eq!(
        A4ReviewReviewer::new(Some("".into()), None, None, None),
        Err(A4ReviewConstructionError::EmptyReviewerProviderId)
    );
    assert_eq!(
        A4ReviewReviewer::new(None, Some("".into()), None, None),
        Err(A4ReviewConstructionError::EmptyReviewerModelId)
    );
    assert_eq!(
        A4ReviewReviewer::new(None, None, Some("".into()), None),
        Err(A4ReviewConstructionError::EmptyReviewerRuntimeId)
    );
    let spaces = A4ReviewReviewer::new(
        Some(" ".into()),
        Some("\n".into()),
        Some("\t".into()),
        Some(" 界 ".into()),
    )
    .unwrap();
    assert_eq!(
        (
            spaces.provider_id(),
            spaces.model_id(),
            spaces.runtime_id(),
            spaces.session_ref()
        ),
        (Some(" "), Some("\n"), Some("\t"), Some(" 界 "))
    );
    for reviewer in [None, Some(empty), Some(reviewer), Some(spaces)] {
        let r = A4ReviewNonTemporalCore::new(
            "r".into(),
            "t".into(),
            SHA.into(),
            A4ReviewVerdict::Reject,
            false,
            vec![],
            vec![],
            vec![dimension()],
            reviewer.clone(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.reviewer(), reviewer.as_ref());
    }
}
#[test]
fn findings_exercise_every_vocabulary_and_optional_shape() {
    for severity in A4ReviewFindingSeverity::ALL {
        for category in A4ReviewFindingCategory::ALL {
            for confidence in A4ReviewFindingConfidence::ALL {
                for source in A4ReviewFindingSource::ALL {
                    let f = A4ReviewFinding::new(
                        " 界 ".into(),
                        severity,
                        category,
                        " \n".into(),
                        false,
                        Some("".into()),
                        NonZeroU64::new(1),
                        None,
                        Some(confidence),
                        Some(source),
                    )
                    .unwrap();
                    assert_eq!(
                        (f.severity(), f.category(), f.confidence(), f.source()),
                        (severity, category, Some(confidence), Some(source))
                    );
                    assert_eq!(
                        (
                            f.finding_id(),
                            f.description(),
                            f.path(),
                            f.line(),
                            f.blocking()
                        ),
                        (" 界 ", " \n", Some(""), NonZeroU64::new(1), false)
                    );
                }
            }
        }
    }
    assert_eq!(
        finding("f", ""),
        Err(A4ReviewConstructionError::EmptyFindingDescription)
    );
    let f = finding("f", &"界".repeat(1000)).unwrap();
    assert_eq!(f.description(), "界".repeat(1000));
    assert_eq!(
        (
            f.path(),
            f.line(),
            f.evidence_ref(),
            f.confidence(),
            f.source()
        ),
        (None, None, None, None, None)
    );
    assert!(NonZeroU64::new(0).is_none());
    let f = A4ReviewFinding::new(
        "f".into(),
        f.severity(),
        f.category(),
        "x".into(),
        true,
        Some(" ../界 \n".into()),
        NonZeroU64::new(u64::MAX),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(f.line().unwrap().get(), u64::MAX);
    assert_eq!(f.path(), Some(" ../界 \n"));
}
#[test]
fn workspace_references_have_physical_identity_and_exact_contents() {
    for kind in WorkspaceCheckpointRefType::ALL {
        let original = WorkspaceCheckpointRef::new(
            kind,
            " ../界 opaque ",
            Some(" digest ".into()),
            Some("".into()),
        )
        .unwrap();
        let f = A4ReviewFinding::new(
            "f".into(),
            A4ReviewFindingSeverity::Critical,
            A4ReviewFindingCategory::DocGap,
            "x".into(),
            true,
            None,
            None,
            Some(original.clone()),
            None,
            None,
        )
        .unwrap();
        let c = A4ReviewReproductionCheck::new(
            WorkspaceCheckpointCheckSource::GitProvenance,
            vec!["tool".into()],
            137,
            SHA.into(),
            Some(false),
            Some(original.clone()),
            Some(A4ReviewReproductionCheckResult::Unknown),
        )
        .unwrap();
        let evidence: &WorkspaceCheckpointRef = f.evidence_ref().unwrap();
        let output: &WorkspaceCheckpointRef = c.output_ref().unwrap();
        assert_eq!(evidence, &original);
        assert_eq!(output, &original);
        assert_eq!(
            (
                output.ref_type(),
                output.target(),
                output.digest(),
                output.section()
            ),
            (kind, " ../界 opaque ", Some(" digest "), Some(""))
        );
    }
}
#[test]
fn dimensions_minimum_vocabularies_order_and_duplicates() {
    assert_eq!(
        core("r", "t", SHA, vec![]),
        Err(A4ReviewConstructionError::EmptyDimensions)
    );
    for d in A4ReviewDimension::ALL {
        for a in A4ReviewDimensionAssessment::ALL {
            let entry = A4ReviewDimensionReview::new(d, a, Some("".into()));
            let r = core("r", "t", SHA, vec![entry.clone()]).unwrap();
            assert_eq!(r.dimensions(), &[entry]);
            assert_eq!(
                (
                    r.dimensions()[0].dimension(),
                    r.dimensions()[0].assessment(),
                    r.dimensions()[0].note()
                ),
                (d, a, Some(""))
            );
        }
    }
    let entries = vec![
        dimension(),
        A4ReviewDimensionReview::new(
            A4ReviewDimension::ScopeCompliance,
            A4ReviewDimensionAssessment::Satisfied,
            Some(" 界 \n".into()),
        ),
        dimension(),
        A4ReviewDimensionReview::new(
            A4ReviewDimension::WriteScope,
            A4ReviewDimensionAssessment::NotApplicable,
            None,
        ),
    ];
    assert_eq!(
        core("r", "t", SHA, entries.clone()).unwrap().dimensions(),
        entries
    );
}
#[test]
fn reproduction_result_closed_vocabulary() {
    use A4ReviewReproductionCheckResult::*;
    let expected = [Pass, Fail, Error, Skipped, Unknown];
    assert_eq!(A4ReviewReproductionCheckResult::ALL, expected);
    assert_eq!(expected.len(), 5);
    for (i, (value, text)) in expected
        .into_iter()
        .zip(["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"])
        .enumerate()
    {
        assert_eq!(value.as_str(), text);
        let exhaustive = match value {
            Pass => "PASS",
            Fail => "FAIL",
            Error => "ERROR",
            Skipped => "SKIPPED",
            Unknown => "UNKNOWN",
        };
        assert_eq!(exhaustive, text);
        for other in &expected[i + 1..] {
            assert_ne!(value, *other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}
#[test]
fn reproduction_optional_states_are_independent() {
    for performed in [None, Some(false), Some(true)] {
        for checks in [
            None,
            Some(vec![]),
            Some(vec![check(SHA, vec!["".into()]).unwrap()]),
        ] {
            for limitation in [
                A4ReviewReproductionLimitation::Omitted,
                A4ReviewReproductionLimitation::Null,
                A4ReviewReproductionLimitation::Text("".into()),
                A4ReviewReproductionLimitation::Text(" 界 \n".into()),
            ] {
                let reproduction =
                    A4ReviewReproduction::new(performed, checks.clone(), limitation.clone());
                assert_eq!(reproduction.performed(), performed);
                assert_eq!(reproduction.checks(), checks.as_deref());
                assert_eq!(reproduction.limitation(), &limitation);
                let r = A4ReviewNonTemporalCore::new(
                    "r".into(),
                    "t".into(),
                    SHA.into(),
                    A4ReviewVerdict::Pass,
                    false,
                    vec![],
                    vec![],
                    vec![dimension()],
                    None,
                    Some(reproduction.clone()),
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(r.reproduction(), Some(&reproduction));
            }
        }
    }
    assert!(
        core("r", "t", SHA, vec![dimension()])
            .unwrap()
            .reproduction()
            .is_none()
    );
}
#[test]
fn reproduction_check_preserves_argv_and_never_infers_outcomes() {
    assert_eq!(
        check(SHA, vec![]),
        Err(A4ReviewConstructionError::EmptyReproductionCommand)
    );
    for source in WorkspaceCheckpointCheckSource::ALL {
        for argv in [
            vec!["".into()],
            vec!["tool".into()],
            vec![
                "tool".into(),
                "".into(),
                " \t".into(),
                "界".into(),
                "tool".into(),
            ],
        ] {
            for exit_code in [0, 1, 124, 137, -1, i64::MIN, i64::MAX] {
                for timed_out in [None, Some(false), Some(true)] {
                    for result in
                        std::iter::once(None).chain(A4ReviewReproductionCheckResult::ALL.map(Some))
                    {
                        let c = A4ReviewReproductionCheck::new(
                            source,
                            argv.clone(),
                            exit_code,
                            SHA.into(),
                            timed_out,
                            None,
                            result,
                        )
                        .unwrap();
                        let stored_source: WorkspaceCheckpointCheckSource = c.source();
                        assert_eq!(stored_source, source);
                        assert_eq!(c.command(), argv);
                        assert_eq!(
                            (
                                c.exit_code(),
                                c.timed_out(),
                                c.result(),
                                c.code_sha(),
                                c.output_ref()
                            ),
                            (exit_code, timed_out, result, SHA, None)
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn aggregate_preserves_partitions_and_has_no_policy_matrix() {
    let false_finding = finding(" false 界 ", " \n").unwrap();
    let true_finding = A4ReviewFinding::new(
        "true".into(),
        A4ReviewFindingSeverity::Critical,
        A4ReviewFindingCategory::SecurityBoundaryViolation,
        "evidence".into(),
        true,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let blocking = vec![
        false_finding.clone(),
        true_finding.clone(),
        false_finding.clone(),
    ];
    let nonblocking = vec![true_finding.clone(), false_finding, true_finding];
    for verdict in A4ReviewVerdict::ALL {
        for independence in [true, false] {
            for action in std::iter::once(None).chain(A4ReviewRecommendedAction::ALL.map(Some)) {
                for paths in [
                    None,
                    Some(vec![]),
                    Some(vec![
                        "".into(),
                        "src/review/lib.rs".into(),
                        " ../界 ".into(),
                        "src/review/lib.rs".into(),
                    ]),
                ] {
                    for rationale in [None, Some("".into()), Some(" \n界 e\u{301} ".into())] {
                        let checks = vec![
                            check(SHA, vec!["tool".into()]).unwrap(),
                            A4ReviewReproductionCheck::new(
                                WorkspaceCheckpointCheckSource::ReviewExecution,
                                vec!["x".into()],
                                0,
                                SHA.into(),
                                Some(false),
                                None,
                                Some(A4ReviewReproductionCheckResult::Error),
                            )
                            .unwrap(),
                            A4ReviewReproductionCheck::new(
                                WorkspaceCheckpointCheckSource::ReviewExecution,
                                vec!["x".into()],
                                1,
                                SHA.into(),
                                None,
                                None,
                                Some(A4ReviewReproductionCheckResult::Unknown),
                            )
                            .unwrap(),
                        ];
                        let reproduction = A4ReviewReproduction::new(
                            Some(false),
                            Some(checks),
                            A4ReviewReproductionLimitation::Null,
                        );
                        let r = A4ReviewNonTemporalCore::new(
                            " r 界 ".into(),
                            " t 界 ".into(),
                            SHA.into(),
                            verdict,
                            independence,
                            blocking.clone(),
                            nonblocking.clone(),
                            vec![dimension()],
                            None,
                            Some(reproduction.clone()),
                            paths.clone(),
                            action,
                            rationale.clone(),
                        )
                        .unwrap();
                        assert_eq!(
                            (r.review_id(), r.task_id(), r.reviewed_sha()),
                            (" r 界 ", " t 界 ", SHA)
                        );
                        assert_eq!(
                            (
                                r.verdict(),
                                r.independence_attested(),
                                r.recommended_action()
                            ),
                            (verdict, independence, action)
                        );
                        assert_eq!(r.blocking_findings(), blocking);
                        assert_eq!(r.nonblocking_findings(), nonblocking);
                        assert!(!r.blocking_findings()[0].blocking());
                        assert!(r.nonblocking_findings()[0].blocking());
                        assert_eq!(r.unauthorized_file_changes(), paths.as_deref());
                        assert_eq!(r.rationale(), rationale.as_deref());
                        assert_eq!(r.reproduction(), Some(&reproduction));
                        assert_eq!(r.clone(), r);
                    }
                }
            }
        }
    }
}
