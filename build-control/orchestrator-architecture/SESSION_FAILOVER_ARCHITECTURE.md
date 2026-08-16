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

# SESSION_FAILOVER_ARCHITECTURE

## The invariant (§56, §57, I-1)

```
LOGICAL ROLE  ≠  LLM SESSION
```

```
RUNTIME-A2-AUTH                          ← durable: ownership, branch, decisions, history
    ├── binding#1  provider P, model M   [RELEASED — rate limited]
    └── binding#2  provider Q, model N   [ACTIVE]
```

Same A2. Only the binding changed.

## Executor binding record

```
ExecutorBinding {
  binding_id, role_id, provider_id, model_id, runtime_id,
  session_ref?, bound_at, released_at?,
  release_reason: RATE_LIMITED | SESSION_EXHAUSTED | AUTH_REQUIRED | PROVIDER_DOWN
                | CRASH | HOST_SWITCH | USER_REQUEST | COMPLETED | LEASE_EXPIRED,
  routing_decision_id
}
```

Bindings are an append-only history: the sequence of executors that served a role is itself evidence.

## Manager failover policies (§58)

| Policy | Behaviour | Use |
|---|---|---|
| `STRICT` | Wait for the same model/provider | Reproducibility-critical projects |
| **`FRONTIER_FAILOVER`** | Rebind to another eligible frontier executor | **Recommended default** |
| `ASK` | Ask the user before rebinding | High-trust or cost-sensitive projects |

**Why `FRONTIER_FAILOVER` by default:** `STRICT` converts every rate limit into a project stall, which defeats the long-horizon purpose. `ASK` requires a human at unpredictable moments — also fatal to unattended operation. `FRONTIER_FAILOVER` preserves autonomy while the floor (I-9) prevents the failover from degrading quality.

Defaults: RUNTIME-A1 `FRONTIER_FAILOVER`; RUNTIME-A2 `FRONTIER_FAILOVER`; critical projects `ASK`.

## Manager failover sequence

```
executor unavailable
   → release binding (reason recorded)
   → role remains ACTIVE; its work is untouched
   → router selects a new eligible executor at the role's floor
   → bind
   → MANDATORY context rehydration          ← not optional
   → resume from persisted next actions
```

Rehydration is mandatory because the new executor knows nothing. Handing it the previous executor's summary would propagate that executor's misreadings — the failure mode `CONTEXT_REHYDRATION_ARCHITECTURE.md` exists to prevent.

## A3 failover (§54)

The scenario: A3 on model X implemented; A4 rejected; X is now rate-limited.

**The work must not be lost — and it is not, because none of it lived in X's session.** Already durable: the original Task Capsule, worktree, branch, current SHA, diff, executed checks, A4 findings, dependencies, and context references.

```
A2 requests REPAIR
  → quality_floor = FRONTIER (never lower than the parent)
  → router selects another currently eligible frontier executor
  → fresh A3 receives a Repair Capsule + repository state
  → no conversational inheritance required
```

This is the direct payoff of I-2: because A3 was always ephemeral, provider failover is an ordinary routing decision rather than a recovery operation.

## A1 failover (§57)

Same mechanism. RUNTIME-A1 belongs to orchestrator state, not to a Claude chat or a Codex chat. It may move between models and between hosts without destroying project identity. A single-active-binding lease prevents two authoritative A1 executors — divergent DAG mutations would be unrecoverable.

## Lease and crash safety

Bindings hold a time-bounded lease, renewed while active. A crashed executor's lease expires and the role becomes rebindable. Without expiry, a crash would lock a project permanently.

## What failover never does

Never lowers a quality floor to find an available executor. Never accepts partial work as complete. Never proceeds without rehydration. Never routes to a provider that refused on safety grounds in order to obtain the refused result (I-12).
