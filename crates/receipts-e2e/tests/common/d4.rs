//! Non-destructive observations, never a recovery permit or handoff issuer.
use receipts_review_integration::state::{
    AttemptFinish, AttemptId, AttemptRole, AttemptStart, StateWriteOperationV1,
    StateWriteSubjectV1, TrustedCoreStore,
};
use std::path::Path;

pub const ATTEMPT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn admission(store: &TrustedCoreStore) -> receipts_review_integration::state::StateWriteAdmission {
    store.admit(StateWriteSubjectV1 {
        operation: StateWriteOperationV1::Attempt,
        project_id: super::PROJECT.into(),
        graph_id: Some("graph-fixture".into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
}

pub fn seed(db: &Path, workspace: &str, sha: &str) -> String {
    let mut store = TrustedCoreStore::open_exclusive(db).unwrap();
    let start = AttemptStart::new(
        AttemptId::parse(ATTEMPT).unwrap(),
        super::PROJECT,
        "graph-fixture",
        "task-1",
        "1",
        AttemptRole::RuntimeA3,
        "synthetic",
        workspace,
        sha,
        super::STAMP,
    )
    .unwrap();
    let grant = admission(&store);
    store.append_attempt_started(grant, start).unwrap();
    store.store_id().to_owned()
}

pub fn finish(store: &mut TrustedCoreStore, sha: &str) {
    let finish = AttemptFinish::new(
        AttemptId::parse(ATTEMPT).unwrap(),
        "2026-11-08T10:00:01.000000000Z",
        Some(sha),
        None,
        None,
    )
    .unwrap();
    let grant = admission(store);
    store.append_attempt_finished(grant, finish).unwrap();
}

#[derive(Debug, PartialEq)]
pub enum Refusal {
    ForeignStore,
    ForeignAttempt,
    StaleAttempt,
}

pub fn current_unfinished(
    store: &TrustedCoreStore,
    store_id: &str,
    workspace: &str,
    sha: &str,
) -> Result<(), Refusal> {
    if store.store_id() != store_id {
        return Err(Refusal::ForeignStore);
    }
    let attempt = store
        .reader()
        .find_attempt(&AttemptId::parse(ATTEMPT).unwrap())
        .unwrap()
        .unwrap();
    if attempt.project_id() != super::PROJECT
        || attempt.node_id() != "task-1"
        || attempt.workspace_id() != workspace
        || attempt.start_sha() != sha
    {
        return Err(Refusal::ForeignAttempt);
    }
    if attempt.is_finished() {
        return Err(Refusal::StaleAttempt);
    }
    Ok(())
}
