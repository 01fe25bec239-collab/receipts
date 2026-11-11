use crate::{AttemptEvidenceSinkV1, TrustedAttemptEvidenceSinkV1};
use receipts_state::*;
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU64, Ordering};
#[test]
fn trusted_checkpoint_sink_reopens_replays_and_refuses_conflicting_and_foreign_metadata() {
    static N: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "receipts-sink-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    let db = path.join("state.db");
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let id = AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let admission = store.admit(StateWriteSubjectV1 {
        operation: StateWriteOperationV1::Attempt,
        project_id: "p".into(),
        graph_id: Some("g".into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    });
    store
        .append_attempt_started(
            admission,
            AttemptStart::new(
                id.clone(),
                "p",
                "g",
                "task",
                "1",
                AttemptRole::RuntimeA3,
                "runtime",
                "w",
                &"a".repeat(40),
                "2026-11-11T00:00:00.000000000Z",
            )
            .unwrap(),
        )
        .unwrap();
    let json = serde_json::json!({"checkpoint_id":"cp","workspace_id":"w","attempt_id":id.as_str(),"head_sha":"a".repeat(40),"captured_at":"2026-11-11T00:00:00Z","kind":"PRE_TERMINATION"});
    let checkpoint = CheckpointRecord::parse_json("p", &json.to_string()).unwrap();
    let output = OutputRef::new("p", "stdout", &"b".repeat(64), "artifact:stdout", "0").unwrap();
    let first = TrustedAttemptEvidenceSinkV1::new(&mut store)
        .persist_checkpoint(&checkpoint, std::slice::from_ref(&output))
        .unwrap();
    assert_eq!(first.attempt_id(), &id);
    assert_eq!(first.workspace_id(), "w");
    assert_eq!(first.project_id(), "p");
    assert_eq!(first.checkpoint_id(), "cp");
    let digest = first.checkpoint_sha256().to_owned();
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let replay = TrustedAttemptEvidenceSinkV1::new(&mut store)
        .persist_checkpoint(&checkpoint, std::slice::from_ref(&output))
        .unwrap();
    assert_eq!(replay.checkpoint_sha256(), digest);
    let conflict = OutputRef::new("p", "stdout", &"c".repeat(64), "artifact:changed", "0").unwrap();
    assert!(
        TrustedAttemptEvidenceSinkV1::new(&mut store)
            .persist_checkpoint(&checkpoint, &[conflict])
            .is_err()
    );
    assert!(
        store
            .reader()
            .find_output_ref("p", "stdout")
            .unwrap()
            .unwrap()
            == output
    );
    let mut foreign = json.clone();
    foreign["workspace_id"] = serde_json::json!("foreign");
    foreign["checkpoint_id"] = serde_json::json!("foreign");
    let foreign = CheckpointRecord::parse_json("p", &foreign.to_string()).unwrap();
    assert!(
        TrustedAttemptEvidenceSinkV1::new(&mut store)
            .persist_checkpoint(&foreign, &[])
            .is_err()
    );
    assert!(
        store
            .reader()
            .find_checkpoint("p", "foreign")
            .unwrap()
            .is_none()
    );
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}
