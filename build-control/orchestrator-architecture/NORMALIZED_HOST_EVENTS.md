<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# NORMALIZED_HOST_EVENTS

The single interface between host adapters and the core. Both adapters emit exactly this vocabulary; the core understands nothing else.

## Envelope

```
NormalizedHostEvent {
  event_id: string              # ULID
  event_type: EventType
  host: 'claude-code'|'codex'|'headless'
  host_session_id: string
  project_id: string?
  occurred_at: ISO8601
  payload: object               # typed per event_type
  raw_ref: string?              # pointer to raw host payload, not inlined
  confidence: 'OBSERVED'|'INFERRED'   # see below
}
```

**`confidence` is load-bearing.** An embedded host observes an event directly; a supervised host sometimes infers it from process output. Marking the difference prevents the core from treating a guess as a fact — and lets a later bug be traced to inference rather than to logic.

## Event vocabulary

`Source class` distinguishes what kind of thing is emitting the event, which is the axis that matters for provenance — not every event is, or should be, a host lifecycle hook:

- **HOST_HOOK** — a PRIMARY EMBEDDED HOST SOURCE: the host's own native plugin/hook system (registry C-02). This is the primary Codex integration, not a fallback.
- **WORKER_DISPATCH** — a FALLBACK / EXTERNAL WORKER SOURCE: an externally dispatched task worker's own output (e.g. `codex exec` JSONL). Task-worker events correctly derive from `codex exec` regardless of which posture the host integration itself is running in — this is not routed through native host hooks, and is not a sign that the host integration has fallen back to SUPERVISED.
- **ELICITATION** — a direct request/response turn with the user, independent of host hooks.
- **CORE_DRIVEN** — computed by the core itself; no host signal involved.

| Event | Meaning | Source class | Claude source | Codex source |
|---|---|---|---|---|
| `HOST_SESSION_STARTED` | Host session began | HOST_HOOK | `SessionStart` hook | `SessionStart` hook (fallback when SUPERVISED: supervisor start) |
| `HOST_SESSION_ENDING` | Session ending | HOST_HOOK | `SessionEnd` hook | `SessionEnd` hook (fallback when SUPERVISED: process exit) |
| `USER_GOAL_SUBMITTED` | User invoked START_GOAL | ELICITATION | command/skill | command wrapper |
| `USER_INPUT_PROVIDED` | Response to a core prompt | ELICITATION | elicitation | supervisor prompt (this is a direct user turn, not a host-hook path, in either posture) |
| `ROLE_EXECUTOR_STARTED` | Executor bound to a logical role | CORE_DRIVEN | core-driven | core-driven |
| `ROLE_EXECUTOR_STOPPED` | Executor released (with reason) | CORE_DRIVEN | core-driven | core-driven |
| `TASK_STARTED` | A3/A4 attempt began | WORKER_DISPATCH | adapter dispatch | adapter dispatch |
| `TASK_COMPLETED` | Attempt finished with a result | WORKER_DISPATCH | handoff | `codex exec` exit + JSONL |
| `TASK_FAILED` | Attempt failed (classified) | WORKER_DISPATCH | error/exit | non-zero exit |
| `TOOL_EXECUTED` | A tool/command ran | HOST_HOOK / WORKER_DISPATCH | `PostToolUse` hook | JSONL event from the dispatched worker's `codex exec` run — a worker-lifecycle signal, not a host-hook substitute |
| `WORKSPACE_CREATED` | Worktree created | CORE_DRIVEN | `WorktreeCreate` | core-driven |
| `WORKSPACE_CHANGED` | Files changed | WORKER_DISPATCH | `FileChanged` | polling/diff over the dispatched worker's workspace |
| `WORKSPACE_REMOVED` | Worktree removed | CORE_DRIVEN | `WorktreeRemove` | core-driven |
| `CONTEXT_COMPACTED` | Host compacted context | HOST_HOOK | `PreCompact`/`PostCompact` hooks | `PreCompact`/`PostCompact` hooks (fallback when SUPERVISED: inferred) |
| `PROVIDER_SIGNAL` | Rate limit / auth / safety signal observed | WORKER_DISPATCH | error surface | JSONL/stderr classification |
| `HOST_ERROR` | Host-level failure | HOST_HOOK | any | any |

## Design rules

1. **Normalized, not lowest-common-denominator.** Where one host provides richer signal (Claude's `PreCompact`), the event exists for both; the other emits it as `INFERRED` or not at all. The core handles absence explicitly.
2. **No host-specific fields in payloads.** Host specifics go behind `raw_ref`.
3. **Events are facts, not commands.** An event never instructs the core.
4. **All events are persisted** to the event log before the core acts on them, so a crash mid-handling is recoverable.
5. **Unknown event types are logged and ignored**, never inferred into a known type.

## Core → host direction

```
CoreView { view_type: 'goal_status'|'dag'|'routing_decision'|'review_result'
                    |'integration_result'|'goal_evaluation'|'error'
           content: object            # structured, not prose
           render_policy: 'TEMPLATE'|'ECONOMY_RENDER'|'RAW' }
```

The core emits **structure**. Prose rendering happens at the edge, and per I-13 a renderer may present authoritative state but never alter it. For simple status, `TEMPLATE` uses no model at all.

## Ownership

`BUILD-A2-HOST-INTEGRATION` owns this vocabulary. Adding an event type is a contract change requiring both adapters to be updated together, so the vocabulary cannot drift into a Claude-shaped interface with a Codex translation layer bolted on.
