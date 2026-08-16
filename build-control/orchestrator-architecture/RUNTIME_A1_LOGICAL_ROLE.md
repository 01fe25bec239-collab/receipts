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

# RUNTIME_A1_LOGICAL_ROLE

**RUNTIME-A1 is a durable logical role, not a chat session.** It belongs to orchestrator state and survives model, session, provider, and host replacement.

## Responsibilities

| # | Responsibility |
|---|---|
| 1 | Ingest the original goal/spec and persist it verbatim as the reference artifact |
| 2 | Inspect repository state |
| 3 | Identify major engineering workstreams |
| 4 | Construct and own the global task DAG |
| 5 | Create RUNTIME-A2 logical roles and assign ownership |
| 6 | Resolve cross-workstream dependencies |
| 7 | Control global scheduling and concurrency budgets |
| 8 | Request executor selection for A2 roles (via the router — never by preference) |
| 9 | Own integration to `main` |
| 10 | Track global progress |
| 11 | Reconcile A2 escalations |
| 12 | Enforce project-wide cost/time/token budgets |
| 13 | Rehydrate context on every mandatory trigger |
| 14 | Run the Global Goal Evaluator and decide COMPLETE / CONTINUE / BLOCKED / HUMAN_REQUIRED |

## Persisted identity

```
logical_role {
  role_id, project_id, role_type: 'RUNTIME_A1',
  created_at, status: ACTIVE|SUSPENDED|RETIRED,
  owned_artifacts: [global DAG, integration branch, budgets],
  context_manifest_id, current_context_epoch,
  active_binding_id?, binding_history[]
}
```

Nothing in this record depends on a conversation.

## Executor binding

A1 holds an executor only while it has work. Binding is on demand: bind → rehydrate → decide → act → release. Between decisions, A1 is a database row and costs nothing.

This matters for economics as much as resilience: a permanently resident frontier manager session would burn budget doing nothing.

## What A1 must not do

- **Implement code.** Route through A2 → A3. No exception for small changes.
- **Select an executor from memory.** Every material dispatch consults Model Intelligence (I-4).
- **Declare completion from a summary.** Completion requires the Global Goal Evaluator against the original spec (I-14).
- **Bypass the integration gate.**
- **Hold more than one active binding.** Enforced by lease.

## Failover

Default policy `FRONTIER_FAILOVER` (`SESSION_FAILOVER_ARCHITECTURE.md`): on executor loss, rebind to another eligible frontier executor, rehydrate mandatorily, and continue. Identity, DAG, decisions, and history are untouched — only the binding changed.

## Rehydration

Mandatory at: initialization, executor replacement, provider replacement, host switch, context compaction, architecture/contract change, new wave, configurable task threshold, serious A4 rejection, before integration, and before declaring COMPLETE.

Rehydration means **rereading authoritative artifacts**, not replaying a previous executor's summary (§62). A summary is a lossy compression made by a model that may itself have misread something; inheriting it inherits the error.

## Budget authority

A1 enforces max concurrent workers, max cost per goal, max repair attempts per task, and max wall-clock per goal. Budget exhaustion is a first-class outcome — `BLOCKED` with a stated reason — not a silent stall.
