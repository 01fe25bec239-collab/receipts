use super::*;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointExecutedCheckCoreError, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};

const BASE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const IMPLEMENTATION_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn reference(ref_type: WorkspaceCheckpointRefType, target: &str) -> WorkspaceCheckpointRef {
    WorkspaceCheckpointRef::new(
        ref_type,
        target,
        Some(" digest ".into()),
        Some(" section ".into()),
    )
    .unwrap()
}

fn criterion() -> ReviewCapsuleCriterion {
    ReviewCapsuleCriterion::new(
        "criterion".into(),
        "description".into(),
        ReviewCapsuleCriterionKind::Deterministic,
        None,
        None,
    )
    .unwrap()
}

fn severity() -> ReviewCapsuleSeverityPolicy {
    ReviewCapsuleSeverityPolicy::new(vec!["BLOCKING".into()], None).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn capsule(
    review_id: &str,
    task_id: &str,
    attempt_id: &str,
    baseline_sha: &str,
    implementation_sha: &str,
    objective: &str,
    acceptance_criteria: Vec<ReviewCapsuleCriterion>,
    allowed_write_paths: Vec<String>,
    severity_policy: ReviewCapsuleSeverityPolicy,
    context_epoch: i64,
) -> Result<ReviewCapsuleNonTemporalCore, ReviewCapsuleConstructionError> {
    ReviewCapsuleNonTemporalCore::new(
        review_id.into(),
        task_id.into(),
        attempt_id.into(),
        baseline_sha.into(),
        implementation_sha.into(),
        objective.into(),
        acceptance_criteria,
        None,
        None,
        None,
        reference(
            WorkspaceCheckpointRefType::RepoPath,
            "diff supplied by caller",
        ),
        allowed_write_paths,
        None,
        None,
        ReviewCapsuleReviewScope::Full,
        severity_policy,
        false,
        None,
        context_epoch,
    )
}

#[test]
fn required_core_constructs_and_optional_fields_are_absent() {
    let value = capsule(
        "review",
        "task",
        "attempt",
        BASE_SHA,
        IMPLEMENTATION_SHA,
        "objective",
        vec![criterion()],
        vec!["src/review/**".into()],
        severity(),
        0,
    )
    .unwrap();

    assert_eq!(value.review_id(), "review");
    assert_eq!(value.task_id(), "task");
    assert_eq!(value.attempt_id(), "attempt");
    assert_eq!(value.baseline_sha().as_str(), BASE_SHA);
    assert_eq!(value.implementation_sha().as_str(), IMPLEMENTATION_SHA);
    assert_eq!(value.objective(), "objective");
    assert_eq!(value.acceptance_criteria(), &[criterion()]);
    assert_eq!(value.diff().target(), "diff supplied by caller");
    assert_eq!(value.allowed_write_paths(), &["src/review/**"]);
    assert_eq!(value.review_scope(), ReviewCapsuleReviewScope::Full);
    assert_eq!(value.severity_policy(), &severity());
    assert!(!value.reproduction_required());
    assert_eq!(value.context_epoch(), 0);
    assert_eq!(value.non_goals(), None);
    assert_eq!(value.architecture_refs(), None);
    assert_eq!(value.contract_refs(), None);
    assert_eq!(value.checks(), None);
    assert_eq!(value.security_requirements(), None);
    assert_eq!(value.structured_output_schema(), None);

    let _: fn(&ReviewCapsuleNonTemporalCore) -> &CommitSha =
        ReviewCapsuleNonTemporalCore::baseline_sha;
    let _: fn(&ReviewCapsuleNonTemporalCore) -> &CommitSha =
        ReviewCapsuleNonTemporalCore::implementation_sha;
}

#[test]
fn identifier_boundaries_count_unicode_characters_and_preserve_input() {
    let one = "界";
    let two_hundred = "界".repeat(200);
    let two_hundred_one = "界".repeat(201);
    for position in 0..3 {
        for accepted in [one, two_hundred.as_str(), " \t界\n"] {
            let mut ids = ["review", "task", "attempt"];
            ids[position] = accepted;
            let value = capsule(
                ids[0],
                ids[1],
                ids[2],
                BASE_SHA,
                IMPLEMENTATION_SHA,
                "x",
                vec![criterion()],
                vec!["x".into()],
                severity(),
                0,
            )
            .unwrap();
            assert_eq!(
                [value.review_id(), value.task_id(), value.attempt_id()][position],
                accepted
            );
        }

        for (invalid, expected) in [
            (
                "",
                [
                    ReviewCapsuleConstructionError::EmptyReviewId,
                    ReviewCapsuleConstructionError::EmptyTaskId,
                    ReviewCapsuleConstructionError::EmptyAttemptId,
                ][position],
            ),
            (
                two_hundred_one.as_str(),
                [
                    ReviewCapsuleConstructionError::ReviewIdTooLong,
                    ReviewCapsuleConstructionError::TaskIdTooLong,
                    ReviewCapsuleConstructionError::AttemptIdTooLong,
                ][position],
            ),
        ] {
            let mut ids = ["review", "task", "attempt"];
            ids[position] = invalid;
            assert_eq!(
                capsule(
                    ids[0],
                    ids[1],
                    ids[2],
                    BASE_SHA,
                    IMPLEMENTATION_SHA,
                    "x",
                    vec![criterion()],
                    vec!["x".into()],
                    severity(),
                    0,
                ),
                Err(expected)
            );
        }
    }
}

#[test]
fn both_shas_use_exact_workspace_lexical_contract() {
    let invalid = [
        "0123456789abcdef0123456789abcdef0123456",
        "0123456789abcdef0123456789abcdef012345678",
        "0123456789abcdef0123456789abcdef0123456A",
        "0123456789abcdef0123456789abcdef0123456g",
        " 0123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef01234567 ",
        "HEAD",
        "main",
        "0123456",
        "０123456789abcdef0123456789abcdef0123456",
    ];
    for position in 0..2 {
        for malformed in invalid {
            let mut shas = [BASE_SHA, IMPLEMENTATION_SHA];
            shas[position] = malformed;
            assert_eq!(
                capsule(
                    "r",
                    "t",
                    "a",
                    shas[0],
                    shas[1],
                    "x",
                    vec![criterion()],
                    vec!["x".into()],
                    severity(),
                    0,
                ),
                Err([
                    ReviewCapsuleConstructionError::MalformedBaselineSha,
                    ReviewCapsuleConstructionError::MalformedImplementationSha,
                ][position])
            );
        }
    }
    let value = capsule(
        "r",
        "t",
        "a",
        BASE_SHA,
        IMPLEMENTATION_SHA,
        "x",
        vec![criterion()],
        vec!["x".into()],
        severity(),
        0,
    )
    .unwrap();
    assert_eq!(value.baseline_sha().as_str(), BASE_SHA);
    assert_eq!(value.implementation_sha().as_str(), IMPLEMENTATION_SHA);
}

#[test]
fn objective_and_required_array_validation_is_exact() {
    assert_eq!(
        capsule(
            "r",
            "t",
            "a",
            BASE_SHA,
            IMPLEMENTATION_SHA,
            "",
            vec![criterion()],
            vec!["x".into()],
            severity(),
            0,
        ),
        Err(ReviewCapsuleConstructionError::EmptyObjective)
    );
    for objective in [" ", " \t界 e\u{301} \n"] {
        assert_eq!(
            capsule(
                "r",
                "t",
                "a",
                BASE_SHA,
                IMPLEMENTATION_SHA,
                objective,
                vec![criterion()],
                vec!["x".into()],
                severity(),
                0,
            )
            .unwrap()
            .objective(),
            objective
        );
    }
    assert_eq!(
        capsule(
            "r",
            "t",
            "a",
            BASE_SHA,
            IMPLEMENTATION_SHA,
            "x",
            vec![],
            vec!["x".into()],
            severity(),
            0,
        ),
        Err(ReviewCapsuleConstructionError::EmptyAcceptanceCriteria)
    );
    assert_eq!(
        capsule(
            "r",
            "t",
            "a",
            BASE_SHA,
            IMPLEMENTATION_SHA,
            "x",
            vec![criterion()],
            vec![],
            severity(),
            0,
        ),
        Err(ReviewCapsuleConstructionError::EmptyAllowedWritePaths)
    );
}

#[test]
fn criterion_contract_and_closed_vocabulary_are_preserved() {
    use ReviewCapsuleCriterionKind::*;
    assert_eq!(ReviewCapsuleCriterionKind::ALL, [Deterministic, Semantic]);
    assert_eq!(
        ReviewCapsuleCriterionKind::ALL.map(|v| v.as_str()),
        ["DETERMINISTIC", "SEMANTIC"]
    );
    assert_ne!(Deterministic, Semantic);
    for kind in ReviewCapsuleCriterionKind::ALL {
        let exhaustive = match kind {
            Deterministic => "DETERMINISTIC",
            Semantic => "SEMANTIC",
        };
        assert_eq!(kind.as_str(), exhaustive);
    }

    for kind in ReviewCapsuleCriterionKind::ALL {
        for id in ["界".into(), "界".repeat(200), " \t界 ".into()] {
            for description in [" ", " 界 e\u{301} "] {
                for command in [
                    None,
                    Some(vec![]),
                    Some(vec!["".into()]),
                    Some(vec!["界".into(), "".into(), " x ".into(), "界".into()]),
                ] {
                    for rationale in [None, Some("".into()), Some(" 界 e\u{301} ".into())] {
                        let value = ReviewCapsuleCriterion::new(
                            id.clone(),
                            description.into(),
                            kind,
                            command.clone(),
                            rationale.clone(),
                        )
                        .unwrap();
                        assert_eq!(value.id(), id);
                        assert_eq!(value.description(), description);
                        assert_eq!(value.kind(), kind);
                        assert_eq!(value.check_command(), command.as_deref());
                        assert_eq!(value.rationale(), rationale.as_deref());
                    }
                }
            }
        }
    }
    assert_eq!(
        ReviewCapsuleCriterion::new("".into(), "x".into(), Deterministic, None, None),
        Err(ReviewCapsuleConstructionError::EmptyCriterionId)
    );
    assert_eq!(
        ReviewCapsuleCriterion::new("界".repeat(201), "x".into(), Deterministic, None, None),
        Err(ReviewCapsuleConstructionError::CriterionIdTooLong)
    );
    assert_eq!(
        ReviewCapsuleCriterion::new("x".into(), "".into(), Deterministic, None, None),
        Err(ReviewCapsuleConstructionError::EmptyCriterionDescription)
    );
}

#[test]
fn workspace_references_are_reused_in_every_capsule_position() {
    let refs: Vec<_> = WorkspaceCheckpointRefType::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| reference(kind, &format!(" target {index} 界 ")))
        .collect();
    let check_core = WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::ReviewExecution,
        vec!["tool".into()],
        0,
        CommitSha::parse(BASE_SHA).unwrap(),
        None,
        Some(refs[3].clone()),
    )
    .unwrap();
    let value = ReviewCapsuleNonTemporalCore::new(
        "r".into(),
        "t".into(),
        "a".into(),
        BASE_SHA.into(),
        IMPLEMENTATION_SHA.into(),
        "x".into(),
        vec![criterion()],
        None,
        Some(refs.clone()),
        Some(refs.clone()),
        refs[0].clone(),
        vec!["x".into()],
        Some(vec![ReviewCapsuleCheck::new(check_core, None)]),
        None,
        ReviewCapsuleReviewScope::Full,
        severity(),
        false,
        Some(refs[2].clone()),
        0,
    )
    .unwrap();

    let architecture: &[WorkspaceCheckpointRef] = value.architecture_refs().unwrap();
    let contracts: &[WorkspaceCheckpointRef] = value.contract_refs().unwrap();
    let diff: &WorkspaceCheckpointRef = value.diff();
    let output: &WorkspaceCheckpointRef = value.checks().unwrap()[0].output_ref().unwrap();
    let schema: &WorkspaceCheckpointRef = value.structured_output_schema().unwrap();
    assert_eq!(architecture, refs);
    assert_eq!(contracts, refs);
    assert_eq!(diff, &refs[0]);
    assert_eq!(output, &refs[3]);
    assert_eq!(schema, &refs[2]);
    for (stored, expected_type) in architecture.iter().zip(WorkspaceCheckpointRefType::ALL) {
        assert_eq!(stored.ref_type(), expected_type);
    }
}

#[test]
fn optional_arrays_distinguish_absent_empty_and_values() {
    for state in [
        None,
        Some(vec![]),
        Some(vec!["".into(), " 界 ".into(), "".into()]),
    ] {
        let value = ReviewCapsuleNonTemporalCore::new(
            "r".into(),
            "t".into(),
            "a".into(),
            BASE_SHA.into(),
            IMPLEMENTATION_SHA.into(),
            "x".into(),
            vec![criterion()],
            state.clone(),
            None,
            None,
            reference(WorkspaceCheckpointRefType::RepoPath, "diff"),
            vec!["x".into()],
            None,
            state.clone(),
            ReviewCapsuleReviewScope::Full,
            severity(),
            false,
            None,
            0,
        )
        .unwrap();
        assert_eq!(value.non_goals(), state.as_deref());
        assert_eq!(value.security_requirements(), state.as_deref());
    }

    for empty in [false, true] {
        let refs = Some(if empty {
            vec![]
        } else {
            vec![reference(WorkspaceCheckpointRefType::Url, " u ")]
        });
        let checks = Some(if empty {
            vec![]
        } else {
            vec![ReviewCapsuleCheck::new(
                WorkspaceCheckpointExecutedCheckCore::new(
                    WorkspaceCheckpointCheckSource::WorkerExecution,
                    vec!["x".into()],
                    0,
                    CommitSha::parse(BASE_SHA).unwrap(),
                    None,
                    None,
                )
                .unwrap(),
                None,
            )]
        });
        let value = ReviewCapsuleNonTemporalCore::new(
            "r".into(),
            "t".into(),
            "a".into(),
            BASE_SHA.into(),
            IMPLEMENTATION_SHA.into(),
            "x".into(),
            vec![criterion()],
            None,
            refs.clone(),
            refs.clone(),
            reference(WorkspaceCheckpointRefType::RepoPath, "diff"),
            vec!["x".into()],
            checks.clone(),
            None,
            ReviewCapsuleReviewScope::Full,
            severity(),
            false,
            None,
            0,
        )
        .unwrap();
        assert_eq!(value.architecture_refs(), refs.as_deref());
        assert_eq!(value.contract_refs(), refs.as_deref());
        assert_eq!(value.checks(), checks.as_deref());
    }
}

#[test]
fn allowed_write_paths_are_opaque_ordered_data() {
    let paths = vec!["".into(), " ../界/** ".into(), "x".into(), "".into()];
    let value = capsule(
        "r",
        "t",
        "a",
        BASE_SHA,
        IMPLEMENTATION_SHA,
        "x",
        vec![criterion()],
        paths.clone(),
        severity(),
        0,
    )
    .unwrap();
    assert_eq!(value.allowed_write_paths(), paths);
}

#[test]
fn workspace_check_core_preserves_all_fields_and_argv() {
    assert_eq!(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec![],
            0,
            CommitSha::parse(BASE_SHA).unwrap(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointExecutedCheckCoreError::EmptyCommand)
    );
    let argv = vec![
        "tool".into(),
        "".into(),
        " \t".into(),
        "界".into(),
        "tool".into(),
    ];
    let output = reference(WorkspaceCheckpointRefType::ArtifactId, "output");
    for source in WorkspaceCheckpointCheckSource::ALL {
        for exit_code in [i64::MIN, -1, 0, 1, i64::MAX] {
            for timed_out in [None, Some(false), Some(true)] {
                let workspace_core = WorkspaceCheckpointExecutedCheckCore::new(
                    source,
                    argv.clone(),
                    exit_code,
                    CommitSha::parse(BASE_SHA).unwrap(),
                    timed_out,
                    Some(output.clone()),
                )
                .unwrap();
                let value = ReviewCapsuleCheck::new(workspace_core.clone(), None);
                let physical: &WorkspaceCheckpointExecutedCheckCore = value.core();
                assert_eq!(physical, &workspace_core);
                assert_eq!(value.source(), source);
                assert_eq!(value.command(), argv);
                assert_eq!(value.exit_code(), exit_code);
                assert_eq!(value.code_sha().as_str(), BASE_SHA);
                assert_eq!(value.timed_out(), timed_out);
                assert_eq!(value.output_ref(), Some(&output));
                assert_eq!(value.result(), None);
            }
        }
    }
}

#[test]
fn check_result_vocabulary_is_closed_and_never_inferred() {
    use ReviewCapsuleCheckResult::*;
    let expected = [Pass, Fail, Error, Skipped, Unknown];
    let strings = ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"];
    assert_eq!(ReviewCapsuleCheckResult::ALL, expected);
    assert_eq!(expected.len(), 5);
    for (index, value) in expected.into_iter().enumerate() {
        let exhaustive = match value {
            Pass => "PASS",
            Fail => "FAIL",
            Error => "ERROR",
            Skipped => "SKIPPED",
            Unknown => "UNKNOWN",
        };
        assert_eq!(value.as_str(), strings[index]);
        assert_eq!(exhaustive, strings[index]);
        for other in &expected[index + 1..] {
            assert_ne!(value, *other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
    for exit_code in [-1, 0, 1] {
        for result in std::iter::once(None).chain(expected.map(Some)) {
            let value = ReviewCapsuleCheck::new(
                WorkspaceCheckpointExecutedCheckCore::new(
                    WorkspaceCheckpointCheckSource::ReviewExecution,
                    vec!["x".into()],
                    exit_code,
                    CommitSha::parse(BASE_SHA).unwrap(),
                    None,
                    None,
                )
                .unwrap(),
                result,
            );
            assert_eq!(value.result(), result);
        }
    }
}

#[test]
fn review_scope_vocabulary_is_closed_and_passive() {
    use ReviewCapsuleReviewScope::*;
    let expected = [Full, Security, Regression, RepairVerification];
    let strings = ["FULL", "SECURITY", "REGRESSION", "REPAIR_VERIFICATION"];
    assert_eq!(ReviewCapsuleReviewScope::ALL, expected);
    assert_eq!(expected.len(), 4);
    for (index, value) in expected.into_iter().enumerate() {
        let exhaustive = match value {
            Full => "FULL",
            Security => "SECURITY",
            Regression => "REGRESSION",
            RepairVerification => "REPAIR_VERIFICATION",
        };
        assert_eq!(value.as_str(), strings[index]);
        assert_eq!(exhaustive, strings[index]);
        for other in &expected[index + 1..] {
            assert_ne!(value, *other);
            assert_ne!(value.as_str(), other.as_str());
        }
        let mut capsule = capsule(
            "r",
            "t",
            "a",
            BASE_SHA,
            IMPLEMENTATION_SHA,
            "x",
            vec![criterion()],
            vec!["x".into()],
            severity(),
            0,
        )
        .unwrap();
        // Reconstruct because fields are private and immutable; scope changes no other data.
        capsule = ReviewCapsuleNonTemporalCore::new(
            capsule.review_id().into(),
            capsule.task_id().into(),
            capsule.attempt_id().into(),
            capsule.baseline_sha().as_str().into(),
            capsule.implementation_sha().as_str().into(),
            capsule.objective().into(),
            capsule.acceptance_criteria().to_vec(),
            None,
            None,
            None,
            capsule.diff().clone(),
            capsule.allowed_write_paths().to_vec(),
            None,
            None,
            value,
            capsule.severity_policy().clone(),
            capsule.reproduction_required(),
            None,
            capsule.context_epoch(),
        )
        .unwrap();
        assert_eq!(capsule.review_scope(), value);
    }
}

#[test]
fn severity_policy_uses_opaque_strings_and_preserves_optional_states() {
    assert_eq!(
        ReviewCapsuleSeverityPolicy::new(vec![], None),
        Err(ReviewCapsuleConstructionError::EmptyBlockingCategories)
    );
    let blocking = vec!["FUTURE".into(), "".into(), " 界 ".into(), "FUTURE".into()];
    for nonblocking in [
        None,
        Some(vec![]),
        Some(vec!["".into(), "NEXT".into(), " 界 ".into(), "NEXT".into()]),
    ] {
        let value =
            ReviewCapsuleSeverityPolicy::new(blocking.clone(), nonblocking.clone()).unwrap();
        assert_eq!(value.blocking_categories(), blocking);
        assert_eq!(value.nonblocking_categories(), nonblocking.as_deref());
    }
}

#[test]
fn reproduction_structured_schema_and_context_epoch_are_stored_without_inference() {
    for reproduction_required in [false, true] {
        for schema in [
            None,
            Some(reference(
                WorkspaceCheckpointRefType::StateQuery,
                " schema ",
            )),
        ] {
            for epoch in [0, 1, i64::MAX] {
                let value = ReviewCapsuleNonTemporalCore::new(
                    "r".into(),
                    "t".into(),
                    "a".into(),
                    BASE_SHA.into(),
                    IMPLEMENTATION_SHA.into(),
                    "x".into(),
                    vec![criterion()],
                    None,
                    None,
                    None,
                    reference(WorkspaceCheckpointRefType::RepoPath, "diff"),
                    vec!["x".into()],
                    None,
                    None,
                    ReviewCapsuleReviewScope::Security,
                    severity(),
                    reproduction_required,
                    schema.clone(),
                    epoch,
                )
                .unwrap();
                assert_eq!(value.reproduction_required(), reproduction_required);
                assert_eq!(value.structured_output_schema(), schema.as_ref());
                assert_eq!(value.context_epoch(), epoch);
            }
        }
    }
    assert_eq!(
        capsule(
            "r",
            "t",
            "a",
            BASE_SHA,
            IMPLEMENTATION_SHA,
            "x",
            vec![criterion()],
            vec!["x".into()],
            severity(),
            -1,
        ),
        Err(ReviewCapsuleConstructionError::NegativeContextEpoch)
    );
}

#[test]
fn reconciled_and_deferred_fields_are_absent_from_the_api_boundary() {
    let source = include_str!("review_capsule_structured_core.rs");
    for forbidden_field_or_api in [
        "test_results:",
        "fn test_results",
        "started_at:",
        "fn started_at",
        "finished_at:",
        "fn finished_at",
        "history:",
        "messages:",
        "reasoning:",
        "transcript:",
        "implementer_notes:",
        "agent_context:",
    ] {
        assert!(
            !source.contains(forbidden_field_or_api),
            "unexpected API: {forbidden_field_or_api}"
        );
    }
}
