# CONTRACT-PLUGIN-001 — Normalized Hook → Broker Request

**Version:** 1.0.0  
**Owner:** A2-CLAUDE-INTEGRATION  
**Consumers:** A2-CORE, A2-INTEGRITY-SECURITY, A2-LEDGER  
**Status:** FROZEN  
**First milestone:** M3  
**Depends on:** CORE-002, CLI-001, current Claude hook schemas  
**Architecture correction of record:** ADR-001 (APPROVED 2026-08-09)

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Normalize mutable Claude hook JSON into a stable minimal broker request.

## Normative envelope

```text
HookEvent =
  SessionStart | PostToolUse | PostToolBatch | SubagentStart | SubagentStop |
  TaskCompleted | PreToolUse | Stop

NormalizedHookRequest {
  version: 1
  event: HookEvent
  sessionId: string
  cwd: AbsolutePath
  agent: { agentId: string?, agentType: string? }?
  payload: EventPayload
}
```

## Event payloads

```text
SessionStartPayload { source: string? }

PostToolUsePayload {
  toolName: string
  filePath: string?
}

PostToolBatchPayload {
  changed: boolean
}

SubagentStartPayload {
  agentId: string
  agentType: string?
}

SubagentStopPayload {
  agentId: string
  agentType: string?
  lastAssistantMessage: string?
}

TaskCompletedPayload {
  taskId: string
  taskSubject: string
  taskDescription: string?
  teammateName: string?
  teamName: string?
}

PreToolUsePayload {
  toolName: string
  command: string?
  filePath: string?
}

StopPayload { lastAssistantMessage: string? }
```

## Field semantics

Raw hook input is untrusted client input. Agent identity is provenance only. Unknown upstream fields are ignored. `teamName` is compatibility-only because current upstream marks it deprecated. Raw Bash command is parser input only; never executable authority.

## Required / optional

sessionId/cwd/event/payload required. Agent optional where hook does not provide identity. Event payload optional fields as shown.

## Invariants

Normalizer never produces evidence or admission. No raw input becomes shell syntax. Current hook-field changes are absorbed at adapter edge where normalized meaning remains unchanged. Receipts never installs a hook that replaces a default Claude Code or Git behavior in order to observe it; observation that can be obtained read-only from `cwd`, repository identity, and Git metadata MUST be obtained that way.

## Validation

Strict JSON parse. Event-specific required fields checked. Paths/commands size-bounded. Missing required gate fields are errors.

## Failure behavior

Gating event normalization failure fails closed through PLUGIN-002. Observational event failure does not block user work.

## Workspace identity binding (ADR-001, APPROVED)

Receipts **MUST NOT** install a `WorktreeCreate` hook in MVP, and **MUST NOT** implement custom Git worktree creation. Claude Code and Git remain responsible for normal Git worktree creation. `WorktreeRemove` is likewise **not** installed in MVP; workspace cleanup remains Claude Code's / Git's responsibility. Neither event appears in `HookEvent`, and the normalizer **MUST NOT** accept them in MVP.

Workspace identity is instead bound **observationally**, with no hook of its own, from:

- `SessionStart` and the normalized `cwd`;
- repository identity per `CONTRACT-CORE-001`;
- read-only Git worktree metadata discovered by the broker under `CONTRACT-PROCESS-001` (explicit argv, no shell, read-only);
- the `cwd` of any normal broker invocation.

Workspace-binding invalidation is **lazy**: a binding that no longer resolves to an existing worktree is discarded at the next `SessionStart` or the next broker invocation that observes it. Receipts emits no removal notification and takes no cleanup action.

`WorktreeCreate` and `WorktreeRemove` remain **reserved names**. Re-introducing either is a `CONTRACT_CHANGE_REQUEST` against this contract, not an implementation decision, and requires a local version smoke test under `OI-009`.

## Compatibility/versioning

Upstream aliases/fixture updates are patch-compatible. Stable envelope/event semantic changes require contract change.

## Security constraints

No authority from agent-supplied fingerprint/task state. No command execution. Factual bounded data only.

## Example

```json
{"version":1,"event":"TaskCompleted","sessionId":"abc123","cwd":"/repo","agent":{"agentId":"a17","agentType":"implementer"},"payload":{"taskId":"task-001","taskSubject":"Implement auth"}}
```

## Negative examples

Using deprecated team_name as durable identity; executing tool_input.command; trusting agent-provided code-state identity; installing a `WorktreeCreate` hook in any form, including a "harmless" observer, because doing so replaces Claude Code's default worktree creation; installing `WorktreeRemove` for symmetry; implementing custom worktree creation inside Receipts.

## Current permission-rule transport note

Current Claude Code file-path permission checks use `Read(path)` and `Edit(path)`; `Edit(path)` covers built-in file-editing tools, while path-scoped `Write(path)`/`NotebookEdit(path)` rules are accepted but not consulted. Receipts MUST therefore generate `Read(...)` + `Edit(...)` path denies, never `Write(path)` as the hard file-tool rule. Absolute paths use Claude's current `//absolute/path/**` permission syntax.

Plugin `settings.json` currently does not carry arbitrary permission rules, so Receipts MUST NOT pretend those rules are automatically installed by plugin packaging. Any permission-rule installation/configuration is a human-visible supported Claude settings operation. This transport constraint does not change the architecture's broker-only-write invariant or make plugin settings an authority path.

## External interface source

- Claude Code Hooks reference — `https://code.claude.com/docs/en/hooks` — accessed 2026-08-09. A configured `WorktreeCreate` hook replaces default Git worktree creation and must return the created worktree path; any non-zero exit aborts creation. `WorktreeRemove` has no decision control and its failures are logged in debug mode only.
- Claude Code Permissions reference — `https://code.claude.com/docs/en/permissions` — accessed 2026-08-09.
- Claude Code Plugins reference — `https://code.claude.com/docs/en/plugins-reference` — accessed 2026-08-09. Hook field names and event behavior are adapter-edge dependencies, not Receipts domain authority.

## Required tests

Golden current-doc fixtures for each event in `HookEvent`; unknown fields; missing fields; deprecated team name; hostile command string; long data; a negative packaging test asserting that the shipped `hooks/hooks.json` declares **no** `WorktreeCreate` and **no** `WorktreeRemove` entry; a normalizer test asserting that a `WorktreeCreate` or `WorktreeRemove` event name is rejected as an unsupported event rather than silently normalized; a workspace-binding test asserting identity is derived from `SessionStart` `cwd` plus repository identity plus read-only Git worktree metadata, with no hook installed.
