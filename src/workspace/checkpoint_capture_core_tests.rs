use crate::{
    CommitSha, WorkspaceCheckpointCaptureCore, WorkspaceCheckpointCaptureCoreError,
    WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointKind,
    WorkspaceCheckpointRef, WorkspaceCheckpointRefType, WorkspaceRecoveryDecision,
};

const HEAD_SHA_FIXTURE: &str = "0123456789abcdef0123456789abcdef01234567";
const BASE_SHA_FIXTURE: &str = "abcdef0123456789abcdef0123456789abcdef01";

fn fixture_head_sha() -> CommitSha {
    CommitSha::parse(HEAD_SHA_FIXTURE).expect("fixture head SHA is 40 lowercase hex chars")
}

fn fixture_base_sha() -> CommitSha {
    CommitSha::parse(BASE_SHA_FIXTURE).expect("fixture base SHA is 40 lowercase hex chars")
}

#[allow(clippy::too_many_arguments)]
fn build_core(
    checkpoint_id: &str,
    workspace_id: &str,
    task_id: Option<String>,
    attempt_id: Option<String>,
    kind: WorkspaceCheckpointKind,
    head_sha: CommitSha,
    base_sha: Option<CommitSha>,
    dirty_diff_ref: Option<WorkspaceCheckpointRef>,
    modified_files: Vec<String>,
    untracked_files: Vec<String>,
    executed_checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
    recovery_decision: Option<WorkspaceRecoveryDecision>,
    recovery_rationale: Option<String>,
) -> Result<WorkspaceCheckpointCaptureCore, WorkspaceCheckpointCaptureCoreError> {
    WorkspaceCheckpointCaptureCore::new(
        checkpoint_id.to_string(),
        workspace_id.to_string(),
        task_id,
        attempt_id,
        kind,
        head_sha,
        base_sha,
        dirty_diff_ref,
        modified_files,
        untracked_files,
        executed_checks,
        recovery_decision,
        recovery_rationale,
    )
}

fn minimal_core() -> WorkspaceCheckpointCaptureCore {
    build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("minimal fixture satisfies the capture contract")
}

fn fixture_check(
    source: WorkspaceCheckpointCheckSource,
    command: Vec<&str>,
    exit_code: i64,
) -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        source,
        command.into_iter().map(str::to_string).collect(),
        exit_code,
        fixture_head_sha(),
        None,
        None,
    )
    .expect("fixture check satisfies the executed-check contract")
}

#[test]
fn rejects_empty_checkpoint_id() {
    let result = build_core(
        "",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    );
    assert_eq!(
        result,
        Err(WorkspaceCheckpointCaptureCoreError::EmptyCheckpointId)
    );
}

#[test]
fn rejects_empty_workspace_id() {
    let result = build_core(
        "checkpoint-1",
        "",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    );
    assert_eq!(
        result,
        Err(WorkspaceCheckpointCaptureCoreError::EmptyWorkspaceId)
    );
}

#[test]
fn rejects_empty_optional_ids_without_coercing_to_none() {
    let task_result = build_core(
        "checkpoint-1",
        "workspace-1",
        Some(String::new()),
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    );
    assert_eq!(
        task_result,
        Err(WorkspaceCheckpointCaptureCoreError::EmptyTaskId)
    );

    let attempt_result = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        Some(String::new()),
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    );
    assert_eq!(
        attempt_result,
        Err(WorkspaceCheckpointCaptureCoreError::EmptyAttemptId)
    );
}

#[test]
fn accepts_none_optional_ids() {
    let core = minimal_core();
    assert_eq!(core.task_id(), None);
    assert_eq!(core.attempt_id(), None);
}

#[test]
fn accepts_whitespace_only_ids_and_preserves_them() {
    let core = build_core(
        " ",
        " ",
        Some(" ".to_string()),
        Some(" ".to_string()),
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("single-space IDs are length 1 and valid");
    assert_eq!(core.checkpoint_id(), " ");
    assert_eq!(core.workspace_id(), " ");
    assert_eq!(core.task_id(), Some(" "));
    assert_eq!(core.attempt_id(), Some(" "));
}

#[test]
fn accepts_200_char_ascii_ids_for_every_id_field() {
    let id_200 = "a".repeat(200);
    let core = build_core(
        &id_200,
        &id_200,
        Some(id_200.clone()),
        Some(id_200.clone()),
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("200-character IDs are valid");
    assert_eq!(core.checkpoint_id(), id_200);
    assert_eq!(core.workspace_id(), id_200);
    assert_eq!(core.task_id(), Some(id_200.as_str()));
    assert_eq!(core.attempt_id(), Some(id_200.as_str()));
}

#[test]
fn rejects_201_char_ascii_ids_with_field_specific_errors() {
    let id_201 = "a".repeat(201);

    assert_eq!(
        build_core(
            &id_201,
            "workspace-1",
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::CheckpointIdTooLong)
    );

    assert_eq!(
        build_core(
            "checkpoint-1",
            &id_201,
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::WorkspaceIdTooLong)
    );

    assert_eq!(
        build_core(
            "checkpoint-1",
            "workspace-1",
            Some(id_201.clone()),
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::TaskIdTooLong)
    );

    assert_eq!(
        build_core(
            "checkpoint-1",
            "workspace-1",
            None,
            Some(id_201.clone()),
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::AttemptIdTooLong)
    );
}

#[test]
fn unicode_200_chars_accepted_for_every_id_field() {
    let uni_200 = "界".repeat(200);
    assert_eq!(uni_200.chars().count(), 200);
    let core = build_core(
        &uni_200,
        &uni_200,
        Some(uni_200.clone()),
        Some(uni_200.clone()),
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("200 Unicode chars (600 UTF-8 bytes) must be accepted");
    assert_eq!(core.checkpoint_id(), uni_200);
    assert_eq!(core.workspace_id(), uni_200);
    assert_eq!(core.task_id(), Some(uni_200.as_str()));
    assert_eq!(core.attempt_id(), Some(uni_200.as_str()));
}

#[test]
fn unicode_201_chars_rejected_for_every_id_field() {
    let uni_201 = "界".repeat(201);
    assert_eq!(uni_201.chars().count(), 201);

    assert_eq!(
        build_core(
            &uni_201,
            "workspace-1",
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::CheckpointIdTooLong)
    );
    assert_eq!(
        build_core(
            "checkpoint-1",
            &uni_201,
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::WorkspaceIdTooLong)
    );
    assert_eq!(
        build_core(
            "checkpoint-1",
            "workspace-1",
            Some(uni_201.clone()),
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::TaskIdTooLong)
    );
    assert_eq!(
        build_core(
            "checkpoint-1",
            "workspace-1",
            None,
            Some(uni_201.clone()),
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ),
        Err(WorkspaceCheckpointCaptureCoreError::AttemptIdTooLong)
    );
}

#[test]
fn preserves_ids_exactly_without_trim_or_normalization() {
    let checkpoint = "  checkpoint-α  ";
    let workspace = "  workspace-α  ";
    let task = "  task-α  ".to_string();
    let attempt = "  attempt-α  ".to_string();
    let core = build_core(
        checkpoint,
        workspace,
        Some(task.clone()),
        Some(attempt.clone()),
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("padded Unicode IDs are valid");
    assert_eq!(core.checkpoint_id(), checkpoint);
    assert_eq!(core.workspace_id(), workspace);
    assert_eq!(core.task_id(), Some(task.as_str()));
    assert_eq!(core.attempt_id(), Some(attempt.as_str()));
}

#[test]
fn preserves_kind_values() {
    for kind in WorkspaceCheckpointKind::ALL {
        let core = build_core(
            "checkpoint-1",
            "workspace-1",
            None,
            None,
            kind,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        )
        .expect("every kind is valid");
        assert_eq!(core.kind(), kind);
    }
}

#[test]
fn preserves_head_sha_exactly() {
    let head = fixture_head_sha();
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::PreTermination,
        head.clone(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("head SHA fixture is valid");
    assert_eq!(core.head_sha(), &head);
    assert_eq!(core.head_sha().as_str(), HEAD_SHA_FIXTURE);
}

#[test]
fn preserves_base_sha_none() {
    let core = minimal_core();
    assert_eq!(core.base_sha(), None);
}

#[test]
fn preserves_base_sha_some_exactly_without_relationship_inference() {
    let base = fixture_base_sha();
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        Some(base.clone()),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("explicit base SHA is valid");
    assert_eq!(core.base_sha(), Some(&base));
    assert_eq!(
        core.base_sha().expect("base is Some").as_str(),
        BASE_SHA_FIXTURE
    );
}

#[test]
fn preserves_base_sha_equal_to_head_without_rejection() {
    let head = fixture_head_sha();
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::RecoveryCapture,
        head.clone(),
        Some(head.clone()),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("base equal to head is not rejected");
    assert_eq!(core.base_sha(), Some(&head));
    assert_eq!(core.head_sha(), &head);
}

#[test]
fn accepts_empty_file_arrays() {
    let core = minimal_core();
    assert_eq!(core.modified_files(), &[] as &[String]);
    assert_eq!(core.untracked_files(), &[] as &[String]);
}

#[test]
fn preserves_modified_files_exactly_with_duplicates_order_and_edge_entries() {
    let files = vec![
        "".to_string(),
        "same".to_string(),
        "same".to_string(),
        "  path  ".to_string(),
        "路径".to_string(),
    ];
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        files.clone(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("edge-case file entries are preserved");
    assert_eq!(core.modified_files().len(), 5);
    assert_eq!(core.modified_files(), files.as_slice());
}

#[test]
fn preserves_untracked_files_exactly_with_duplicates_order_and_edge_entries() {
    let files = vec![
        "".to_string(),
        "same".to_string(),
        "same".to_string(),
        "  path  ".to_string(),
        "路径".to_string(),
    ];
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        files.clone(),
        Vec::new(),
        None,
        None,
    )
    .expect("edge-case file entries are preserved");
    assert_eq!(core.untracked_files().len(), 5);
    assert_eq!(core.untracked_files(), files.as_slice());
}

#[test]
fn preserves_dirty_diff_ref_none() {
    let core = minimal_core();
    assert_eq!(core.dirty_diff_ref(), None);
}

#[test]
fn preserves_dirty_diff_ref_some_structurally_exact() {
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::StateQuery,
        "state-query-α",
        Some("digest-α".to_string()),
        Some("  section-α  ".to_string()),
    )
    .expect("distinctive reference target is non-empty");
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        Some(reference.clone()),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("dirty diff ref is stored exactly");
    let observed = core.dirty_diff_ref().expect("dirty diff ref is Some");
    assert_eq!(observed, &reference);
    assert_eq!(observed.ref_type(), WorkspaceCheckpointRefType::StateQuery);
    assert_eq!(observed.target(), "state-query-α");
    assert_eq!(observed.digest(), Some("digest-α"));
    assert_eq!(observed.section(), Some("  section-α  "));
}

#[test]
fn accepts_empty_executed_checks() {
    let core = minimal_core();
    assert_eq!(
        core.executed_checks(),
        &[] as &[WorkspaceCheckpointExecutedCheckCore]
    );
}

#[test]
fn preserves_executed_checks_values_and_order() {
    let first = fixture_check(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["cargo", "test"],
        0,
    );
    let second = fixture_check(
        WorkspaceCheckpointCheckSource::GitProvenance,
        vec!["git", "status", "--porcelain"],
        1,
    );
    let checks = vec![first.clone(), second.clone()];
    let core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        checks,
        None,
        None,
    )
    .expect("executed checks are stored exactly");
    assert_eq!(core.executed_checks().len(), 2);
    assert_eq!(core.executed_checks(), &[first, second]);
}

#[test]
fn preserves_recovery_decision_none() {
    let core = minimal_core();
    assert_eq!(core.recovery_decision(), None);
}

#[test]
fn preserves_every_recovery_decision_value() {
    for decision in WorkspaceRecoveryDecision::ALL {
        let core = build_core(
            "checkpoint-1",
            "workspace-1",
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            fixture_head_sha(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(decision),
            None,
        )
        .expect("every recovery decision is stored without execution");
        assert_eq!(core.recovery_decision(), Some(decision));
    }
    assert_eq!(WorkspaceRecoveryDecision::ALL.len(), 3);
}

#[test]
fn preserves_recovery_rationale_none_empty_and_exact() {
    let none_core = minimal_core();
    assert_eq!(none_core.recovery_rationale(), None);

    let empty_core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Some(String::new()),
    )
    .expect("empty rationale is allowed");
    assert_eq!(empty_core.recovery_rationale(), Some(""));

    let exact = "  rationale α  ".to_string();
    let exact_core = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Some(exact.clone()),
    )
    .expect("rationale is preserved exactly");
    assert_eq!(exact_core.recovery_rationale(), Some(exact.as_str()));
}

#[test]
fn accepts_unrelated_optional_combinations_without_relational_validation() {
    let rationale_only = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Some("orphan rationale".to_string()),
    )
    .expect("rationale without decision is valid");
    assert_eq!(rationale_only.recovery_decision(), None);
    assert_eq!(
        rationale_only.recovery_rationale(),
        Some("orphan rationale")
    );

    let decision_only = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Some(WorkspaceRecoveryDecision::ResetToLastAccepted),
        None,
    )
    .expect("decision without rationale is valid");
    assert_eq!(
        decision_only.recovery_decision(),
        Some(WorkspaceRecoveryDecision::ResetToLastAccepted)
    );
    assert_eq!(decision_only.recovery_rationale(), None);

    let files_without_ref = build_core(
        "checkpoint-1",
        "workspace-1",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        fixture_head_sha(),
        None,
        None,
        vec!["changed.rs".to_string()],
        Vec::new(),
        Vec::new(),
        None,
        None,
    )
    .expect("modified files without a dirty diff ref are valid");
    assert_eq!(
        files_without_ref.modified_files(),
        &["changed.rs".to_string()]
    );
    assert_eq!(files_without_ref.dirty_diff_ref(), None);
}

#[test]
fn clone_preserves_full_capture_structurally() {
    let dirty_ref = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::ArtifactId,
        "artifact-α",
        Some("digest".to_string()),
        Some("section".to_string()),
    )
    .expect("reference fixture is valid");
    let checks = vec![
        fixture_check(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec!["cargo", "test"],
            0,
        ),
        fixture_check(
            WorkspaceCheckpointCheckSource::ReviewExecution,
            vec!["cargo", "clippy"],
            2,
        ),
    ];
    let core = build_core(
        "  checkpoint-α  ",
        "  workspace-α  ",
        Some("task-α".to_string()),
        Some("attempt-α".to_string()),
        WorkspaceCheckpointKind::RecoveryCapture,
        fixture_head_sha(),
        Some(fixture_base_sha()),
        Some(dirty_ref),
        vec!["a".to_string(), "a".to_string(), "".to_string()],
        vec!["u".to_string()],
        checks,
        Some(WorkspaceRecoveryDecision::InspectAndSalvage),
        Some("  rationale α  ".to_string()),
    )
    .expect("full capture fixture is valid");

    let cloned = core.clone();
    assert_eq!(cloned, core);
    assert_eq!(cloned.checkpoint_id(), core.checkpoint_id());
    assert_eq!(cloned.workspace_id(), core.workspace_id());
    assert_eq!(cloned.task_id(), core.task_id());
    assert_eq!(cloned.attempt_id(), core.attempt_id());
    assert_eq!(cloned.kind(), core.kind());
    assert_eq!(cloned.head_sha(), core.head_sha());
    assert_eq!(cloned.base_sha(), core.base_sha());
    assert_eq!(cloned.dirty_diff_ref(), core.dirty_diff_ref());
    assert_eq!(cloned.modified_files(), core.modified_files());
    assert_eq!(cloned.untracked_files(), core.untracked_files());
    assert_eq!(cloned.executed_checks(), core.executed_checks());
    assert_eq!(cloned.recovery_decision(), core.recovery_decision());
    assert_eq!(cloned.recovery_rationale(), core.recovery_rationale());
}

#[test]
fn capture_errors_are_typed_and_standard() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<WorkspaceCheckpointCaptureCoreError>();

    for error in [
        WorkspaceCheckpointCaptureCoreError::EmptyCheckpointId,
        WorkspaceCheckpointCaptureCoreError::CheckpointIdTooLong,
        WorkspaceCheckpointCaptureCoreError::EmptyWorkspaceId,
        WorkspaceCheckpointCaptureCoreError::WorkspaceIdTooLong,
        WorkspaceCheckpointCaptureCoreError::EmptyTaskId,
        WorkspaceCheckpointCaptureCoreError::TaskIdTooLong,
        WorkspaceCheckpointCaptureCoreError::EmptyAttemptId,
        WorkspaceCheckpointCaptureCoreError::AttemptIdTooLong,
    ] {
        assert!(!error.to_string().is_empty());
        let cloned = error;
        assert_eq!(cloned, error);
    }

    assert_eq!(
        WorkspaceCheckpointCaptureCoreError::EmptyCheckpointId.to_string(),
        "workspace checkpoint capture checkpoint_id is empty"
    );
    assert_eq!(
        WorkspaceCheckpointCaptureCoreError::EmptyWorkspaceId.to_string(),
        "workspace checkpoint capture workspace_id is empty"
    );
    assert_eq!(
        WorkspaceCheckpointCaptureCoreError::EmptyTaskId.to_string(),
        "workspace checkpoint capture task_id is empty"
    );
    assert_eq!(
        WorkspaceCheckpointCaptureCoreError::EmptyAttemptId.to_string(),
        "workspace checkpoint capture attempt_id is empty"
    );
}
