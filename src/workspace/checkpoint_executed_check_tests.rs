use crate::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointExecutedCheckCoreError, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};

const CODE_SHA_FIXTURE: &str = "0123456789abcdef0123456789abcdef01234567";

fn fixture_sha() -> CommitSha {
    CommitSha::parse(CODE_SHA_FIXTURE).expect("fixture SHA is exactly 40 lowercase hex chars")
}

fn fixture_sha_value(value: &str) -> CommitSha {
    CommitSha::parse(value).expect("test SHA is exactly 40 lowercase hex chars")
}

fn core_fixture(
    source: WorkspaceCheckpointCheckSource,
    command: Vec<String>,
    exit_code: i64,
    code_sha: CommitSha,
    timed_out: Option<bool>,
    output_ref: Option<WorkspaceCheckpointRef>,
) -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        source, command, exit_code, code_sha, timed_out, output_ref,
    )
    .expect("fixture inputs satisfy the core contract")
}

#[test]
fn constructs_normal_core_value_and_exposes_each_accessor() {
    let code_sha = fixture_sha();
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["cargo".to_string(), "test".to_string()],
        0,
        code_sha.clone(),
        None,
        None,
    );

    assert_eq!(
        core.source(),
        WorkspaceCheckpointCheckSource::WorkerExecution
    );
    assert_eq!(core.command(), &["cargo".to_string(), "test".to_string()]);
    assert_eq!(core.exit_code(), 0);
    assert_eq!(core.code_sha(), &code_sha);
    assert_eq!(core.code_sha().as_str(), CODE_SHA_FIXTURE);
    assert_eq!(core.timed_out(), None);
    assert_eq!(core.output_ref(), None);
}

#[test]
fn rejects_empty_command_with_typed_error() {
    let result = WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        Vec::new(),
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(
        result,
        Err(WorkspaceCheckpointExecutedCheckCoreError::EmptyCommand)
    );
}

#[test]
fn accepts_single_argv_element() {
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::BrokerExecution,
        vec!["cargo".to_string()],
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.command(), &["cargo".to_string()]);
}

#[test]
fn preserves_multiple_argv_count_order_and_values() {
    let command = vec![
        "cargo".to_string(),
        "test".to_string(),
        "--locked".to_string(),
        "-p".to_string(),
        "receipts-workspace-execution".to_string(),
    ];
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        command.clone(),
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.command().len(), 5);
    assert_eq!(core.command(), command.as_slice());
}

#[test]
fn preserves_empty_individual_argv_elements() {
    let command = vec!["".to_string(), "arg".to_string(), "".to_string()];
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        command.clone(),
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.command().len(), 3);
    assert_eq!(core.command(), command.as_slice());
    assert_eq!(core.command()[0], "");
    assert_eq!(core.command()[1], "arg");
    assert_eq!(core.command()[2], "");
}

#[test]
fn preserves_whitespace_argv_exactly() {
    let command = vec![
        " ".to_string(),
        "  value  ".to_string(),
        "\t".to_string(),
        "\n".to_string(),
    ];
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        command.clone(),
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.command(), command.as_slice());
}

#[test]
fn preserves_unicode_and_punctuation_argv_as_data() {
    let command = vec![
        "α".to_string(),
        "路径".to_string(),
        "--value=a b".to_string(),
        "$HOME".to_string(),
        "x;y".to_string(),
        "a&&b".to_string(),
    ];
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::GitProvenance,
        command.clone(),
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.command(), command.as_slice());
}

#[test]
fn preserves_every_check_source_variant() {
    for source in WorkspaceCheckpointCheckSource::ALL {
        let core = core_fixture(
            source,
            vec!["cargo".to_string()],
            0,
            fixture_sha(),
            None,
            None,
        );
        assert_eq!(core.source(), source);
    }
    assert_eq!(WorkspaceCheckpointCheckSource::ALL.len(), 4);
}

#[test]
fn preserves_negative_zero_and_positive_exit_codes() {
    for exit_code in [-1_i64, 0, 1] {
        let core = core_fixture(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec!["cargo".to_string()],
            exit_code,
            fixture_sha(),
            None,
            None,
        );
        assert_eq!(core.exit_code(), exit_code);
    }
}

#[test]
fn preserves_code_sha_exactly() {
    let code_sha = fixture_sha_value(CODE_SHA_FIXTURE);
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::ReviewExecution,
        vec!["cargo".to_string()],
        0,
        code_sha,
        None,
        None,
    );

    assert_eq!(core.code_sha().as_str(), CODE_SHA_FIXTURE);
}

#[test]
fn preserves_timed_out_none_false_and_true_without_inference() {
    for timed_out in [None, Some(false), Some(true)] {
        let core = core_fixture(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec!["cargo".to_string()],
            0,
            fixture_sha(),
            timed_out,
            None,
        );
        assert_eq!(core.timed_out(), timed_out);
        assert_eq!(core.exit_code(), 0);
    }
}

#[test]
fn preserves_output_ref_none() {
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["cargo".to_string()],
        0,
        fixture_sha(),
        None,
        None,
    );

    assert_eq!(core.output_ref(), None);
}

#[test]
fn preserves_output_ref_some_structurally_exact() {
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::ArtifactId,
        "artifact-α",
        Some(String::new()),
        Some("  stdout.tail  ".to_string()),
    )
    .expect("the distinctive reference target is non-empty");

    let core = core_fixture(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["cargo".to_string()],
        0,
        fixture_sha(),
        None,
        Some(reference.clone()),
    );

    let observed = core.output_ref().expect("output_ref is Some");
    assert_eq!(observed, &reference);
    assert_eq!(observed.ref_type(), WorkspaceCheckpointRefType::ArtifactId);
    assert_eq!(observed.target(), "artifact-α");
    assert_eq!(observed.digest(), Some(""));
    assert_eq!(observed.section(), Some("  stdout.tail  "));
}

#[test]
fn clone_preserves_all_six_fields_structurally() {
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::ArtifactId,
        "artifact-α",
        Some(String::new()),
        Some("  stdout.tail  ".to_string()),
    )
    .expect("the distinctive reference target is non-empty");
    let core = core_fixture(
        WorkspaceCheckpointCheckSource::ReviewExecution,
        vec!["cargo".to_string(), "test".to_string()],
        1,
        fixture_sha(),
        Some(true),
        Some(reference),
    );

    let cloned = core.clone();
    assert_eq!(cloned, core);
    assert_eq!(cloned.source(), core.source());
    assert_eq!(cloned.command(), core.command());
    assert_eq!(cloned.exit_code(), core.exit_code());
    assert_eq!(cloned.code_sha(), core.code_sha());
    assert_eq!(cloned.timed_out(), core.timed_out());
    assert_eq!(cloned.output_ref(), core.output_ref());
}

#[test]
fn empty_command_error_is_typed_and_standard() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<WorkspaceCheckpointExecutedCheckCoreError>();
    assert_eq!(
        WorkspaceCheckpointExecutedCheckCoreError::EmptyCommand.to_string(),
        "workspace checkpoint executed-check command is empty"
    );
}
