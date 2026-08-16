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

# RUNTIME_ROLE_MODEL

## Namespace warning

This document describes **RUNTIME** roles — those the finished product creates inside the *user's* project. For the roles building this repository, see `BUILD_A2_DECOMPOSITION.md`.

## Hierarchy

```
RUNTIME-A1  (durable)     global orchestrator
    │
    ├── RUNTIME-A2  (durable)     engineering workstream manager
    │        │
    │        ├── RUNTIME-A3  (ephemeral)   one implementation attempt
    │        │        └── RUNTIME-A4 (ephemeral)  independent audit of that attempt
    │        └── RUNTIME-A3 …
    └── RUNTIME-A2 …
```

## The durable/ephemeral split

This is the architecture's central structural decision.

| | Durable | Ephemeral |
|---|---|---|
| Roles | A1, A2 | A3, A4 |
| Identity | persisted, survives everything | none beyond one attempt |
| Executor | bound and rebound over time | one session, then destroyed |
| Context | rehydrated from authoritative sources | delivered in a capsule |
| On failure | rebind executor, continue | discard, dispatch fresh |

**Why managers are durable:** a workstream outlives any session. Ownership, decisions, dependencies, and review history are the accumulated judgement of the project; losing them to a rate limit would be losing the project.

**Why workers are ephemeral:** a long-lived implementer accumulates context that is expensive, stale, and unreviewable. A fresh session per attempt guarantees the work is driven by the capsule and the repository — which is also what makes provider failover trivial (`SESSION_FAILOVER_ARCHITECTURE.md`). Deliberate disposability is a feature, not a limitation.

## Role vs executor

```
LOGICAL ROLE                    EXECUTOR BINDING
────────────                    ────────────────
RUNTIME-A2-AUTH        ←→       binding#1  provider P, model M, session S1  [RELEASED: rate limit]
(id, ownership, branch,         binding#2  provider Q, model N, session S2  [ACTIVE]
 decisions, DAG slice,
 history)
```

The role is a row in the state store. The binding is a temporary association. `RUNTIME-A2-AUTH` remains the same manager across both bindings — same ownership, same branch, same history.

## Authority ladder

| Role | May | May not |
|---|---|---|
| RUNTIME-A1 | mutate global DAG, create A2s, resolve cross-workstream deps, integrate to main, set budgets, declare goal state | implement code directly; bypass A2 for convenience |
| RUNTIME-A2 | decompose its workstream, issue capsules, accept A3 results, own its integration branch, escalate | mutate the global DAG unilaterally; write outside its workstream; skip required audit |
| RUNTIME-A3 | implement one task in its workspace, run checks, commit, hand off | spawn agents; broaden scope; self-approve; merge; write outside allowed paths |
| RUNTIME-A4 | read the exact SHA, audit, return a structured verdict | modify anything; negotiate its verdict; review its own implementation |

**A1 does not become a coding agent.** If implementation is needed, it flows A1 → A2 → A3. A small change is still a change, and the moment A1 implements directly, the provenance chain for that change does not exist.

## Escalation path

```
RUNTIME-A3 blocked   → SUBTASK_REQUEST or BLOCKED handoff → RUNTIME-A2
RUNTIME-A2 blocked   → escalation → RUNTIME-A1
RUNTIME-A1 blocked   → HUMAN_REQUIRED
```

Every level may escalate; none may improvise past a blocker.

## Role count discipline

RUNTIME-A2 managers correspond to **substantial workstreams** (auth, payments, data layer), not to individual tasks or file categories. There is no documentation RUNTIME-A2 (§16): docs correctness belongs to the engineering A2 owning the subsystem, and docs work is an economy RUNTIME-A3 task.

Creating one A2 per task would collapse the distinction between managers and workers and reintroduce the per-session-state problem the durable/ephemeral split exists to solve.

## Policy-dependent role instantiation (V1.3)

```
FREE : GraphCoordinator + single-runtime execution policy
PRO  : RUNTIME-A1 → RUNTIME-A2 → RUNTIME-A3 → RUNTIME-A4
```

`GraphCoordinator` is a **simplified single-runtime coordinator**, explicitly not a distributed RUNTIME-A1: it holds no cross-workstream authority, creates no durable RUNTIME-A2 managers, and performs no routing. It compiles, schedules and executes the graph against one eligible runtime.

**Graph state is not duplicated between tiers.** Both operate on the same `ExecutionGraph`; only role instantiation and dispatch differ. On upgrade, PRO roles take over the existing graph — they do not rebuild it.
