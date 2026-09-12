use super::{ProjectRemotePolicy, WorkspaceCompositionLevel};
use receipts_workspace_execution::WorkspaceRemotePublishPolicy;

#[test]
fn all_eight_combinations_return_canonical_workspace_policy() {
    use ProjectRemotePolicy::{LocalOnly, PushA2Branches, PushAcceptedA3, PushAllCheckpoints};
    use WorkspaceCompositionLevel::{TaskAttempt, Workstream};
    use WorkspaceRemotePublishPolicy::{LocalOnly as Local, PushAlways, PushOnAccept};

    let compose: fn(
        ProjectRemotePolicy,
        WorkspaceCompositionLevel,
    ) -> WorkspaceRemotePublishPolicy = ProjectRemotePolicy::compose;
    let cases = [
        (LocalOnly, Workstream, Local),
        (LocalOnly, TaskAttempt, Local),
        (PushA2Branches, Workstream, PushAlways),
        (PushA2Branches, TaskAttempt, Local),
        (PushAcceptedA3, Workstream, PushAlways),
        (PushAcceptedA3, TaskAttempt, PushOnAccept),
        (PushAllCheckpoints, Workstream, PushAlways),
        (PushAllCheckpoints, TaskAttempt, PushAlways),
    ];
    for (project, level, expected) in cases {
        assert_eq!(compose(project, level), expected, "{project:?} + {level:?}");
    }
}

#[test]
fn project_policy_defaults_to_push_a2_branches() {
    assert_eq!(
        ProjectRemotePolicy::default(),
        ProjectRemotePolicy::PushA2Branches
    );
}
