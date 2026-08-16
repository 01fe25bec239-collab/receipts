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

# EVENT_MODEL

## Purpose (§83)

Record enough to **reconstruct orchestration decisions** after the fact. Not telemetry; an audit trail.

## Envelope

```
Event { event_id (ULID), project_id, goal_id?, event_type,
        actor { kind: SYSTEM|ROLE|HOST|USER|PROVIDER, id? },
        subject { kind: TASK|ROLE|WORKSPACE|REVIEW|PROVIDER|GOAL, id },
        occurred_at, payload, correlation_id, epoch }
```

`correlation_id` threads an entire task lineage — dispatch, attempt, review, rejection, repair, acceptance, integration — into one readable story.

## Event types

**Goal:** `GOAL_CREATED` · `GOAL_DECOMPOSED` · `GOAL_EVALUATED` · `GOAL_COMPLETED` · `GOAL_BLOCKED`
**Roles:** `ROLE_CREATED` · `EXECUTOR_SELECTED` · `EXECUTOR_BOUND` · `EXECUTOR_RELEASED` · `EXECUTOR_REPLACED`
**Routing:** `ROUTING_REQUESTED` · `ROUTING_DECIDED` (full decision record) · `ROUTING_FAILED_NO_CANDIDATE` · `USER_ROUTING_INPUT`
**Tasks:** `TASK_CREATED` · `TASK_READY` · `TASK_DISPATCHED` · `TASK_STARTED` · `TASK_COMPLETED` · `TASK_FAILED` · `TASK_CANCELLED` · `SUBTASK_REQUESTED` · `SUBTASK_DISPOSITIONED`
**Workspace:** `WORKSPACE_CREATED` · `CHECKPOINT_WRITTEN` · `WORKSPACE_RECOVERED` · `WORKSPACE_REMOVED`
**Review:** `REVIEW_DISPATCHED` · `REVIEW_PASSED` · `REVIEW_REJECTED` · `FINDING_RAISED` · `FINDING_DISPOSITIONED` · `REPAIR_ISSUED` · `REPAIR_LIMIT_REACHED`
**Provider:** `RATE_LIMIT_OBSERVED` · `PROVIDER_DEGRADED` · `PROVIDER_RECOVERED` · `AUTH_REQUIRED` · `SAFETY_CHECK_PENDING` · `POLICY_BLOCKED` · `MODEL_DISCOVERED` · `MODEL_LIFECYCLE_CHANGED` · `REGISTRY_REFRESHED`
**Context:** `CONTEXT_REHYDRATED` · `CONTEXT_EPOCH_ADVANCED` · `CONTEXT_COMPACTED`
**Integration:** `ACCEPTANCE_EVALUATED` · `INTEGRATION_ACCEPTED` · `INTEGRATION_REJECTED` · `INTEGRATION_BLOCKED`
**Escalation:** `ESCALATED_TO_A2` · `ESCALATED_TO_A1` · `HUMAN_REQUIRED`

## Rules

1. **Append-only.** No event is edited or deleted.
2. **Persist before acting** where feasible, so a crash mid-handling is recoverable.
3. **Decisions carry their reasoning**, not just their outcome — `ROUTING_DECIDED` embeds the full decision record.
4. **No secrets, ever.** All payloads pass a redaction layer; credentials, tokens, and auth values never reach the log.
5. **Large payloads by reference.** Diffs and outputs are stored as references with digests, not inlined.

## Reconstruction

The log answers, after the fact: why was this model chosen, what did it produce, at what SHA, who reviewed it, what did they find, why was it repaired, what changed, why was it accepted, and what did the goal evaluator conclude.

If a question of that shape cannot be answered from the log, the log is missing an event — and that is a defect, because the answer will be needed exactly when something went wrong.

## Retention

Full retention by default; a local orchestration log is small relative to a repository. Pruning, if configured, never removes events referenced by a provenance chain that still supports an integrated result.
