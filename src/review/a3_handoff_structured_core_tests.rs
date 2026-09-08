use super::*;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointExecutedCheckCoreError, WorkspaceCheckpointRef, WorkspaceCheckpointRefError,
    WorkspaceCheckpointRefType,
};

const START: &str = "0123456789abcdef0123456789abcdef01234567";
const FINAL: &str = "89abcdef0123456789abcdef0123456789abcdef";

#[allow(clippy::too_many_arguments)]
fn required(
    task: &str,
    attempt: &str,
    start: &str,
    final_sha: &str,
    branch: &str,
    files: Vec<String>,
    checks: Vec<A3HandoffCheck>,
    ready: bool,
) -> Result<A3HandoffNonTemporalCore, A3HandoffConstructionError> {
    A3HandoffNonTemporalCore::new(
        task.into(),
        attempt.into(),
        start.into(),
        final_sha.into(),
        branch.into(),
        files,
        checks,
        ready,
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
}

fn reference(kind: WorkspaceCheckpointRefType) -> WorkspaceCheckpointRef {
    WorkspaceCheckpointRef::new(
        kind,
        " unresolved 界 ",
        Some("".into()),
        Some(" section ".into()),
    )
    .unwrap()
}

fn check(result: Option<A3HandoffCheckResult>) -> A3HandoffCheck {
    A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::ReviewExecution,
            vec!["nonexistent-command".into()],
            137,
            CommitSha::parse(START).unwrap(),
            Some(true),
            None,
        )
        .unwrap(),
        result,
    )
}

#[test]
fn required_record_preserves_all_fields_and_omits_every_optional_property() {
    let checks = vec![check(None)];
    let value = required(
        "task",
        "attempt",
        START,
        FINAL,
        "branch",
        vec!["file".into()],
        checks.clone(),
        true,
    )
    .unwrap();
    assert_eq!(value.task_id(), "task");
    assert_eq!(value.attempt_id(), "attempt");
    let start: &CommitSha = value.start_sha();
    let final_sha: &CommitSha = value.final_sha();
    assert_eq!(start.as_str(), START);
    assert_eq!(final_sha.as_str(), FINAL);
    assert_eq!(value.branch(), "branch");
    assert_eq!(value.files_changed(), &["file"]);
    assert_eq!(value.checks(), checks);
    assert!(value.ready_for_a4());
    assert_eq!(value.implementer(), None);
    assert_eq!(value.files_created(), None);
    assert_eq!(value.files_deleted(), None);
    assert_eq!(value.contracts_consumed(), None);
    assert_eq!(value.implementation_summary(), None);
    assert_eq!(value.not_run(), None);
    assert_eq!(value.evidence_labels(), None);
    assert_eq!(value.known_limitations(), None);
    assert_eq!(value.assumptions(), None);
    assert_eq!(value.security_notes(), None);
    assert_eq!(value.subtask_requests(), None);
    assert_eq!(value.open_questions(), None);
    assert_eq!(value.evidence_refs(), None);
    assert_eq!(value.blocker(), None);
}

#[test]
fn task_and_attempt_boundaries_count_unicode_and_preserve_whitespace() {
    use A3HandoffConstructionError::*;
    for position in 0..2 {
        for input in [
            "界".into(),
            "界".repeat(200),
            " \t\n".into(),
            " e\u{301} ".into(),
        ] {
            let mut ids = ["t", "a"];
            ids[position] = &input;
            let value = required(ids[0], ids[1], START, FINAL, "", vec![], vec![], false).unwrap();
            assert_eq!([value.task_id(), value.attempt_id()][position], input);
        }
        for (input, expected) in [
            (String::new(), [EmptyTaskId, EmptyAttemptId][position]),
            (
                "界".repeat(201),
                [TaskIdTooLong, AttemptIdTooLong][position],
            ),
        ] {
            let mut ids = ["t", "a"];
            ids[position] = &input;
            assert_eq!(
                required(ids[0], ids[1], START, FINAL, "", vec![], vec![], false),
                Err(expected)
            );
        }
    }
}

#[test]
fn both_shas_reject_every_malformed_or_symbolic_form_without_normalization() {
    for position in 0..2 {
        for invalid in [
            "a".repeat(39),
            "a".repeat(41),
            "A".repeat(40),
            "g".repeat(40),
            format!(" {START}"),
            format!("{START} "),
            "HEAD".into(),
            "main".into(),
            "refs/heads/topic".into(),
            "v1.0".into(),
            "abcdef0".into(),
            "ａ".repeat(40),
            String::new(),
        ] {
            let mut shas = [START, FINAL];
            shas[position] = &invalid;
            assert_eq!(
                required("t", "a", shas[0], shas[1], "", vec![], vec![], true),
                Err([
                    A3HandoffConstructionError::MalformedStartSha,
                    A3HandoffConstructionError::MalformedFinalSha
                ][position])
            );
            assert!(CommitSha::parse(&invalid).is_err());
        }
    }
}

#[test]
fn empty_branch_files_checks_and_opaque_ordered_paths_are_valid() {
    for branch in ["", " \t\n", " 分支 e\u{301} "] {
        for files in [
            vec![],
            vec!["".into()],
            vec!["z".into(), " 界 ".into(), "z".into(), "".into()],
        ] {
            let value =
                required("t", "a", START, FINAL, branch, files.clone(), vec![], true).unwrap();
            assert_eq!(value.branch(), branch);
            assert_eq!(value.files_changed(), files);
            assert_eq!(value.checks(), &[]);
            assert!(value.ready_for_a4());
        }
    }
}

#[test]
fn workspace_check_core_enforces_only_argv_cardinality_and_preserves_evidence() {
    assert_eq!(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec![],
            0,
            CommitSha::parse(START).unwrap(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointExecutedCheckCoreError::EmptyCommand)
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
            for exit_code in [i64::MIN, -1, 0, 1, 124, 137, i64::MAX] {
                for timed_out in [None, Some(false), Some(true)] {
                    for output in [None, Some(reference(WorkspaceCheckpointRefType::Url))] {
                        for result in
                            std::iter::once(None).chain(A3HandoffCheckResult::ALL.map(Some))
                        {
                            let core = WorkspaceCheckpointExecutedCheckCore::new(
                                source,
                                argv.clone(),
                                exit_code,
                                CommitSha::parse(START).unwrap(),
                                timed_out,
                                output.clone(),
                            )
                            .unwrap();
                            let value = A3HandoffCheck::new(core.clone(), result);
                            let reused: &WorkspaceCheckpointExecutedCheckCore = value.core();
                            assert_eq!(reused, &core);
                            assert_eq!(value.source(), source);
                            assert_eq!(value.command(), argv);
                            assert_eq!(value.exit_code(), exit_code);
                            assert_eq!(value.code_sha().as_str(), START);
                            assert_eq!(value.timed_out(), timed_out);
                            assert_eq!(value.output_ref(), output.as_ref());
                            assert_eq!(value.result(), result);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn check_results_have_exact_closed_vocabulary() {
    use A3HandoffCheckResult::*;
    assert_eq!(
        A3HandoffCheckResult::ALL,
        [Pass, Fail, Error, Skipped, Unknown]
    );
    assert_eq!(
        A3HandoffCheckResult::ALL.map(|v| v.as_str()),
        ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]
    );
    for result in A3HandoffCheckResult::ALL {
        assert_eq!(
            result.as_str(),
            match result {
                Pass => "PASS",
                Fail => "FAIL",
                Error => "ERROR",
                Skipped => "SKIPPED",
                Unknown => "UNKNOWN",
            }
        );
    }
}

#[test]
fn evidence_labels_have_exact_closed_vocabulary_and_preserve_claims() {
    use A3HandoffEvidenceLabel::*;
    assert_eq!(
        A3HandoffEvidenceLabel::ALL,
        [Implemented, Tested, NotTested, Blocked, Assumed]
    );
    assert_eq!(
        A3HandoffEvidenceLabel::ALL.map(|v| v.as_str()),
        ["IMPLEMENTED", "TESTED", "NOT_TESTED", "BLOCKED", "ASSUMED"]
    );
    for label in A3HandoffEvidenceLabel::ALL {
        assert_eq!(
            label.as_str(),
            match label {
                Implemented => "IMPLEMENTED",
                Tested => "TESTED",
                NotTested => "NOT_TESTED",
                Blocked => "BLOCKED",
                Assumed => "ASSUMED",
            }
        );
        for claim in ["", " 界 e\u{301} "] {
            let value = A3HandoffLabeledEvidence::new(claim.into(), label);
            assert_eq!(value.claim(), claim);
            assert_eq!(value.label(), label);
        }
    }
}

#[test]
fn implementer_empty_independent_and_all_fields_preserve_exact_strings() {
    for mask in 0..16 {
        for input in [
            "界".into(),
            "界".repeat(200),
            " \t\n".into(),
            " future/e\u{301} ".into(),
        ] {
            let fields: [Option<String>; 4] =
                std::array::from_fn(|i| (mask & (1 << i) != 0).then(|| input.clone()));
            let value = A3HandoffImplementer::new(
                fields[0].clone(),
                fields[1].clone(),
                fields[2].clone(),
                fields[3].clone(),
            )
            .unwrap();
            assert_eq!(
                [
                    value.provider_id(),
                    value.model_id(),
                    value.runtime_id(),
                    value.binding_id()
                ],
                fields.each_ref().map(|v| v.as_deref())
            );
        }
    }
    let long = "界".repeat(201);
    assert!(
        A3HandoffImplementer::new(Some(long.clone()), Some(long.clone()), Some(long), None).is_ok()
    );
}

#[test]
fn implementer_applies_only_machine_length_constraints() {
    use A3HandoffConstructionError::*;
    for (position, expected) in [
        EmptyProviderId,
        EmptyModelId,
        EmptyRuntimeId,
        EmptyBindingId,
    ]
    .into_iter()
    .enumerate()
    {
        let mut fields = [None, None, None, None];
        fields[position] = Some(String::new());
        let [p, m, r, b] = fields;
        assert_eq!(A3HandoffImplementer::new(p, m, r, b), Err(expected));
    }
    assert_eq!(
        A3HandoffImplementer::new(None, None, None, Some("界".repeat(201))),
        Err(BindingIdTooLong)
    );
}

#[test]
fn optional_arrays_and_text_preserve_absent_empty_and_ordered_duplicate_values() {
    for strings in [
        None,
        Some(vec![]),
        Some(vec!["".into(), " 界 e\u{301} ".into(), "".into()]),
    ] {
        for text in [None, Some(String::new()), Some(" 界 e\u{301} ".into())] {
            let value = A3HandoffNonTemporalCore::new(
                "t".into(),
                "a".into(),
                START.into(),
                FINAL.into(),
                "".into(),
                vec![],
                vec![],
                true,
                None,
                strings.clone(),
                strings.clone(),
                None,
                text.clone(),
                strings.clone(),
                None,
                strings.clone(),
                strings.clone(),
                text.clone(),
                None,
                strings.clone(),
                None,
                None,
            )
            .unwrap();
            assert_eq!(value.files_created(), strings.as_deref());
            assert_eq!(value.files_deleted(), strings.as_deref());
            assert_eq!(value.not_run(), strings.as_deref());
            assert_eq!(value.known_limitations(), strings.as_deref());
            assert_eq!(value.assumptions(), strings.as_deref());
            assert_eq!(value.open_questions(), strings.as_deref());
            assert_eq!(value.implementation_summary(), text.as_deref());
            assert_eq!(value.security_notes(), text.as_deref());
        }
    }
}

#[test]
fn subtask_request_boundaries_preserve_absence_empty_order_and_duplicates() {
    use A3HandoffConstructionError::*;
    for requests in [
        None,
        Some(vec![]),
        Some(vec![
            "界".into(),
            "界".repeat(200),
            " \t".into(),
            "界".into(),
        ]),
        Some(vec!["ok".into(), "".into()]),
        Some(vec!["ok".into(), "界".repeat(201)]),
    ] {
        let result = A3HandoffNonTemporalCore::new(
            "t".into(),
            "a".into(),
            START.into(),
            FINAL.into(),
            "".into(),
            vec![],
            vec![],
            false,
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
            requests.clone(),
            None,
            None,
            None,
        );
        match &requests {
            Some(values) if values.iter().any(String::is_empty) => {
                assert_eq!(result, Err(EmptySubtaskRequest))
            }
            Some(values) if values.iter().any(|v| v.chars().count() > 200) => {
                assert_eq!(result, Err(SubtaskRequestTooLong))
            }
            _ => assert_eq!(result.unwrap().subtask_requests(), requests.as_deref()),
        }
    }
}

#[test]
fn optional_nested_records_preserve_empty_objects_arrays_duplicates_and_refs() {
    let contract = A3HandoffContractConsumed::new("".into(), " non-semver 界 ".into());
    assert_eq!(contract.contract_id(), "");
    assert_eq!(contract.version(), " non-semver 界 ");
    let other = A3HandoffContractConsumed::new(" c ".into(), "".into());
    assert_eq!(other.version(), "");
    let refs: Vec<_> = WorkspaceCheckpointRefType::ALL
        .into_iter()
        .map(reference)
        .collect();
    let labels: Vec<_> = A3HandoffEvidenceLabel::ALL
        .into_iter()
        .map(|label| A3HandoffLabeledEvidence::new("".into(), label))
        .collect();
    for empty in [true, false] {
        let contracts = if empty {
            vec![]
        } else {
            vec![contract.clone(), other.clone(), contract.clone()]
        };
        let evidence = if empty { vec![] } else { refs.clone() };
        let labels = if empty {
            vec![]
        } else {
            labels
                .iter()
                .cloned()
                .chain(labels.iter().cloned())
                .collect()
        };
        let implementer = A3HandoffImplementer::new(None, None, None, None).unwrap();
        let value = A3HandoffNonTemporalCore::new(
            "t".into(),
            "a".into(),
            START.into(),
            FINAL.into(),
            "".into(),
            vec![],
            vec![],
            false,
            Some(implementer.clone()),
            None,
            None,
            Some(contracts.clone()),
            None,
            None,
            Some(labels.clone()),
            None,
            None,
            None,
            None,
            None,
            Some(evidence.clone()),
            None,
        )
        .unwrap();
        assert_eq!(value.implementer(), Some(&implementer));
        assert_eq!(value.contracts_consumed(), Some(contracts.as_slice()));
        assert_eq!(value.evidence_labels(), Some(labels.as_slice()));
        let stored: &[WorkspaceCheckpointRef] = value.evidence_refs().unwrap();
        assert_eq!(stored, evidence);
    }
    for r in refs {
        let value = A3HandoffCheck::new(
            WorkspaceCheckpointExecutedCheckCore::new(
                WorkspaceCheckpointCheckSource::GitProvenance,
                vec!["x".into()],
                0,
                CommitSha::parse(START).unwrap(),
                None,
                Some(r.clone()),
            )
            .unwrap(),
            None,
        );
        assert_eq!(value.output_ref(), Some(&r));
        assert_eq!(r.target(), " unresolved 界 ");
        assert_eq!(r.digest(), Some(""));
        assert_eq!(r.section(), Some(" section "));
    }
    assert_eq!(
        WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::RepoPath, "", None, None),
        Err(WorkspaceCheckpointRefError::EmptyTarget)
    );
}

#[test]
fn all_four_blocker_states_are_distinct_and_readiness_is_never_inferred() {
    let states = [
        None,
        Some(A3HandoffBlocker::ExplicitNull),
        Some(A3HandoffBlocker::Text("".into())),
        Some(A3HandoffBlocker::Text(" blocked 界 ".into())),
    ];
    for (index, blocker) in states.iter().enumerate() {
        for other in &states[index + 1..] {
            assert_ne!(blocker, other);
        }
        for checks in [
            vec![],
            vec![check(Some(A3HandoffCheckResult::Pass))],
            vec![
                check(Some(A3HandoffCheckResult::Fail)),
                check(None),
                check(Some(A3HandoffCheckResult::Error)),
            ],
        ] {
            for ready in [true, false] {
                let value = A3HandoffNonTemporalCore::new(
                    "t".into(),
                    "a".into(),
                    FINAL.into(),
                    START.into(),
                    "".into(),
                    vec![],
                    checks.clone(),
                    ready,
                    None,
                    Some(vec!["not in files_changed".into()]),
                    Some(vec!["same".into()]),
                    None,
                    Some(" summary ".into()),
                    Some(vec!["not run".into()]),
                    Some(vec![A3HandoffLabeledEvidence::new(
                        "blocked".into(),
                        A3HandoffEvidenceLabel::Blocked,
                    )]),
                    Some(vec!["limit".into()]),
                    Some(vec!["assume".into()]),
                    Some("security notes".into()),
                    None,
                    None,
                    None,
                    blocker.clone(),
                )
                .unwrap();
                assert_eq!(value.blocker(), blocker.as_ref());
                assert_eq!(value.ready_for_a4(), ready);
                assert_eq!(value.checks(), checks);
                assert_eq!(value.start_sha().as_str(), FINAL);
                assert_eq!(value.final_sha().as_str(), START);
            }
        }
    }
}
