//! Frozen, host-neutral vocabularies for normalized host events.
//!
//! This module defines only two closed word lists: the normalized host
//! event type and its confidence. Both are owned by Host Integration and
//! are the vocabulary a later `NormalizedHostEvent` envelope will use.
//!
//! Nothing else attaches here: no envelope, no identifiers, no timestamps,
//! no payloads, no persistence, no dispatch, no parsing, and no mapping
//! from any host hook, source class, or fallback posture to either value.
//! Deciding which real event carries which confidence belongs to later
//! event-bridge work; this module only defines the words.

/// A normalized host event type.
///
/// The set is closed by contract: exactly these sixteen event types exist,
/// they are host-neutral, and matching over them is exhaustive. Host hook
/// names (for example Claude's or Codex's `SessionStart`) are sources, not
/// members of this vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedHostEventType {
    /// Host session began.
    HostSessionStarted,
    /// Host session is ending.
    HostSessionEnding,
    /// The user submitted a goal.
    UserGoalSubmitted,
    /// The user answered a core prompt.
    UserInputProvided,
    /// An executor was bound to a logical role.
    RoleExecutorStarted,
    /// An executor was released.
    RoleExecutorStopped,
    /// An attempt began.
    TaskStarted,
    /// An attempt finished with a result.
    TaskCompleted,
    /// An attempt failed.
    TaskFailed,
    /// A tool or command ran.
    ToolExecuted,
    /// A workspace was created.
    WorkspaceCreated,
    /// Workspace files changed.
    WorkspaceChanged,
    /// A workspace was removed.
    WorkspaceRemoved,
    /// Context was compacted.
    ContextCompacted,
    /// A provider-level signal was observed.
    ProviderSignal,
    /// A host-level failure occurred.
    HostError,
}

impl NormalizedHostEventType {
    /// Every event type in the frozen vocabulary.
    pub const ALL: [NormalizedHostEventType; 16] = [
        NormalizedHostEventType::HostSessionStarted,
        NormalizedHostEventType::HostSessionEnding,
        NormalizedHostEventType::UserGoalSubmitted,
        NormalizedHostEventType::UserInputProvided,
        NormalizedHostEventType::RoleExecutorStarted,
        NormalizedHostEventType::RoleExecutorStopped,
        NormalizedHostEventType::TaskStarted,
        NormalizedHostEventType::TaskCompleted,
        NormalizedHostEventType::TaskFailed,
        NormalizedHostEventType::ToolExecuted,
        NormalizedHostEventType::WorkspaceCreated,
        NormalizedHostEventType::WorkspaceChanged,
        NormalizedHostEventType::WorkspaceRemoved,
        NormalizedHostEventType::ContextCompacted,
        NormalizedHostEventType::ProviderSignal,
        NormalizedHostEventType::HostError,
    ];

    /// Canonical external string for this event type.
    pub fn as_str(self) -> &'static str {
        match self {
            NormalizedHostEventType::HostSessionStarted => "HOST_SESSION_STARTED",
            NormalizedHostEventType::HostSessionEnding => "HOST_SESSION_ENDING",
            NormalizedHostEventType::UserGoalSubmitted => "USER_GOAL_SUBMITTED",
            NormalizedHostEventType::UserInputProvided => "USER_INPUT_PROVIDED",
            NormalizedHostEventType::RoleExecutorStarted => "ROLE_EXECUTOR_STARTED",
            NormalizedHostEventType::RoleExecutorStopped => "ROLE_EXECUTOR_STOPPED",
            NormalizedHostEventType::TaskStarted => "TASK_STARTED",
            NormalizedHostEventType::TaskCompleted => "TASK_COMPLETED",
            NormalizedHostEventType::TaskFailed => "TASK_FAILED",
            NormalizedHostEventType::ToolExecuted => "TOOL_EXECUTED",
            NormalizedHostEventType::WorkspaceCreated => "WORKSPACE_CREATED",
            NormalizedHostEventType::WorkspaceChanged => "WORKSPACE_CHANGED",
            NormalizedHostEventType::WorkspaceRemoved => "WORKSPACE_REMOVED",
            NormalizedHostEventType::ContextCompacted => "CONTEXT_COMPACTED",
            NormalizedHostEventType::ProviderSignal => "PROVIDER_SIGNAL",
            NormalizedHostEventType::HostError => "HOST_ERROR",
        }
    }
}

/// Whether a normalized host event was directly observed or derived.
///
/// The set is closed by contract: exactly these two values exist. This type
/// carries the words only; it assigns no value to any event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedHostEventConfidence {
    /// The event was observed directly.
    Observed,
    /// The event was derived rather than observed directly.
    Inferred,
}

impl NormalizedHostEventConfidence {
    /// Every confidence value in the frozen vocabulary.
    pub const ALL: [NormalizedHostEventConfidence; 2] = [
        NormalizedHostEventConfidence::Observed,
        NormalizedHostEventConfidence::Inferred,
    ];

    /// Canonical external string for this confidence value.
    pub fn as_str(self) -> &'static str {
        match self {
            NormalizedHostEventConfidence::Observed => "OBSERVED",
            NormalizedHostEventConfidence::Inferred => "INFERRED",
        }
    }
}
