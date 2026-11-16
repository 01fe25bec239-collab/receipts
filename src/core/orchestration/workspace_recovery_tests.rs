//! Forwarding must retain Core project admission before any D4 State write.
use super::*;
use receipts_state::{GraphCompilerRefV1, WorkspaceAcceptedTargetV1, WorkspaceRecoverySubjectV1};
#[test]
fn workspace_recovery_forwarder_refuses_an_unadmitted_project_before_state_mutation() {
    let root = std::env::temp_dir().join(format!("receipts-core-d4-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .create(&root)
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let identity = CoreAdmissionIdentity::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "policy".into(),
    )
    .unwrap();
    let mut writer = CoreStateWriter::open(root.join("state.db"), identity).unwrap();
    let target = WorkspaceAcceptedTargetV1 {
        subject: WorkspaceRecoverySubjectV1 {
            project_id: "foreign".into(),
            graph_id: "graph".into(),
            attempt_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        },
        anchor_id: "anchor".into(),
        workspace_id: "workspace".into(),
        accepted_commit: "a".repeat(40),
        accepted_tree: "b".repeat(40),
        acceptance_claim_id: "claim".into(),
        decision_id: "accept".into(),
        provisioning_identity: "c".repeat(64),
    };
    assert!(matches!(
        writer.bind_workspace_acceptance_v1(&target),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::ProjectNotAdmitted
        ))
    ));
    drop(writer);
    std::fs::remove_dir_all(root).unwrap();
}
