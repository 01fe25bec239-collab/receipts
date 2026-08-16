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

# HOST_ARCHITECTURE

## Shape

```
              USER
                │
     ┌──────────┴──────────┐
     │                     │
Claude Code              Codex
     │                     │
     ▼                     ▼
ClaudeHostAdapter    CodexHostAdapter
     │                     │
     └────────┬────────────┘
              ▼
       NORMALIZED HOST EVENTS
              ▼
       SHARED ORCHESTRATOR CORE
              ▼
        DURABLE STATE STORE
```

## Activation

```
host = detectHost()          # env markers, parent process, explicit config
if host == CLAUDE_CODE: activate(ClaudeHostAdapter)
elif host == CODEX:     activate(CodexHostAdapter)
else:                   activate(HeadlessAdapter)   # CI / direct CLI; parity-exempt
```

Detection is explicit and overridable. A misdetected host is a startup error, never a silent fallback — silently guessing the host would produce a subtly wrong event stream that is very hard to debug later.

## What an adapter is, and is not

**An adapter is:** a translation shell. It converts host-native signals into normalized events, and normalized core intents into host-native actions.

**An adapter is not:** a place for orchestration logic. No adapter may contain DAG logic, routing logic, role lifecycle, repair decisions, or integration decisions. If a behaviour differs between hosts because the adapters implemented it differently, the architecture has already failed.

Enforcement: adapters depend on the core; the core has **no** compile-time dependency on any adapter. A dependency-direction test enforces this in CI.

## Core boundary interface

```
interface HostAdapter {
  id: 'claude-code' | 'codex' | 'headless'
  detect(): boolean
  install(): InstallPlan          // what the user must do, if anything
  start(core: CoreHandle): void
  emit(event: NormalizedHostEvent): void      // host → core
  present(view: CoreView): void               // core → host UI
  requestUserInput(prompt: UserPrompt): Promise<UserResponse>
  capabilities(): HostCapabilityReport
  shutdown(reason): void
}
```

`capabilities()` reports what the host natively supports so the core can choose a strategy — never so it can change orchestration semantics.

## Posture is discovered, not hardcoded (V1.3)

V1.2.3 fixed Claude as embedded and Codex as supervised. That encoded a vendor-capability snapshot as a structural decision, and §9 correctly flagged it as stale.

V1.3 replaces it with `HOST_CAPABILITY_DISCOVERY.md`: each adapter probes its host and selects **EMBEDDED**, **SUPERVISED**, or **HYBRID**. `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json` requires a lightweight validity check on session start/resume and re-probe on changed/unproven validity; no stale report authorizes EMBEDDED. Both hosts satisfy the same parity contract in any mode. Both hosts' current primary posture is EMBEDDED.

Codex native plugins and hooks are **`VERIFIED_CURRENT_SELF_FETCHED`** (registry `C-01`, `C-02`, fetched 2026-08-15). Discovery remains mandatory nonetheless: a host supporting hooks is not the same as *our* hooks being configured, trusted, enabled and permitted at runtime.

## Reference postures (selected by discovery, not by vendor name)

**Embedded (Claude Code).** The orchestrator installs as a plugin. Hooks provide real-time lifecycle signal; skills and commands provide the UX. The core still runs as the same process/module set; the host provides events and a surface, not the logic.

**Embedded (Codex) — CURRENT PRIMARY.** Codex has native plugins and lifecycle hooks (registry `C-01`, `C-02`, `VERIFIED_CURRENT_SELF_FETCHED`). The orchestrator installs as a Codex plugin; hooks emit `NormalizedHostEvent` into the shared core:

```
CODEX → native plugin / lifecycle hooks → CodexHostAdapter → NormalizedHostEvent → shared core
```

**Supervised / hybrid (any host) — COMPATIBILITY FALLBACK ONLY.** A companion supervisor owns the core and events are derived from process output, marked `INFERRED` where they cannot be observed. Selected by capability discovery when: the host version predates plugin hooks; hooks are disabled; our plugin hooks are untrusted; `allow_managed_hooks_only` excludes them; a required lifecycle signal is unsupported; a specialized tool path falls outside hook coverage; or the operation genuinely requires worker-side `codex exec`.

Fallback is **not** equal to the current primary posture. A-14 is **RETIRED** as current architecture.

This asymmetry is deliberate and documented rather than hidden. Both hosts now have a native hook mechanism to integrate against; fallback exists for each for different reasons — on Codex, runtime conditions (trust, configuration, admin policy, coverage) that vary per install; on Claude Code, negligible benefit, since the native path is available essentially whenever the host is. Forcing supervision onto Claude Code would waste a capability the host already provides for free.

## Worker vs host — a distinction that matters

Claude Code and Codex each appear in two unrelated roles:

| Role | Meaning |
|---|---|
| **Host** | The surface the *user* is sitting in when they start a goal |
| **Worker runtime** | A runtime the *router* may dispatch a RUNTIME-A3 or A4 to |

They are independent. A goal started in Claude Code may dispatch a Codex worker, and vice versa. Nothing in the host choice constrains routing, and nothing in routing constrains the host.

## Headless adapter

A third, parity-exempt adapter for CI and direct CLI use. It has no interactive surface and no user prompts; policies requiring user input resolve to their configured non-interactive fallback. It exists to keep the core honest: if the core needs a host to function, it is not really a core.

## Shared local core process model

**MVP: on-demand local core invocation** (Option A). Durability already comes from the state store, not from process residency, so a daemon would add supervision, stale-process cleanup and idle footprint to solve a solved problem. Full comparison in `GRAPH_RUNTIME_ARCHITECTURE.md`. Option C (hybrid) is the upgrade path if measured latency requires it; the core interface is identical either way.
