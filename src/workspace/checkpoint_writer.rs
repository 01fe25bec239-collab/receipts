//! Capture and admit a checkpoint before the caller performs cleanup.
use crate::{
    AttemptEvidenceSinkV1, WorkspaceCheckpointEvidenceCaptureError,
    WorkspaceCheckpointEvidenceCaptureRequest, WorkspaceCheckpointJsonExportError,
    WorkspaceDateTimeV1, capture_workspace_checkpoint_temporal_evidence,
    workspace_checkpoint_to_json,
};
use receipts_state::{CheckpointRecord, OutputRef, StateError};
#[derive(Debug)]
pub enum CheckpointWriteError {
    Capture(WorkspaceCheckpointEvidenceCaptureError),
    Encode(WorkspaceCheckpointJsonExportError),
    State(StateError),
}
/// Call at start, interval, prepared cancellation and finish. Errors must prevent
/// caller-controlled destructive cleanup. Automatic runner cleanup stays bounded.
pub fn capture_and_persist<S: AttemptEvidenceSinkV1>(
    sink: &mut S,
    project: &str,
    request: WorkspaceCheckpointEvidenceCaptureRequest<'_>,
    captured_at: WorkspaceDateTimeV1,
    outputs: &[OutputRef],
) -> Result<S::Receipt, CheckpointWriteError> {
    let checkpoint = capture_workspace_checkpoint_temporal_evidence(request, captured_at)
        .map_err(CheckpointWriteError::Capture)?;
    let json = workspace_checkpoint_to_json(&checkpoint).map_err(CheckpointWriteError::Encode)?;
    let record =
        CheckpointRecord::parse_json(project, &json).map_err(CheckpointWriteError::State)?;
    sink.persist_checkpoint(&record, outputs)
        .map_err(CheckpointWriteError::State)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Sink(usize);
    impl AttemptEvidenceSinkV1 for Sink {
        type Receipt = usize;
        fn persist_checkpoint(
            &mut self,
            checkpoint: &CheckpointRecord,
            _: &[OutputRef],
        ) -> Result<usize, StateError> {
            assert_eq!(checkpoint.workspace_id(), "workspace");
            self.0 += 1;
            Ok(self.0)
        }
    }
    #[test]
    fn captures_git_before_persisting_for_each_caller_event() {
        let repo = crate::test_support::TestRepo::new("writer");
        let mut sink = Sink(0);
        for (index, kind) in [
            crate::WorkspaceCheckpointKind::Progress,
            crate::WorkspaceCheckpointKind::Progress,
            crate::WorkspaceCheckpointKind::PreTermination,
            crate::WorkspaceCheckpointKind::Progress,
        ]
        .into_iter()
        .enumerate()
        {
            let request = WorkspaceCheckpointEvidenceCaptureRequest {
                directory: repo.path(),
                checkpoint_id: format!("cp-{index}"),
                workspace_id: "workspace".into(),
                task_id: None,
                attempt_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
                kind,
                base_sha: None,
                dirty_diff_ref: None,
                executed_checks: vec![],
            };
            assert_eq!(
                capture_and_persist(
                    &mut sink,
                    "project",
                    request,
                    WorkspaceDateTimeV1::try_new("2026-11-11T00:00:00Z").unwrap(),
                    &[]
                )
                .unwrap(),
                index + 1
            );
        }
    }
}
