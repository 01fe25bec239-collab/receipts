use super::persisted_attempt::*;
use crate::{
    AttemptEvidenceSinkV1, WorkspaceProvisionRequest,
    execution::{
        LiveProcessPreparedCancelAcceptance, LiveProcessTerminalCause, ProcessRunRequest,
        ProcessTimeoutPolicy,
    },
    test_support::{TempDir, TestRepo},
};
use receipts_state::*;
use std::{sync::mpsc, time::Duration};
struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-11-11T00:00:00.000000000Z".into(),
            clock_source_id: "fixture".into(),
            clock_contract_version: "1".into(),
        })
    }
}
struct Sink {
    kinds: Vec<String>,
    fail: Option<usize>,
    pause_pre_cancel: Option<Duration>,
}
impl AttemptEvidenceSinkV1 for Sink {
    type Receipt = usize;
    fn persist_checkpoint(
        &mut self,
        cp: &CheckpointRecord,
        outputs: &[OutputRef],
    ) -> Result<usize, StateError> {
        let json: serde_json::Value = serde_json::from_str(&cp.to_json().unwrap()).unwrap();
        let kind = json["kind"].as_str().unwrap();
        self.kinds.push(kind.into());
        for output in outputs {
            assert!(!output.output_ref().is_empty());
        }
        if let Some(pause) = self.pause_pre_cancel
            && kind == "PRE_TERMINATION"
        {
            std::thread::sleep(pause);
        }
        if self.fail == Some(self.kinds.len()) {
            return Err(StateError::SetupRefused {
                reason: SetupRefusalV1::MissingEvidence,
            });
        }
        Ok(self.kinds.len())
    }
}
fn exercise(
    cancel: bool,
    timeout: Duration,
    fail: Option<usize>,
    pause: bool,
) -> (
    Result<PersistedAttemptOutcome<usize>, PersistedAttemptError>,
    Sink,
) {
    let repo = TestRepo::new("persisted-run");
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "workspace",
        None,
        "task/run",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let process = ProcessRunRequest::new(
        "/bin/sleep",
        ["0.3"],
        handle.worktree_path(),
        handle.worktree_path(),
    )
    .unwrap();
    let timeout = ProcessTimeoutPolicy::new(timeout, Duration::from_millis(20)).unwrap();
    let outputs = TempDir::new("persisted-outputs");
    let (tx, rx) = mpsc::channel();
    if cancel {
        tx.send(()).unwrap();
    }
    let mut sink = Sink {
        kinds: vec![],
        fail,
        pause_pre_cancel: pause.then_some(Duration::from_millis(180)),
    };
    let result = run_persisted_attempt(
        PersistedAttemptRequest {
            handle: &handle,
            process: &process,
            timeout: &timeout,
            project_id: "project",
            attempt_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            output_directory: outputs.path(),
            interval: Duration::from_millis(30),
            clock: &Clock,
            cancellation: &rx,
        },
        &mut sink,
    );
    (result, sink)
}
#[test]
fn start_interval_finish_and_explicit_cancel_are_persisted_in_order() {
    let (result, sink) = exercise(false, Duration::from_secs(3), None, false);
    assert!(result.unwrap().terminal.success());
    assert!(sink.kinds.len() >= 3, "start, interval, finish");
    let (result, sink) = exercise(true, Duration::from_secs(3), None, false);
    let result = result.unwrap();
    assert_eq!(
        result.cancellation,
        Some(LiveProcessPreparedCancelAcceptance::CancelAccepted)
    );
    assert_eq!(
        result.terminal.terminal_cause(),
        LiveProcessTerminalCause::Cancelled
    );
    assert_eq!(sink.kinds, ["PROGRESS", "PRE_TERMINATION", "PROGRESS"]);
}
#[test]
fn timeout_wins_while_persistence_runs_and_failure_preserves_bounded_cleanup() {
    let (result, sink) = exercise(true, Duration::from_millis(70), None, true);
    let result = result.unwrap();
    assert_eq!(
        result.terminal.terminal_cause(),
        LiveProcessTerminalCause::TimedOut
    );
    assert_ne!(
        result.cancellation,
        Some(LiveProcessPreparedCancelAcceptance::CancelAccepted)
    );
    assert!(sink.kinds.contains(&"PRE_TERMINATION".into()));
    for fail in [1, 2, 3] {
        let started = std::time::Instant::now();
        let (result, sink) = exercise(true, Duration::from_secs(3), Some(fail), false);
        assert!(matches!(result, Err(PersistedAttemptError::Checkpoint(_))));
        assert_eq!(sink.kinds.len(), fail);
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

#[test]
fn actual_state_sink_reopens_checkpoint_and_bound_output_metadata() {
    let repo = TestRepo::new("persisted-state");
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "workspace",
        None,
        "task/run",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let root = TempDir::new("persisted-state-private");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = root.path().join("state.db");
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let admission = store.admit(StateWriteSubjectV1 {
        operation: StateWriteOperationV1::Attempt,
        project_id: "project".into(),
        graph_id: Some("graph".into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    });
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    store
        .append_attempt_started(
            admission,
            AttemptStart::new(
                AttemptId::parse(id).unwrap(),
                "project",
                "graph",
                "node",
                "1",
                AttemptRole::RuntimeA3,
                "runtime",
                "workspace",
                &repo.head_sha(),
                "2026-11-11T00:00:00.000000000Z",
            )
            .unwrap(),
        )
        .unwrap();
    let process = ProcessRunRequest::new(
        "/bin/sleep",
        ["0.15"],
        handle.worktree_path(),
        handle.worktree_path(),
    )
    .unwrap();
    let timeout =
        ProcessTimeoutPolicy::new(Duration::from_secs(3), Duration::from_millis(20)).unwrap();
    let (_tx, rx) = mpsc::channel();
    let result = run_persisted_attempt(
        PersistedAttemptRequest {
            handle: &handle,
            process: &process,
            timeout: &timeout,
            project_id: "project",
            attempt_id: id,
            output_directory: root.path(),
            interval: Duration::from_secs(1),
            clock: &Clock,
            cancellation: &rx,
        },
        &mut crate::TrustedAttemptEvidenceSinkV1::new(&mut store),
    )
    .unwrap();
    assert!(result.terminal.success());
    let checkpoint_id = result.finish_receipt.checkpoint_id().to_owned();
    let saved = store
        .reader()
        .find_checkpoint("project", &checkpoint_id)
        .unwrap()
        .unwrap();
    let core = crate::WorkspaceCheckpointWire::from_json(&saved.to_json().unwrap())
        .unwrap()
        .to_temporal_capture_core()
        .unwrap();
    let evidence = crate::finalize_workspace_attempt(crate::WorkspaceAttemptFinalizationRequest {
        handle: &handle,
        workspace_id: "workspace",
        task_id: None,
        attempt_id: Some(id),
        expected_worktree: handle.worktree_path(),
        expected_branch: handle.branch(),
        start_sha: handle.base_sha().as_str(),
        final_sha: handle.base_sha().as_str(),
        allowed_write_paths: &[],
        forbidden_write_paths: &[],
        checkpoint: core.core(),
        terminal: &result.terminal,
    })
    .unwrap();
    let finish = AttemptFinish::new(
        AttemptId::parse(id).unwrap(),
        "2026-11-11T00:01:00.000000000Z",
        Some(handle.base_sha().as_str()),
        None,
        None,
    )
    .unwrap();
    let mut sink = crate::TrustedAttemptEvidenceSinkV1::new(&mut store);
    let refs = ["stdout", "stderr", "dirty-diff"]
        .map(|name| format!("{id}:1:{name}"))
        .to_vec();
    let foreign_finish = AttemptFinish::new(
        AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        "2026-11-11T00:01:00.000000000Z",
        Some(handle.base_sha().as_str()),
        None,
        None,
    )
    .unwrap();
    assert!(
        sink.persist_verified_finalization(
            &evidence,
            &result.finish_receipt,
            &foreign_finish,
            refs.clone()
        )
        .is_err()
    );
    assert_eq!(
        sink.persist_verified_finalization(
            &evidence,
            &result.finish_receipt,
            &finish,
            refs.clone()
        )
        .unwrap(),
        SetupWriteOutcomeV1::Applied
    );
    assert_eq!(
        sink.persist_verified_finalization(&evidence, &result.finish_receipt, &finish, refs)
            .unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    drop(store);
    let store = TrustedCoreStore::open_exclusive(&db).unwrap();
    assert!(
        store
            .reader()
            .find_checkpoint("project", &checkpoint_id)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .reader()
            .find_attempt(&AttemptId::parse(id).unwrap())
            .unwrap()
            .unwrap()
            .is_finished()
    );
    for name in ["stdout", "stderr", "dirty-diff"] {
        let reference = format!("{id}:1:{name}");
        assert!(
            store
                .reader()
                .find_output_ref("project", &reference)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn natural_completion_wins_prepared_cancel_during_slow_sink() {
    let repo = TestRepo::new("completion-race");
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "workspace",
        None,
        "task/run",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let process = ProcessRunRequest::new(
        "/bin/sleep",
        ["0.1"],
        handle.worktree_path(),
        handle.worktree_path(),
    )
    .unwrap();
    let timeout =
        ProcessTimeoutPolicy::new(Duration::from_secs(3), Duration::from_millis(20)).unwrap();
    let output = TempDir::new("completion-output");
    let (tx, rx) = mpsc::channel();
    tx.send(()).unwrap();
    let mut sink = Sink {
        kinds: vec![],
        fail: None,
        pause_pre_cancel: Some(Duration::from_millis(250)),
    };
    let outcome = run_persisted_attempt(
        PersistedAttemptRequest {
            handle: &handle,
            process: &process,
            timeout: &timeout,
            project_id: "project",
            attempt_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            output_directory: output.path(),
            interval: Duration::from_secs(1),
            clock: &Clock,
            cancellation: &rx,
        },
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        outcome.terminal.terminal_cause(),
        LiveProcessTerminalCause::Completed
    );
    assert_ne!(
        outcome.cancellation,
        Some(LiveProcessPreparedCancelAcceptance::CancelAccepted)
    );
    assert_eq!(sink.kinds, ["PROGRESS", "PRE_TERMINATION", "PROGRESS"]);
}

#[test]
fn failed_start_persistence_never_launches_child_and_interval_failure_cleans_up() {
    let repo = TestRepo::new("start-refusal");
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "workspace",
        None,
        "task/run",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let marker = handle.worktree_path().join("must-not-exist");
    let process = ProcessRunRequest::new(
        "/usr/bin/touch",
        [marker.as_os_str()],
        handle.worktree_path(),
        handle.worktree_path(),
    )
    .unwrap();
    let timeout =
        ProcessTimeoutPolicy::new(Duration::from_secs(3), Duration::from_millis(20)).unwrap();
    let output = TempDir::new("start-refusal-output");
    let (_tx, rx) = mpsc::channel();
    let mut sink = Sink {
        kinds: vec![],
        fail: Some(1),
        pause_pre_cancel: None,
    };
    assert!(
        run_persisted_attempt(
            PersistedAttemptRequest {
                handle: &handle,
                process: &process,
                timeout: &timeout,
                project_id: "project",
                attempt_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                output_directory: output.path(),
                interval: Duration::from_secs(1),
                clock: &Clock,
                cancellation: &rx
            },
            &mut sink
        )
        .is_err()
    );
    assert!(!marker.exists());
    let (result, sink) = exercise(false, Duration::from_secs(3), Some(2), false);
    assert!(matches!(result, Err(PersistedAttemptError::Checkpoint(_))));
    assert_eq!(sink.kinds, ["PROGRESS", "PROGRESS"]);
}
