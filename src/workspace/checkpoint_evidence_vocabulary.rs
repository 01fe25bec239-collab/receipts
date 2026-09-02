/// Closed vocabulary identifying the source of an executed checkpoint check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointCheckSource {
    WorkerExecution,
    BrokerExecution,
    ReviewExecution,
    GitProvenance,
}

impl WorkspaceCheckpointCheckSource {
    pub const ALL: [Self; 4] = [
        Self::WorkerExecution,
        Self::BrokerExecution,
        Self::ReviewExecution,
        Self::GitProvenance,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkerExecution => "WORKER_EXECUTION",
            Self::BrokerExecution => "BROKER_EXECUTION",
            Self::ReviewExecution => "REVIEW_EXECUTION",
            Self::GitProvenance => "GIT_PROVENANCE",
        }
    }
}

/// Shared closed vocabulary for checkpoint evidence reference types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointRefType {
    RepoPath,
    StateQuery,
    ArtifactId,
    Url,
}

impl WorkspaceCheckpointRefType {
    pub const ALL: [Self; 4] = [
        Self::RepoPath,
        Self::StateQuery,
        Self::ArtifactId,
        Self::Url,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepoPath => "REPO_PATH",
            Self::StateQuery => "STATE_QUERY",
            Self::ArtifactId => "ARTIFACT_ID",
            Self::Url => "URL",
        }
    }
}
