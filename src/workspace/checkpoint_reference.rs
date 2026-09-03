use std::fmt;

use crate::WorkspaceCheckpointRefType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCheckpointRef {
    ref_type: WorkspaceCheckpointRefType,
    target: String,
    digest: Option<String>,
    section: Option<String>,
}

impl WorkspaceCheckpointRef {
    pub fn new(
        ref_type: WorkspaceCheckpointRefType,
        target: impl Into<String>,
        digest: Option<String>,
        section: Option<String>,
    ) -> Result<Self, WorkspaceCheckpointRefError> {
        let target = target.into();
        if target.is_empty() {
            return Err(WorkspaceCheckpointRefError::EmptyTarget);
        }
        Ok(Self {
            ref_type,
            target,
            digest,
            section,
        })
    }

    pub const fn ref_type(&self) -> WorkspaceCheckpointRefType {
        self.ref_type
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointRefError {
    EmptyTarget,
}

impl fmt::Display for WorkspaceCheckpointRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTarget => f.write_str("workspace checkpoint reference target is empty"),
        }
    }
}

impl std::error::Error for WorkspaceCheckpointRefError {}
