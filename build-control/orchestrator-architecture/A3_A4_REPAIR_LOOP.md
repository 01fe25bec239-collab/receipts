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

# A3_A4_REPAIR_LOOP

The headline capability: implementation is automatically audited, and rejection automatically produces a bounded repair cycle.

## Loop

```
A3 IMPLEMENTS
      ↓
FREEZE IMPLEMENTATION SHA
      ↓
TERMINATE A3                       ← always, before audit
      ↓
AUTOMATIC DISPATCH: fresh A4
      ↓
   verdict?
   ┌────────────────┬─────────────────────────┐
 PASS /            REJECT                   ERROR
 PASS_WITH_NB        │                        │
   │                 ▼                        ▼
   │          structured findings      classify + retry/escalate
   │                 ↓
   │            RUNTIME-A2
   │                 ↓
   │          Repair Capsule (attempt n+1)
   │                 ↓
   │          ROUTER: fresh executor, frontier floor
   │                 ↓
   │          fresh A3 → new SHA
   │                 ↓
   │          fresh A4 ──────────► (loop)
   ▼
A2 acceptance gate → A2 integration branch
```

**Automatic** means no user action between A3 completion and A4 dispatch. That automation is the product; a review step a human must remember to trigger is a review step that gets skipped.

## Bounded (I-7)

| Bound | Default | Rationale |
|---|---|---|
| `max_repair_attempts` | **3** | Two failures usually mean a mechanical gap; three suggests the task or spec is wrong, not the implementer |
| `max_task_wall_clock` | configurable | prevents indefinite stalls |
| `max_task_cost` | configurable | prevents budget drain on one task |
| `max_total_repairs_per_goal` | configurable | prevents project-wide thrash |

Escalation:

```
R1 → R2 → R3 → RUNTIME-A2 escalation → RUNTIME-A1 escalation → HUMAN_REQUIRED
```

At A2 escalation the task specification is re-examined before another attempt is authorised. Repeated failure is treated as a **signal about the specification**, not only about the implementer — the alternative leaves a bad task in the DAG forever, burning frontier compute against an impossible criterion.

## Identity and history

The parent task keeps its ID; attempts are numbered.

```
TASK-AUTH-004        parent
TASK-AUTH-004-R1     attempt 2   parent_task: TASK-AUTH-004
TASK-AUTH-004-R2     attempt 3   parent_task: TASK-AUTH-004-R1
```

**Every rejected SHA and every review is preserved permanently.** Nothing is squashed away. The chain of rejected attempt → findings → repair → pass is the evidence that acceptance was earned.

## Who repairs

Always a **fresh A3 session**. Which executor depends on routing:

| Situation | Executor selection |
|---|---|
| Mechanical findings, original provider available | Same capability tier; same provider acceptable |
| Original provider rate-limited | Another eligible frontier executor (§54) |
| Findings indicate misunderstanding of contract/architecture | Prefer a *different* model — one that misread once tends to misread the same way twice |
| Security-boundary finding | Frontier floor mandatory; `distinct_provider` escalated |

## Who reviews the repair

Always a fresh A4. For a repair of a **security, contract, or invariant** finding, the reviewer must differ from the A4 that raised it, so the finder does not grade its own diagnosis.

## A3 may not argue past a REJECT

An A3 may record disagreement with reasoning in its handoff. That is all. It may not resubmit the same SHA with an explanation, request a different reviewer, or proceed as if passed. A disputed finding goes to A2, which may escalate to A1; if A1 annuls the finding, that disposition is recorded — resolved through the record, not around it.

## PASS_WITH_NONBLOCKING_FINDINGS

Accepted, and every finding explicitly dispositioned: fix now (new bounded task), defer (with an issue ID and a revisit point), or reject (with a reason). A silently dropped nonblocking finding becomes an untracked defect.

## Cost awareness

Every loop iteration costs an implementation plus an audit. That cost is exactly what `EXPECTED_COST_TO_ACCEPTED_RESULT.md` estimates — a cheap model with a high rejection rate is more expensive than a frontier model that passes first time, and the router is required to account for it.
