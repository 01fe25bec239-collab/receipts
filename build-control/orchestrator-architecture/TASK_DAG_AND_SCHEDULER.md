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

# TASK_DAG_AND_SCHEDULER

> **V1.3 STATUS: SUPERSEDED by `ExecutionGraph`.**
>
> `TaskDag` survives as a **reduced read-only compatibility view** over the precedence subgraph, exposing exactly three queries: ready tasks, task state, and dependency status. It no longer owns node identity, mutation, scheduling semantics, or persistence — those belong to `EXECUTION_GRAPH_MODEL.md`, `GRAPH_MUTATION_PROTOCOL.md` and `GRAPH_EXECUTION_POLICIES.md`.
>
> It remains an **M0 behavioural interface** at that narrowed responsibility, so existing consumers do not break. The scheduling semantics below — edge kinds, admission conditions, cycle detection — are **carried forward unchanged** into the graph's precedence subgraph. Read this document for those semantics; read the graph documents for everything else.

## Ownership

**RUNTIME-A1 owns the global DAG.** A RUNTIME-A2 decomposes its own workstream into tasks but may not silently violate the global graph: cross-workstream edges are proposed to A1, never asserted locally. Two managers editing one graph produces a graph neither of them understands.

## Structure

```
Goal
 └── Workstream (RUNTIME-A2)
      └── Task (unit of RUNTIME-A3 dispatch)
           └── Attempt (1, R1, R2 … each with its own SHA and review)
```

```
SPEC
 │
 ▼
AUTH-CONTRACT ──────────┐
 │                      │
 ▼                      ▼
AUTH-API             AUTH-UI
 └──────────┬───────────┘
            ▼
        AUTH-AUDIT
            ▼
       INTEGRATION
```

## Edges

Every dependency is an **exact task ID**. Phrasings such as "after the core work" are not admissible — an unresolvable dependency cannot be scheduled or verified.

| Edge type | Meaning |
|---|---|
| `REQUIRES_ACCEPTED` | Dependency must be `ACCEPTED` before dispatch (default) |
| `REQUIRES_INTEGRATED` | Must be merged into the workstream branch |
| `REQUIRES_INTERFACE` | Only the interface must be frozen; implementations may proceed in parallel |

`REQUIRES_INTERFACE` is what allows `AUTH-API` and `AUTH-UI` to run concurrently once the contract exists — the main source of real parallelism.

## Cycle detection

Runs on every mutation, before persistence. A mutation introducing a cycle is **rejected**, not repaired: the proposer is told which edge closed the loop. Cycles are almost always a decomposition error, and silently breaking one hides the error.

## Task states

`PLANNED → READY → DISPATCHED → IN_PROGRESS → AWAITING_REVIEW → {REVIEW_PASSED | REVIEW_REJECTED} → REPAIRING → ACCEPTED → INTEGRATED`
plus `BLOCKED`, `CANCELLED`, `HUMAN_REQUIRED`.

Transitions are explicit and validated. Free-text state is never authoritative. Forbidden shortcuts: `IN_PROGRESS → ACCEPTED` (skips audit), `AWAITING_REVIEW → ACCEPTED` (skips verdict), `REVIEW_REJECTED → REVIEW_PASSED` (a verdict is not revised by re-reading it — a repair produces a *new* review of a *new* SHA).

## Scheduler admission

A task is admitted only when **all** hold:

1. every dependency satisfied per its edge type;
2. global, per-workstream, and per-provider concurrency limits permit;
3. write-set does not collide with any running task (`CONCURRENCY_MODEL.md`);
4. budget remains (cost, time, tokens);
5. a routing decision is obtainable at the required quality floor;
6. workspace can be provisioned;
7. context epoch is current.

Failing 5 does not fail the task — it **waits**, per `QUALITY_COST_POLICY.md`. Downgrading to whatever is free would violate I-9.

## Priority

Critical-path length, then blocking-count, then age, then user priority. Deliberately simple: elaborate priority schemes are hard to debug and rarely beat critical-path ordering.

## Mutation

Sources: initial decomposition; approved `SUBTASK_REQUEST`; goal-evaluator gaps; A2 escalation resolution; user amendment.

Every mutation is an event: who, why, edges added/removed, cycle check result, resulting epoch. The DAG's history is as auditable as the code's.
