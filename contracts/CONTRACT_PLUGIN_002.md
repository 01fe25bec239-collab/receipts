# CONTRACT-PLUGIN-002 — Broker → Hook Decision / Error Response

**Version:** 1.0.0  
**Owner:** A2-CLAUDE-INTEGRATION  
**Consumers:** A2-CORE, A2-INTEGRITY-SECURITY  
**Status:** FROZEN  
**First milestone:** M3  
**Depends on:** PLUGIN-001, POLICY-002, CLI-001, current Claude hook output rules  
**Architecture correction of record:** ADR-001 (APPROVED 2026-08-09)

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Map stable broker decisions to Claude's current hook control surfaces.

## Normative internal decision

```text
HookAction = NO_DECISION | ALLOW | DENY | BLOCK_TASK | CONTEXT

HookDecision {
  action: HookAction
  reason: string?
  additionalContext: string?
  admission: AdmissionDecision?
}
```

## Frozen current mappings

### TaskCompleted
- ADMIT -> exit 0.
- BLOCK -> exit 2, bounded factual stderr naming unmet claims/changed paths.
- ADMIT_WITH_OVERRIDE -> exit 0 and, if rendered, exact override label.
- broker/config/storage failure -> exit 2 (gate fails closed).

### PreToolUse
Protected Bash merge/push while blocked and protected config/ledger edits return exit 0 with current JSON:
```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"..."}}
```
Non-protected -> exit 0/no decision. JSON decision and nonzero exit MUST NOT be mixed.

### PostToolUse
Observer only; async; never blocks.

### PostToolBatch
Recompute fingerprint/staleness; Receipts deliberately does not block here.

### SessionStart / SubagentStart / SubagentStop / Stop
Factual context/provenance only; no imperative prompt injection. `SessionStart` additionally carries the observational workspace-identity binding described in PLUGIN-001; it emits no decision.

### Worktree events — not mapped (ADR-001, APPROVED)
Receipts installs **no** `WorktreeCreate` hook and **no** `WorktreeRemove` hook in MVP, so this contract defines **no** decision encoding for either event. There is no "always exit 0" worktree handler, because there is no worktree handler.

`WorktreeCreate` is excluded because configuring it replaces Claude Code's default Git worktree creation and requires the hook to create and return the worktree path; any non-zero exit aborts creation. Receipts does not own worktree creation. `WorktreeRemove` is excluded on its own merits, not for symmetry: it grants no decision control, no MVP requirement depends on early removal notification, and there is unresolved evidence that configuring it may also displace default cleanup. Workspace cleanup remains Claude Code's / Git's responsibility and workspace-binding invalidation is lazy.

Emitting any decision, JSON, exit code, or context for either event is a contract violation.

## Output limits

Every hook-facing string < 10,000 characters. Truncate display detail by retaining structured refs, never by changing decision semantics.

## Failure behavior

Gate failure = fail closed. Observer failure = fail open/nonblocking plus diagnostic where safe. Errors never dump credentials/raw logs.

## Compatibility/versioning

Transport syntax changes that preserve internal semantics are adapter patch/minor. Changing fail-open/fail-closed event behavior is contract/architecture change.

## Security constraints

Permission deny rules remain hard-control defense where architecture requires. Hook messages factual. ADMITTED_WITH_OVERRIDE never displayed as VERIFIED.

## Examples

TaskCompleted BLOCK stderr:
`Receipts: AUTH-42 BLOCKED — TESTED is STALE; changed: src/auth/store.ts`

## Negative examples

Exit 2 plus JSON output; infrastructure error allowing protected push; imperative additionalContext; override rendered verified; shipping a `WorktreeCreate` or `WorktreeRemove` handler of any kind, including an always-exit-0 no-op.

## Field semantics

`action` is the broker's hook-independent control intent. `reason` is bounded factual human/agent-facing explanation. `additionalContext` is factual context only. `admission` carries the recomputed admission artifact when the decision is admission-driven. Claude-specific exit/JSON encoding happens only in the adapter mapping sections below.

## Required fields

`HookDecision.action` is required. For `DENY`/`BLOCK_TASK`, a non-empty factual `reason` is required. Admission-driven gate decisions require `admission`.

## Optional fields

`reason`, `additionalContext`, and `admission` are optional only when not required by the selected action/mapping.

## Invariants

Internal broker decisions are transport-independent. JSON permission decisions and nonzero-exit decisions are never mixed. Gate failures fail closed; observation-only failures do not halt the agent loop. Override state is never rendered as ordinary proof/admission.

## Validation rules

Action/field combinations MUST be checked before encoding. Hook event must match the expected decision encoding. All emitted strings MUST remain below Claude's current hook output cap and MUST NOT include secrets/raw logs. Encoding a decision for an event Receipts does not install — including `WorktreeCreate` and `WorktreeRemove` — is a validation failure, not a fallback path.

## Current permission-rule transport note

Current Claude Code file-path permission checks use `Read(path)` and `Edit(path)`; `Edit(path)` covers built-in file-editing tools, while path-scoped `Write(path)`/`NotebookEdit(path)` rules are accepted but not consulted. Receipts MUST therefore generate `Read(...)` + `Edit(...)` path denies, never `Write(path)` as the hard file-tool rule. Absolute paths use Claude's current `//absolute/path/**` permission syntax.

Plugin `settings.json` currently does not carry arbitrary permission rules, so Receipts MUST NOT pretend those rules are automatically installed by plugin packaging. Any permission-rule installation/configuration is a human-visible supported Claude settings operation. This transport constraint does not change the architecture's broker-only-write invariant or make plugin settings an authority path.

## External interface source

- Claude Code Hooks reference — `https://code.claude.com/docs/en/hooks` — accessed 2026-08-09.
- Claude Code Permissions reference — `https://code.claude.com/docs/en/permissions` — accessed 2026-08-09.
- Claude Code Plugins reference — `https://code.claude.com/docs/en/plugins-reference` — accessed 2026-08-09. Exit/JSON transport mappings MUST be reverified before implementation if Claude Code changes.
- `WorktreeCreate` / `WorktreeRemove` exclusion evidence and the unresolved third-party conflict are recorded in `ARCHITECTURE_DEVIATION_REQUEST_001.md`.

## Required tests

TaskCompleted mapping; exact PreToolUse JSON; exit/JSON exclusivity; observer fail-open; gate fail-closed; output cap; override rendering; a negative test asserting the encoder refuses to produce output for `WorktreeCreate` or `WorktreeRemove`; a packaging test asserting neither event appears in the shipped `hooks/hooks.json`.
