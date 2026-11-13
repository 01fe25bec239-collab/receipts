//! Fixture-backed shared observer-hook shape, not installed-host qualification.
//! Reuses bounded parsing, closed payload fields and source policy from Claude.
use crate::claude::{ClaudeHookError, parse_claude_hook};
use crate::{
    HostId, NormalizedHostEvent, NormalizedHostEventConfidence, NormalizedHostEventId,
    NormalizedHostEventInputs, NormalizedHostEventSourceClass,
    validate_normalized_host_event_source,
};
use receipts_orchestration::orchestration::OrchestrationDateTimeV1;

/// Maps only observer events shared by the frozen host contracts. Subagent,
/// task, workspace and goal events require independently supplied Core/worker
/// or elicitation evidence and cannot be manufactured from raw hook payloads.
/// Raw hook claims confer no hook trust or positive enablement authority.
pub fn normalize_codex_hook(
    event_name: &str,
    stdin: &[u8],
    event_id: NormalizedHostEventId,
    occurred_at: OrchestrationDateTimeV1,
) -> Result<NormalizedHostEvent, ClaudeHookError> {
    if !matches!(
        event_name,
        "SessionStart" | "SessionEnd" | "PostToolUse" | "PreCompact" | "PostCompact"
    ) {
        return Err(ClaudeHookError::UnknownEvent);
    }
    let parsed = parse_claude_hook(event_name, stdin)?;
    match parsed.detail() {
        crate::claude::ClaudeHookDetail::SessionStart { source: None }
        | crate::claude::ClaudeHookDetail::SessionEnd { reason: None }
        | crate::claude::ClaudeHookDetail::Compact { trigger: None } => {
            return Err(ClaudeHookError::InvalidField("observer detail"));
        }
        _ => {}
    }
    let event = parsed
        .observed_event(event_id, occurred_at)
        .ok_or(ClaudeHookError::UnknownEvent)?;
    let event = NormalizedHostEvent::try_new(NormalizedHostEventInputs {
        event_id: event.event_id().clone(),
        event_type: event.event_type(),
        host: HostId::Codex.into(),
        host_session_id: event.host_session_id().into(),
        project_id: None,
        occurred_at: event.occurred_at().clone(),
        payload: event.payload().clone(),
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    })
    .map_err(|_| ClaudeHookError::InvalidJson)?;
    validate_normalized_host_event_source(NormalizedHostEventSourceClass::HostHook, &event)
        .map_err(|_| ClaudeHookError::UnknownEvent)?;
    Ok(event)
}

/// Typed input from the supervised composition, never evidence of a native hook.
/// This does not parse or execute the later Codex JSONL companion protocol.
pub fn normalize_codex_supervised_event(
    mut inputs: NormalizedHostEventInputs,
) -> Result<NormalizedHostEvent, crate::NormalizedHostEventError> {
    inputs.host = HostId::Codex.into();
    inputs.confidence = NormalizedHostEventConfidence::Inferred;
    NormalizedHostEvent::try_new(inputs)
}
