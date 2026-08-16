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

# SUBTASK_REQUEST_PROTOCOL

## The prohibition

**RUNTIME-A3 may not spawn agents.** No `A3 → A3 → A3`. Recursive spawning produces an unbounded tree with no owner, no budget, no write-collision analysis, and no audit path — the "uncontrolled swarm" listed in `NON_GOALS.md`.

## The alternative

When an A3 discovers additional required work, it emits a structured request and **stops**.

```
SubtaskRequest {
  request_id, origin_task_id, origin_attempt_id
  discovered_during  string
  requested_work     string              specific and bounded
  justification      string              why the current task cannot complete without it
  blocking           boolean             does the origin task actually depend on it?
  proposed_scope     { allowed_write_paths[], estimated_quality_floor }
  evidence           Ref[]               what in the repo demonstrates the need
}
```

`blocking` is the important field. Non-blocking discoveries are a backlog entry, not an interruption. Treating every discovery as blocking is how a scoped task becomes an open-ended one.

## Disposition

```
A3 → SubtaskRequest → RUNTIME-A2
                        ├── inside this workstream → A2 creates the task
                        ├── another workstream     → escalate to RUNTIME-A1
                        ├── needs re-decomposition → escalate to RUNTIME-A1
                        └── out of goal scope      → reject, with reason recorded
```

Only A2 (within its workstream) or A1 (globally) mutate the authoritative DAG.

## Outcomes for the origin task

| Disposition | Origin task |
|---|---|
| Approved, blocking | `BLOCKED` on the new task; re-dispatched when it accepts |
| Approved, non-blocking | Continues; new task scheduled independently |
| Rejected | Continues within original scope, or reports `BLOCKED` if genuinely impossible |
| Re-decomposition | Origin may be cancelled and replaced; its branch is preserved |

## Anti-patterns rejected

| Anti-pattern | Why rejected |
|---|---|
| A3 silently expands scope | Undisclosed scope creep is a blocking A4 finding |
| A3 spawns a helper | No owner, no budget, no audit |
| A3 edits another workstream | Boundary violation regardless of correctness |
| A2 approves a cross-workstream subtask locally | Only A1 owns cross-workstream edges |
| Request without justification | Rejected on sight; "it would be easier" is not a justification |

## Rate limiting

Repeated subtask requests from one task are a **decomposition signal**: after a configurable count, A2 re-examines the parent task rather than continuing to bolt on children. A task that keeps discovering prerequisites was cut at the wrong boundary.
