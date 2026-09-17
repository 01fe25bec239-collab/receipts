use crate::{
    WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckResult,
    WorkspaceCheckpointRefType,
};

fn assert_check_source_exhaustive(value: WorkspaceCheckpointCheckSource) {
    match value {
        WorkspaceCheckpointCheckSource::WorkerExecution
        | WorkspaceCheckpointCheckSource::BrokerExecution
        | WorkspaceCheckpointCheckSource::ReviewExecution
        | WorkspaceCheckpointCheckSource::GitProvenance => {}
    }
}

fn assert_ref_type_exhaustive(value: WorkspaceCheckpointRefType) {
    match value {
        WorkspaceCheckpointRefType::RepoPath
        | WorkspaceCheckpointRefType::StateQuery
        | WorkspaceCheckpointRefType::ArtifactId
        | WorkspaceCheckpointRefType::Url => {}
    }
}

#[test]
fn check_source_vocabulary_is_exact_ordered_and_unique() {
    use WorkspaceCheckpointCheckSource::{
        BrokerExecution, GitProvenance, ReviewExecution, WorkerExecution,
    };

    assert_eq!(
        WorkspaceCheckpointCheckSource::ALL,
        [
            WorkerExecution,
            BrokerExecution,
            ReviewExecution,
            GitProvenance
        ]
    );
    assert_eq!(
        WorkspaceCheckpointCheckSource::ALL.map(WorkspaceCheckpointCheckSource::as_str),
        [
            "WORKER_EXECUTION",
            "BROKER_EXECUTION",
            "REVIEW_EXECUTION",
            "GIT_PROVENANCE"
        ]
    );
    for (index, value) in WorkspaceCheckpointCheckSource::ALL.iter().enumerate() {
        assert_check_source_exhaustive(*value);
        for other in &WorkspaceCheckpointCheckSource::ALL[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}

#[test]
fn ref_type_vocabulary_is_exact_ordered_and_unique() {
    use WorkspaceCheckpointRefType::{ArtifactId, RepoPath, StateQuery, Url};

    assert_eq!(
        WorkspaceCheckpointRefType::ALL,
        [RepoPath, StateQuery, ArtifactId, Url]
    );
    assert_eq!(
        WorkspaceCheckpointRefType::ALL.map(WorkspaceCheckpointRefType::as_str),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    for (index, value) in WorkspaceCheckpointRefType::ALL.iter().enumerate() {
        assert_ref_type_exhaustive(*value);
        for other in &WorkspaceCheckpointRefType::ALL[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}

#[test]
fn checkpoint_evidence_vocabularies_contain_exactly_thirteen_values() {
    assert_eq!(
        WorkspaceCheckpointCheckSource::ALL.len()
            + WorkspaceCheckpointRefType::ALL.len()
            + WorkspaceCheckpointExecutedCheckResult::ALL.len(),
        13
    );
}

fn assert_executed_check_result_exhaustive(value: WorkspaceCheckpointExecutedCheckResult) {
    match value {
        WorkspaceCheckpointExecutedCheckResult::Pass
        | WorkspaceCheckpointExecutedCheckResult::Fail
        | WorkspaceCheckpointExecutedCheckResult::Error
        | WorkspaceCheckpointExecutedCheckResult::Skipped
        | WorkspaceCheckpointExecutedCheckResult::Unknown => {}
    }
}

#[test]
fn executed_check_result_vocabulary_is_exact_ordered_and_unique() {
    use WorkspaceCheckpointExecutedCheckResult::{Error, Fail, Pass, Skipped, Unknown};

    assert_eq!(WorkspaceCheckpointExecutedCheckResult::ALL.len(), 5);
    assert_eq!(
        WorkspaceCheckpointExecutedCheckResult::ALL,
        [Pass, Fail, Error, Skipped, Unknown]
    );
    assert_eq!(
        WorkspaceCheckpointExecutedCheckResult::ALL
            .map(WorkspaceCheckpointExecutedCheckResult::as_str),
        ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]
    );
    for (index, value) in WorkspaceCheckpointExecutedCheckResult::ALL
        .iter()
        .enumerate()
    {
        assert_executed_check_result_exhaustive(*value);
        for other in &WorkspaceCheckpointExecutedCheckResult::ALL[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}
