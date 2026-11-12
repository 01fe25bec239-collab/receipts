//! Conservative projection of State's trusted startup scan. Unfinished is not
//! proof of an orphan: attempt-to-lease association and external identity matter.
use crate::CommitSha;
use receipts_state::recovery::StartupRecoveryReportV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryRefusal {
    ForeignProject,
    MissingLastAccepted,
    InvalidLastAccepted,
    ProcessIdentityAndProvisioningContinuityUnavailable,
}
#[derive(Debug)]
pub struct RecoveryCandidate {
    pub attempt_id: String,
    pub workspace_id: String,
    pub last_accepted: Option<CommitSha>,
    pub refusal: RecoveryRefusal,
}
/// The trusted caller supplies the accepted target for each workspace. A latest
/// checkpoint is deliberately never consulted as an acceptance substitute.
/// Every candidate retains data; no post-restart signal/reset authority exists.
pub fn detect_recovery_candidates(
    report: &StartupRecoveryReportV1,
    project: &str,
    mut last_accepted: impl FnMut(&str) -> Option<String>,
) -> Result<Vec<RecoveryCandidate>, RecoveryRefusal> {
    if report.project_id != project
        || report
            .unfinished_attempts
            .iter()
            .any(|a| a.project_id() != project || a.is_finished())
    {
        return Err(RecoveryRefusal::ForeignProject);
    }
    Ok(report
        .unfinished_attempts
        .iter()
        .map(|a| {
            let (last_accepted, refusal) = match last_accepted(a.workspace_id()) {
                None => (None, RecoveryRefusal::MissingLastAccepted),
                Some(target) => match CommitSha::parse(&target) {
                    Err(_) => (None, RecoveryRefusal::InvalidLastAccepted),
                    Ok(target) => (
                        Some(target),
                        RecoveryRefusal::ProcessIdentityAndProvisioningContinuityUnavailable,
                    ),
                },
            };
            RecoveryCandidate {
                attempt_id: a.attempt_id().as_str().into(),
                workspace_id: a.workspace_id().into(),
                last_accepted,
                refusal,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use receipts_state::*;
    #[test]
    fn durable_unfinished_candidates_retain_exact_accepted_target_and_always_refuse_mutation() {
        let root = crate::test_support::TempDir::new("orphan-durable");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let db = root.path().join("state.db");
        let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
        let id = AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let admission = store.admit(StateWriteSubjectV1 {
            operation: StateWriteOperationV1::Attempt,
            project_id: "project".into(),
            graph_id: Some("graph".into()),
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        });
        store
            .append_attempt_started(
                admission,
                AttemptStart::new(
                    id,
                    "project",
                    "graph",
                    "node",
                    "1",
                    AttemptRole::RuntimeA3,
                    "runtime",
                    "workspace",
                    &"a".repeat(40),
                    "2026-11-11T00:00:00.000000000Z",
                )
                .unwrap(),
            )
            .unwrap();
        drop(store);
        let store = TrustedCoreStore::open_exclusive(&db).unwrap();
        let report = StartupRecoveryReportV1 {
            project_id: "project".into(),
            trusted_now: "2026-11-11T01:00:00.000000000Z".into(),
            lease_expired_binding_ids: vec![],
            roles: vec![],
            unfinished_attempts: store.reader().list_unfinished_attempts("project").unwrap(),
        };
        let candidates =
            detect_recovery_candidates(&report, "project", |_| Some("b".repeat(40))).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].last_accepted.as_ref().unwrap().as_str(),
            "b".repeat(40)
        );
        assert_eq!(
            candidates[0].refusal,
            RecoveryRefusal::ProcessIdentityAndProvisioningContinuityUnavailable
        );
        assert_eq!(
            detect_recovery_candidates(&report, "project", |_| None).unwrap()[0].refusal,
            RecoveryRefusal::MissingLastAccepted
        );
        assert_eq!(
            detect_recovery_candidates(&report, "project", |_| Some("invalid".into())).unwrap()[0]
                .refusal,
            RecoveryRefusal::InvalidLastAccepted
        );
        assert_eq!(
            detect_recovery_candidates(&report, "foreign", |_| None).unwrap_err(),
            RecoveryRefusal::ForeignProject
        );
        assert_eq!(
            store
                .reader()
                .list_unfinished_attempts("project")
                .unwrap()
                .len(),
            1
        );
    }
}
