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

# CLAUDE_HOST_ADAPTER

Host capability freshness and mode selection follow `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`; session entry verifies the cached host/install report before any EMBEDDED activation.

**Posture:** embedded. Installs as a Claude Code plugin and uses native lifecycle signal.

**Verification status:** built on registry claims `C-04`, `C-05`, `C-07` (reviewer-supplied current primary sources). A-05, A-06, A-09 are `UNVERIFIED` and the design does not depend on them.

## Installation surface

```
<plugin-root>/
  .claude-plugin/plugin.json
  hooks/hooks.json
  skills/orchestrator/SKILL.md
  commands/goal.md
  agents/                       # none required at MVP
```

The plugin registers components on enable. It does **not** install permission rules — those are a human/admin action in `settings.json` layers, and the adapter surfaces an `InstallPlan` telling the user exactly what to add rather than pretending it configured itself.

## Hook mapping

| Hook | Normalized event | Blocking? |
|---|---|---|
| `SessionStart` | `HOST_SESSION_STARTED` | no |
| `SessionEnd` | `HOST_SESSION_ENDING` | no |
| `PostToolUse` | `TOOL_EXECUTED` | no (observer) |
| `FileChanged` | `WORKSPACE_CHANGED` | no |
| `PreCompact` / `PostCompact` | `CONTEXT_COMPACTED` | no |
| `WorktreeCreate` | `WORKSPACE_CREATED` | **yes — replaces default** |
| `WorktreeRemove` | `WORKSPACE_REMOVED` | no (fire-and-forget) |
| `SubagentStart` / `SubagentStop` | `ROLE_EXECUTOR_*` | no |
| `TaskCompleted` | `TASK_COMPLETED` | can block |
| `HOST_ERROR` sources | `HOST_ERROR` | varies |

### WorktreeCreate — the deliberate reversal of ADR-001

Per registry claim `C-05`, configuring `WorktreeCreate` **replaces** Claude Code's default git worktree logic: the hook must create the working copy and print its absolute path, and any non-zero exit aborts creation.

The old Receipts architecture refused this because Receipts had no business owning workspace creation. **The orchestrator's core responsibility is workspace lifecycle**, so the same mechanism is now exactly right — this is the one place where the product genuinely wants to be the authority.

Consequences accepted deliberately:
- the handler must be **correct**, not trivial — a bug breaks worktree creation for the user;
- it must be fast, because it sits on an interactive path;
- it must fall back cleanly: if the orchestrator core is unreachable, the handler creates a plain `git worktree` and reports `INFERRED`, so the user is never stuck.

Full record in `ADR_IMPACT_MATRIX.md`.

## Observer discipline

Every non-blocking hook fails open. An orchestrator bug must never wedge the user's Claude Code session. Only `WorktreeCreate` (which cannot be non-blocking by design) and any explicitly gate-classified hook may block, and each has a defined fallback.

## Output bounds

Per A-06, exact per-event output caps are `UNVERIFIED`. The adapter bounds every hook-facing string defensively and truncates display detail while preserving structured references — the same discipline the old architecture applied, retained because it costs nothing and the cap is unknown.

## Worker path (separate concern)

When the router dispatches a **Claude worker**, that goes through the Claude *runtime adapter* (`RUNTIME_ADAPTER_INTERFACE.md`) via `claude -p`, not through this host adapter. Flag spellings are probed at install time because A-05 is `UNVERIFIED`.

## Sandbox

Claude Code's native sandbox (A-07) is used where available for worker execution. Windows requires WSL2; on native Windows the adapter reports reduced isolation rather than claiming a sandbox it does not have.

## Known limitations

- Permission rules cannot be auto-installed; the plugin surfaces instructions.
- Path-scoped permission rules do not constrain subprocesses (A-08), so write-scope enforcement lives in the workspace layer, not in permission config.
- Subagent frontmatter honouring is `UNVERIFIED` (A-09); nothing depends on it.
