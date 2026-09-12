//! In-process normalized host event envelope and frozen vocabularies.
//!
//! Physical binding under
//! BUILD-A1-ADR-HOST-A3-005-NORMALIZED-HOST-EVENT-PHYSICAL-V1-001.
//!
//! Structural validation only: no ingestion, bridge mapping, persistence,
//! parsing, serialization, or binding to `HostAdapter::emit`. Date-time and
//! generic JSON object carriers are consumed from Orchestration unchanged.

use receipts_orchestration::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};

use crate::HostId;

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

/// A structural construction failure; no host or event-specific policy is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedHostEventError {
    InvalidEventId,
    EmptyHost,
    InvalidHostSessionId,
    InvalidProjectId,
    EmptyRawRefTarget,
}

impl std::fmt::Display for NormalizedHostEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEventId => {
                "event_id must be exactly 26 bytes from 0123456789ABCDEFGHJKMNPQRSTVWXYZ"
            }
            Self::EmptyHost => "host must be non-empty",
            Self::InvalidHostSessionId => "host_session_id must contain 1..=200 characters",
            Self::InvalidProjectId => "present project_id must contain 1..=200 characters",
            Self::EmptyRawRefTarget => "raw_ref target must be non-empty",
        })
    }
}

impl std::error::Error for NormalizedHostEventError {}

/// Exactly 26 ASCII bytes from `0123456789ABCDEFGHJKMNPQRSTVWXYZ`.
/// The supplied value is preserved; there is no first-character restriction.
///
/// Direct unvalidated construction is unavailable:
/// ```compile_fail,E0423
/// use receipts_host_integration::NormalizedHostEventId;
/// let invalid = NormalizedHostEventId(String::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedHostEventId(String);

impl NormalizedHostEventId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, NormalizedHostEventError> {
        let value = value.into();
        if value.len() != 26
            || !value
                .bytes()
                .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte))
        {
            return Err(NormalizedHostEventError::InvalidEventId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An open, non-empty UTF-8 event host, preserved without normalization.
/// This is not the closed adapter identity or its diagnostic representation.
///
/// ```compile_fail,E0423
/// use receipts_host_integration::NormalizedHostEventHost;
/// let invalid = NormalizedHostEventHost(String::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedHostEventHost(String);

impl NormalizedHostEventHost {
    pub fn try_new(value: impl Into<String>) -> Result<Self, NormalizedHostEventError> {
        let value = value.into();
        if value.is_empty() {
            return Err(NormalizedHostEventError::EmptyHost);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<HostId> for NormalizedHostEventHost {
    fn from(host: HostId) -> Self {
        Self(
            match host {
                HostId::ClaudeCode => "claude-code",
                HostId::Codex => "codex",
                HostId::Headless => "headless",
            }
            .to_owned(),
        )
    }
}

/// Closed raw-material reference kinds; no raw host contents are carried here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedHostEventRawRefType {
    RepoPath,
    StateQuery,
    ArtifactId,
    Url,
}

impl NormalizedHostEventRawRefType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepoPath => "REPO_PATH",
            Self::StateQuery => "STATE_QUERY",
            Self::ArtifactId => "ARTIFACT_ID",
            Self::Url => "URL",
        }
    }
}

/// A structured pointer to raw material, with a non-empty target.
/// Optional digest and section preserve both absence and empty strings.
///
/// The target cannot be invalidated after construction:
/// ```compile_fail,E0616
/// use receipts_host_integration::NormalizedHostEventRawRef;
/// fn invalidate(reference: &mut NormalizedHostEventRawRef) {
///     reference.target = String::new();
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHostEventRawRef {
    ref_type: NormalizedHostEventRawRefType,
    target: String,
    digest: Option<String>,
    section: Option<String>,
}

impl NormalizedHostEventRawRef {
    pub fn try_new(
        ref_type: NormalizedHostEventRawRefType,
        target: String,
        digest: Option<String>,
        section: Option<String>,
    ) -> Result<Self, NormalizedHostEventError> {
        if target.is_empty() {
            return Err(NormalizedHostEventError::EmptyRawRefTarget);
        }
        Ok(Self {
            ref_type,
            target,
            digest,
            section,
        })
    }

    pub fn ref_type(&self) -> NormalizedHostEventRawRefType {
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

/// Caller-supplied envelope fields. Session/project lengths are checked by
/// [`NormalizedHostEvent::try_new`]; other structural invariants hold by type.
/// `payload` is a required object, including when it contains no properties.
///
/// Neither an absent payload, a date-time string, nor an opaque raw reference
/// can be substituted for the physical carriers:
/// ```compile_fail,E0308
/// use receipts_host_integration::NormalizedHostEventInputs;
/// fn omit_payload(inputs: &mut NormalizedHostEventInputs) {
///     inputs.payload = None;
/// }
/// ```
/// ```compile_fail,E0308
/// use receipts_host_integration::NormalizedHostEventInputs;
/// fn substitute_date_time(inputs: &mut NormalizedHostEventInputs) {
///     inputs.occurred_at = "2026-09-12T00:00:00Z".to_owned();
/// }
/// ```
/// ```compile_fail,E0308
/// use receipts_host_integration::NormalizedHostEventInputs;
/// fn substitute_raw_ref(inputs: &mut NormalizedHostEventInputs) {
///     inputs.raw_ref = Some("opaque".to_owned());
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHostEventInputs {
    pub event_id: NormalizedHostEventId,
    pub event_type: NormalizedHostEventType,
    pub host: NormalizedHostEventHost,
    /// Required, 1..=200 Unicode scalar values.
    pub host_session_id: String,
    /// Optional; when present, 1..=200 Unicode scalar values.
    pub project_id: Option<String>,
    pub occurred_at: OrchestrationDateTimeV1,
    pub payload: OrchestrationJsonObjectV1,
    pub raw_ref: Option<NormalizedHostEventRawRef>,
    pub confidence: NormalizedHostEventConfidence,
}

/// A structurally valid in-process event. Fields are private, and accessors
/// expose no mutable references. Payload contents and event semantics are
/// deliberately unconstrained; producers own their later bounded contracts.
///
/// Validated envelope fields cannot be overwritten:
/// ```compile_fail,E0616
/// use receipts_host_integration::NormalizedHostEvent;
/// fn invalidate(event: &mut NormalizedHostEvent) {
///     event.host_session_id = String::new();
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHostEvent {
    event_id: NormalizedHostEventId,
    event_type: NormalizedHostEventType,
    host: NormalizedHostEventHost,
    host_session_id: String,
    project_id: Option<String>,
    occurred_at: OrchestrationDateTimeV1,
    payload: OrchestrationJsonObjectV1,
    raw_ref: Option<NormalizedHostEventRawRef>,
    confidence: NormalizedHostEventConfidence,
}

impl NormalizedHostEvent {
    /// Checks session first, then project. No supplied value is normalized.
    pub fn try_new(inputs: NormalizedHostEventInputs) -> Result<Self, NormalizedHostEventError> {
        if !(1..=200).contains(&inputs.host_session_id.chars().count()) {
            return Err(NormalizedHostEventError::InvalidHostSessionId);
        }
        if inputs
            .project_id
            .as_ref()
            .is_some_and(|id| !(1..=200).contains(&id.chars().count()))
        {
            return Err(NormalizedHostEventError::InvalidProjectId);
        }
        Ok(Self {
            event_id: inputs.event_id,
            event_type: inputs.event_type,
            host: inputs.host,
            host_session_id: inputs.host_session_id,
            project_id: inputs.project_id,
            occurred_at: inputs.occurred_at,
            payload: inputs.payload,
            raw_ref: inputs.raw_ref,
            confidence: inputs.confidence,
        })
    }

    pub fn event_id(&self) -> &NormalizedHostEventId {
        &self.event_id
    }

    pub fn event_type(&self) -> NormalizedHostEventType {
        self.event_type
    }

    pub fn host(&self) -> &NormalizedHostEventHost {
        &self.host
    }

    pub fn host_session_id(&self) -> &str {
        &self.host_session_id
    }

    pub fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }

    pub fn occurred_at(&self) -> &OrchestrationDateTimeV1 {
        &self.occurred_at
    }

    pub fn payload(&self) -> &OrchestrationJsonObjectV1 {
        &self.payload
    }

    pub fn raw_ref(&self) -> Option<&NormalizedHostEventRawRef> {
        self.raw_ref.as_ref()
    }

    pub fn confidence(&self) -> NormalizedHostEventConfidence {
        self.confidence
    }
}
