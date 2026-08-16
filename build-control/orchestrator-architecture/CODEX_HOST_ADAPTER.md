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

# CODEX_HOST_ADAPTER

Host capability freshness and mode selection follow `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`; session entry verifies the cached host/install report before any EMBEDDED activation.

**Posture: EMBEDDED (native plugin + hooks) is primary as of V1.3.1.** Supervised and hybrid remain supported fallbacks, selected by discovery.

> **A-14 is RETIRED.** Registry claims `C-01` and `C-02` are `VERIFIED_CURRENT_SELF_FETCHED` (2026-08-15): Codex has native plugins with `.codex-plugin/plugin.json`, skills, and lifecycle hooks including `SessionStart`, `SessionEnd`, `SubagentStart`, `SubagentStop`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `UserPromptSubmit` and `Stop`. The 2026-08-13 statement that no such system existed is preserved only as a dated historical observation.
>
> **Hooks are not an enforcement boundary**, per OpenAI's own documentation: specialized tool paths can opt out (`C-02c`), plugin hooks are untrusted until reviewed (`C-02a`), and hooks can be disabled entirely (`C-02b`). Entitlement and security authority remain in the shared core.

**Verification status:** plugin and hook facts are `VERIFIED_CURRENT_SELF_FETCHED` (registry `C-01`, `C-02`, fetched 2026-08-15). `codex exec` flag details remain `VERIFIED_HISTORICAL` (`C-06`) and are **probed at install** — current evidence is not a permanent assumption.

## Architecture

**Primary — native plugin + hooks.**

```
        Codex (user's session)
              │
   .codex-plugin/plugin.json + hooks.json   ← installed plugin, trusted hooks
              │
              ▼
      CodexHostAdapter (in-process)
              │
              ▼
      NormalizedHostEvent → shared core
```

The shared core owns state; the plugin is a translation shell that emits normalized events from native lifecycle hooks. This is the same shape as `ClaudeHostAdapter` — see `HOST_ARCHITECTURE.md`.

**Fallback — supervised/hybrid compatibility path.** Selected by `HostCapabilityReport` only when the native path cannot safely operate (plugin absent, hooks unsupported/unconfigured/untrusted/disabled/excluded by admin policy, insufficient coverage, or a specialized operation that genuinely requires worker-side execution):

```
        Codex (user's session)
              │
   AGENTS.md + config.toml           ← in-session context
              │
              ▼
   ┌───────────────────────────┐
   │  Orchestrator companion   │   ← owns the core in fallback
   │  process (supervisor)     │
   └────────────┬──────────────┘
                │ codex exec --json …
                ▼
        Codex worker invocations
```

In fallback, the companion process owns the core and the state store, and events are derived rather than observed. `codex exec` is a worker/runtime dispatch mechanism in both postures — used whenever a Codex *worker* is dispatched, regardless of which posture the *host* integration is running in — never the primary host-integration path itself.

## Why supervision remains fully supported

Supervision is the **compatibility fallback**, selected when native lifecycle signal is unavailable at runtime — not because Codex lacks a hook system. It remains valuable precisely because plugin hooks can be present yet inactive: untrusted, disabled, or excluded by admin policy.

In fallback, events that cannot be directly observed are marked `INFERRED`. That honesty about observed-versus-derived is retained; what has changed is that it is now the exception rather than the rule.

## Event derivation

| Normalized event | Source | Confidence |
|---|---|---|
| `HOST_SESSION_STARTED` / `_ENDING` | **`SessionStart` / `SessionEnd` hooks** | OBSERVED |
| `USER_GOAL_SUBMITTED` | command wrapper | OBSERVED |
| `TASK_STARTED` / `TASK_COMPLETED` / `TASK_FAILED` | `codex exec` invocation + exit + JSONL | OBSERVED |
| `TOOL_EXECUTED` | **`PostToolUse` hook** | OBSERVED |
| `WORKSPACE_*` | core-driven (orchestrator creates worktrees directly) | OBSERVED |
| `CONTEXT_COMPACTED` | **`PreCompact` / `PostCompact` hooks** | **OBSERVED** |
| `PROVIDER_SIGNAL` | stderr/exit classification | INFERRED |
| `ROLE_EXECUTOR_STARTED` / `_STOPPED` | **`SubagentStart` / `SubagentStop` hooks** | OBSERVED |

Codex worktree events are `OBSERVED` because the orchestrator creates the worktrees itself rather than intercepting a host action — the supervised posture is actually *more* direct here than the embedded one.

## Invocation contract

Workers are dispatched with the verified non-interactive surface (A-10): `codex exec` with `--json` for JSONL events, `--output-schema` where structured results are required, `--output-last-message` for the final payload, `-C` to bind the working directory, `--sandbox read-only` for reviewers and `workspace-write` for implementers, and `--ignore-user-config` / `--ignore-rules` for deterministic behaviour.

`--full-auto` is **never** used (deprecated per A-11). `--dangerously-bypass-approvals-and-sandbox` is never used.

## Sandbox

Codex's OS-enforced sandbox (A-12) is the isolation mechanism: `read-only` for A4 reviewers, `workspace-write` for A3 implementers with network off by default. Where the environment cannot support Landlock/bubblewrap, the adapter reports reduced isolation rather than silently proceeding.

## User command surface

`START_GOAL` is exposed through Codex's supported extension surface. Exact syntax is **Q-01**, pending current verification. The requirement is semantic — a superior long-horizon execution entry point — not the capture of any reserved command name.

## Cross-host state

The SHARED LOCAL CORE / STATE LAYER owns durable state regardless of whether Codex host mode is EMBEDDED, HYBRID or SUPERVISED — that is what makes `CROSS_HOST_RESUME.md` work: the state was never inside either host. The companion process only accesses that same core/state store in the SUPERVISED and HYBRID fallback modes; in EMBEDDED it plays no role, since the native plugin/hooks path talks to the shared core directly.

## Event provenance improves materially

With native hooks, most Codex events become `OBSERVED` rather than `INFERRED` — including context compaction, which V1.3 could only infer. That is a real fidelity gain and the direct payoff of the verification.

## Known limitations

- Our hooks do not run until the user trusts them (`C-02a`). Installing the plugin does not automatically trust its hooks; **new or changed** hook definitions/hashes are marked for review and skipped until trusted. An update that leaves the hook definition/hash unchanged does not by itself require re-trust.
- Hooks may be disabled by user or admin configuration (`C-02b`); the adapter must detect this and fall back rather than silently observing nothing.
- `SessionEnd` allows at most 3 seconds (`C-02d`), so no meaningful work belongs there.
- With native hooks trusted (EMBEDDED), in-session integration is deep on both hosts; only the SUPERVISED fallback is shallower than Claude Code's, and parity is maintained at the capability level even there (`HOST_PARITY_CONTRACT.md`), not the ergonomic level.
- ChatGPT sign-in and `codex login` are **documented technical capabilities** (registry `C-11`). The unresolved issue is not technical but commercial: `OPENAI / CHATGPT_CONSUMER_SUBSCRIPTION / THIRD_PARTY_PAID_EXTERNAL_WORKER` is `POLICY_NEEDS_REVIEW` and **not routable by default**. Technical support and commercial permission are different axes.
