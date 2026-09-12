use receipts_workspace_execution::WorkspaceRemotePublishPolicy;

/// Project publication intent, composed upstream of Workspace operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectRemotePolicy {
    /// LOCAL_ONLY
    LocalOnly,
    /// PUSH_A2_BRANCHES
    #[default]
    PushA2Branches,
    /// PUSH_ACCEPTED_A3
    PushAcceptedA3,
    /// PUSH_ALL_CHECKPOINTS
    PushAllCheckpoints,
}

/// Explicit caller-supplied composition level; never inferred or defaulted.
///
/// ```compile_fail
/// use receipts_orchestration::orchestration::WorkspaceCompositionLevel;
/// let _: WorkspaceCompositionLevel = Default::default();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCompositionLevel {
    Workstream,
    TaskAttempt,
}

impl ProjectRemotePolicy {
    /// Composes project intent into the canonical Workspace-owned policy.
    pub const fn compose(self, level: WorkspaceCompositionLevel) -> WorkspaceRemotePublishPolicy {
        use WorkspaceCompositionLevel::{TaskAttempt, Workstream};

        match (self, level) {
            (Self::LocalOnly, Workstream | TaskAttempt) | (Self::PushA2Branches, TaskAttempt) => {
                WorkspaceRemotePublishPolicy::LocalOnly
            }
            (
                Self::PushA2Branches | Self::PushAcceptedA3 | Self::PushAllCheckpoints,
                Workstream,
            )
            | (Self::PushAllCheckpoints, TaskAttempt) => WorkspaceRemotePublishPolicy::PushAlways,
            (Self::PushAcceptedA3, TaskAttempt) => WorkspaceRemotePublishPolicy::PushOnAccept,
        }
    }
}
