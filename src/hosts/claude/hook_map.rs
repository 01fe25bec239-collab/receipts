//! Bounded parsing of untrusted Claude Code hook stdin and its mapping to
//! frozen [`NormalizedHostEvent`]s (HI-W2-2b).
//!
//! Input contract (Claude Code hooks reference, checked statically against
//! the installed 2.1.319 bundle): one JSON object on stdin with common
//! `session_id`, `transcript_path`, `cwd`, `hook_event_name`; `WorktreeCreate`
//! adds `name`, `WorktreeRemove` adds `worktree_path`. The parse refuses
//! oversized, non-UTF-8, non-object, duplicate-key and deeply nested input,
//! a `hook_event_name` that is not the registered event, and malformed
//! required fields. Only the bounded fields below survive; tool input and
//! output, transcript paths, prompts and every other host field are dropped
//! (design rule: no host-specific fields in payloads).
//!
//! Mapping (CLAUDE_HOST_ADAPTER.md table, restricted to rows whose frozen
//! source class permits the event; every event is OBSERVED):
//!
//! | Hook | Event | Source class |
//! |---|---|---|
//! | SessionStart | HOST_SESSION_STARTED | HOST_HOOK |
//! | SessionEnd | HOST_SESSION_ENDING | HOST_HOOK |
//! | PostToolUse | TOOL_EXECUTED | HOST_HOOK |
//! | PreCompact, PostCompact | CONTEXT_COMPACTED | HOST_HOOK |
//! | WorktreeCreate | WORKSPACE_CREATED, after the handler created it | CORE_DRIVEN |
//!
//! Not registered: `FileChanged`→WORKSPACE_CHANGED (frozen class
//! WORKER_DISPATCH), `SubagentStart/Stop`→ROLE_EXECUTOR_* and
//! `WorktreeRemove`→WORKSPACE_REMOVED (CORE_DRIVEN, core not wired until
//! HI-W4-1), `TaskCompleted` (WORKER_DISPATCH). `WorktreeRemove` is still
//! handled (see `worktree`), but it emits no event because nothing is
//! removed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1 as V,
};

use crate::host_capability_observation::HostHookExecutionEvidenceV1;
use crate::{
    HostId, NormalizedHostEvent, NormalizedHostEventConfidence, NormalizedHostEventId,
    NormalizedHostEventInputs, NormalizedHostEventSourceClass, NormalizedHostEventType,
    validate_normalized_host_event_source,
};

/// Largest accepted hook stdin. Larger input is refused, not truncated.
pub const CLAUDE_HOOK_PAYLOAD_MAX_BYTES: usize = 1_048_576;
const PATH_MAX_BYTES: usize = 4096;

/// The closed set of hooks Receipts registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaudeHookEvent {
    SessionStart,
    SessionEnd,
    PostToolUse,
    PreCompact,
    PostCompact,
    WorktreeCreate,
    WorktreeRemove,
}

impl ClaudeHookEvent {
    pub const ALL: [Self; 7] = [
        Self::SessionStart,
        Self::SessionEnd,
        Self::PostToolUse,
        Self::PreCompact,
        Self::PostCompact,
        Self::WorktreeCreate,
        Self::WorktreeRemove,
    ];

    /// The exact Claude Code hook event name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::PostToolUse => "PostToolUse",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::WorktreeCreate => "WorktreeCreate",
            Self::WorktreeRemove => "WorktreeRemove",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == name)
    }

    /// Only `WorktreeCreate` may block (it replaces default creation).
    pub fn is_blocking(self) -> bool {
        self == Self::WorktreeCreate
    }
}

/// Payload-free refusal; names a field at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeHookError {
    UnknownEvent,
    Oversized,
    InvalidUtf8,
    InvalidJson,
    NotAnObject,
    /// `hook_event_name` differs from the registered event argument.
    EventMismatch,
    InvalidField(&'static str),
}

impl std::fmt::Display for ClaudeHookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidField(name) => write!(f, "invalid hook field {name}"),
            other => write!(f, "invalid hook input: {other:?}"),
        }
    }
}

impl std::error::Error for ClaudeHookError {}

/// Event-specific bounded facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeHookDetail {
    SessionStart { source: Option<&'static str> },
    SessionEnd { reason: Option<&'static str> },
    PostToolUse { tool_name: String },
    Compact { trigger: Option<&'static str> },
    WorktreeCreate { name: String },
    WorktreeRemove { worktree_path: PathBuf },
}

/// One accepted hook invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHookInvocation {
    event: ClaudeHookEvent,
    session_id: String,
    cwd: PathBuf,
    detail: ClaudeHookDetail,
}

fn closed(value: Option<&V>, allowed: &[&'static str]) -> Option<&'static str> {
    match value {
        Some(V::String(s)) => allowed.iter().copied().find(|a| a == s),
        _ => None,
    }
}

fn token(value: &str, max: usize, extra: &[u8]) -> bool {
    (1..=max).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

fn absolute_path(value: Option<&V>, field: &'static str) -> Result<PathBuf, ClaudeHookError> {
    match value {
        Some(V::String(s))
            if s.len() <= PATH_MAX_BYTES && !s.contains('\0') && Path::new(s).is_absolute() =>
        {
            Ok(PathBuf::from(s))
        }
        _ => Err(ClaudeHookError::InvalidField(field)),
    }
}

/// Parses one hook invocation registered as `event_arg`.
pub fn parse_claude_hook(
    event_arg: &str,
    stdin: &[u8],
) -> Result<ClaudeHookInvocation, ClaudeHookError> {
    let event = ClaudeHookEvent::parse(event_arg).ok_or(ClaudeHookError::UnknownEvent)?;
    if stdin.len() > CLAUDE_HOOK_PAYLOAD_MAX_BYTES {
        return Err(ClaudeHookError::Oversized);
    }
    let text = std::str::from_utf8(stdin).map_err(|_| ClaudeHookError::InvalidUtf8)?;
    let V::Object(object) = V::parse_json(text).map_err(|_| ClaudeHookError::InvalidJson)? else {
        return Err(ClaudeHookError::NotAnObject);
    };
    let m = object.as_map();
    match m.get("hook_event_name") {
        Some(V::String(name)) if name == event.as_str() => {}
        _ => return Err(ClaudeHookError::EventMismatch),
    }
    let session_id = match m.get("session_id") {
        Some(V::String(s)) if token(s, 200, b"._:-") => s.clone(),
        _ => return Err(ClaudeHookError::InvalidField("session_id")),
    };
    let cwd = absolute_path(m.get("cwd"), "cwd")?;
    let detail = match event {
        ClaudeHookEvent::SessionStart => ClaudeHookDetail::SessionStart {
            source: closed(m.get("source"), &["startup", "resume", "clear", "compact"]),
        },
        ClaudeHookEvent::SessionEnd => ClaudeHookDetail::SessionEnd {
            reason: closed(
                m.get("reason"),
                &[
                    "clear",
                    "logout",
                    "prompt_input_exit",
                    "bypass_permissions_disabled",
                    "other",
                ],
            ),
        },
        ClaudeHookEvent::PostToolUse => match m.get("tool_name") {
            Some(V::String(s)) if token(s, 128, b"_.:-") => ClaudeHookDetail::PostToolUse {
                tool_name: s.clone(),
            },
            _ => return Err(ClaudeHookError::InvalidField("tool_name")),
        },
        ClaudeHookEvent::PreCompact | ClaudeHookEvent::PostCompact => ClaudeHookDetail::Compact {
            trigger: closed(m.get("trigger"), &["manual", "auto"]),
        },
        ClaudeHookEvent::WorktreeCreate => match m.get("name") {
            // A single safe path component: no separators, no leading dot or dash.
            Some(V::String(s)) if token(s, 64, b"._-") && !s.starts_with(['.', '-']) => {
                ClaudeHookDetail::WorktreeCreate { name: s.clone() }
            }
            _ => return Err(ClaudeHookError::InvalidField("name")),
        },
        ClaudeHookEvent::WorktreeRemove => ClaudeHookDetail::WorktreeRemove {
            worktree_path: absolute_path(m.get("worktree_path"), "worktree_path")?,
        },
    };
    Ok(ClaudeHookInvocation {
        event,
        session_id,
        cwd,
        detail,
    })
}

fn object(entries: Vec<(&str, String)>) -> OrchestrationJsonObjectV1 {
    OrchestrationJsonObjectV1::new(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), V::String(v)))
            .collect::<BTreeMap<_, _>>(),
    )
}

impl ClaudeHookInvocation {
    pub fn event(&self) -> ClaudeHookEvent {
        self.event
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    /// The host-reported working directory (untrusted, absolute).
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub fn detail(&self) -> &ClaudeHookDetail {
        &self.detail
    }

    /// Evidence that the host launched this Receipts hook from `plugin_root`
    /// (the host-set `CLAUDE_PLUGIN_ROOT`). It counts in observation only
    /// when it matches the installed plugin the probe reports.
    pub fn execution_evidence(&self, plugin_root: &Path) -> Option<HostHookExecutionEvidenceV1> {
        plugin_root
            .is_absolute()
            .then(|| HostHookExecutionEvidenceV1 {
                plugin_root: plugin_root.to_path_buf(),
            })
    }

    fn build(
        &self,
        event_type: NormalizedHostEventType,
        source_class: NormalizedHostEventSourceClass,
        payload: OrchestrationJsonObjectV1,
        confidence: NormalizedHostEventConfidence,
        event_id: NormalizedHostEventId,
        occurred_at: OrchestrationDateTimeV1,
    ) -> Option<NormalizedHostEvent> {
        let event = NormalizedHostEvent::try_new(NormalizedHostEventInputs {
            event_id,
            event_type,
            host: HostId::ClaudeCode.into(),
            host_session_id: self.session_id.clone(),
            project_id: None,
            occurred_at,
            payload,
            raw_ref: None,
            confidence,
        })
        .ok()?;
        // The frozen source policy is applied to every produced event.
        validate_normalized_host_event_source(source_class, &event).ok()?;
        Some(event)
    }

    /// The OBSERVED host-hook event for an observer hook; `None` for the
    /// worktree hooks, whose events depend on the handler's outcome.
    pub fn observed_event(
        &self,
        event_id: NormalizedHostEventId,
        occurred_at: OrchestrationDateTimeV1,
    ) -> Option<NormalizedHostEvent> {
        use NormalizedHostEventType as T;
        let some = |k: &'static str, v: Option<&'static str>| v.map(|v| (k, v.to_owned()));
        let (event_type, entries) = match &self.detail {
            ClaudeHookDetail::SessionStart { source } => (
                T::HostSessionStarted,
                some("start_source", *source).into_iter().collect(),
            ),
            ClaudeHookDetail::SessionEnd { reason } => (
                T::HostSessionEnding,
                some("end_reason", *reason).into_iter().collect(),
            ),
            ClaudeHookDetail::PostToolUse { tool_name } => {
                (T::ToolExecuted, vec![("tool_name", tool_name.clone())])
            }
            ClaudeHookDetail::Compact { trigger } => {
                let phase = if self.event == ClaudeHookEvent::PreCompact {
                    "PRE"
                } else {
                    "POST"
                };
                let mut entries = vec![("phase", phase.to_owned())];
                entries.extend(some("trigger", *trigger));
                (T::ContextCompacted, entries)
            }
            ClaudeHookDetail::WorktreeCreate { .. } | ClaudeHookDetail::WorktreeRemove { .. } => {
                return None;
            }
        };
        self.build(
            event_type,
            NormalizedHostEventSourceClass::HostHook,
            object(entries),
            NormalizedHostEventConfidence::Observed,
            event_id,
            occurred_at,
        )
    }

    /// WORKSPACE_CREATED for a worktree the handler actually created or
    /// attached. A plain no-task worktree (core not driving it) is INFERRED;
    /// a core task worktree is OBSERVED.
    pub fn workspace_created_event(
        &self,
        worktree_path: &Path,
        mode: &str,
        task_bound: bool,
        event_id: NormalizedHostEventId,
        occurred_at: OrchestrationDateTimeV1,
    ) -> Option<NormalizedHostEvent> {
        if self.event != ClaudeHookEvent::WorktreeCreate {
            return None;
        }
        self.build(
            NormalizedHostEventType::WorkspaceCreated,
            NormalizedHostEventSourceClass::CoreDriven,
            object(vec![
                ("worktree_path", worktree_path.to_str()?.to_owned()),
                ("mode", mode.to_owned()),
            ]),
            if task_bound {
                NormalizedHostEventConfidence::Observed
            } else {
                NormalizedHostEventConfidence::Inferred
            },
            event_id,
            occurred_at,
        )
    }
}

/// The current UTC instant as `YYYY-MM-DDTHH:MM:SS.mmmZ` (event and report
/// timestamps; the host clock is untrusted evidence, not ordering authority).
pub fn utc_now() -> OrchestrationDateTimeV1 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    utc_from_unix_millis(since.as_millis() as i64)
}

/// Civil-from-days (Howard Hinnant's algorithm) for non-negative millis.
pub fn utc_from_unix_millis(millis: i64) -> OrchestrationDateTimeV1 {
    let (days, ms) = (millis.div_euclid(86_400_000), millis.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    );
    OrchestrationDateTimeV1::try_new(text).expect("formatted from civil fields")
}

/// Exact `WorktreeCreate` command-hook stdout: the absolute path and one
/// newline, nothing else. `None` for a path Claude could not receive intact.
pub fn worktree_create_stdout(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    (path.is_absolute() && !text.contains(['\n', '\r', '\0'])).then(|| format!("{text}\n"))
}
