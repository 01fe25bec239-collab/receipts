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

# GOAL_ORCHESTRATOR

## Purpose

Turn a developer goal into sustained, measurable progress that continues until the goal is actually satisfied — surviving rate limits, rejections, session loss, and host changes along the way.

## `START_GOAL`

One normalized core operation, available on both hosts (parity row P-01).

```
START_GOAL {
  project_id, goal_text?, spec_refs[],          # e.g. ["SPEC.md","docs/requirements.md"]
  acceptance_criteria[]?, constraints?,          # budgets, deadlines, provider policy
  assurance_profile?, routing_mode?
}
→ { goal_id, status: ACCEPTED|CLARIFICATION_REQUIRED }
```

**Command syntax is Q-01.** The requirement is semantic — a superior long-horizon execution entry point — not the capture of any reserved command name. If a host safely permits an alias, it may be evaluated; if not, a namespaced command is used. Overriding a built-in without verified support would be a fragile, host-hostile choice.

## Ingestion

The original spec is persisted **verbatim** as the reference artifact. Every later goal evaluation measures against it, not against a summary — otherwise the target drifts toward whatever the system found convenient to build.

Where acceptance criteria are absent or unmeasurable, A1 proposes derived criteria and marks them `DERIVED` for user confirmation. A criterion nobody can evaluate cannot close a goal.

## Decomposition

```
spec → candidate workstreams → coherence check → RUNTIME-A2 roles
                                     │
                                     ├── too granular?  merge
                                     ├── no code ownership? not a workstream
                                     └── no stable interface? re-cut the boundary
```

Decomposition is a **frontier-floor** operation. It sets the shape of everything downstream, and a bad cut is expensive to undo after workstreams have branches and history.

## Continuous loop

```
while goal.status == IN_PROGRESS:
    rehydrate_if_triggered()
    ready = dag.ready_tasks()
    for task in scheduler.admit(ready):       # concurrency, budget, collision checks
        decision = router.route(task)         # capability-first, freshness-gated
        dispatch(task, decision)              # A3 → automatic A4 → repair loop
    process_completions()                     # acceptance, integration
    if wave_complete():
        evaluation = global_goal_evaluator.evaluate(goal)
        if evaluation.state == COMPLETE: break
        if evaluation.state in (BLOCKED, HUMAN_REQUIRED): surface(); break
        dag.apply(evaluation.new_tasks)       # INCOMPLETE → continue
```

The loop terminates only on COMPLETE, BLOCKED, HUMAN_REQUIRED, or budget exhaustion. It does not terminate because the task list happens to be empty — that is the difference between "ran out of tasks" and "achieved the goal".

## Progress vs completion

| Signal | Means |
|---|---|
| Tasks accepted | work happened |
| Workstreams integrated | work merged |
| **Goal evaluation COMPLETE** | **the goal is met** |

Only the third closes a goal (I-14).

## Interruption and resume

A goal is durable. It survives host close, provider outage, machine restart, and host switch. On resume: rehydrate, reconcile epoch, re-derive ready tasks, recover any orphaned attempts (`WORKSPACE_RECOVERY.md`), and continue.

## Budgets

Per goal: max cost, max wall-clock, max concurrent workers, max repairs. Exhaustion is a first-class `BLOCKED` outcome with a stated reason and a resume path — never a silent stall, which would look identical to a hang.

## Failure modes handled by design

| Failure | Response |
|---|---|
| Provider rate-limited | Router failover; task preserved |
| A4 rejects | Bounded repair loop |
| Executor session lost | Rebind, rehydrate, continue |
| Host closed | State persists; resume on either host |
| Attempt crashed mid-work | Workspace recovery; partial work never auto-accepted |
| Spec ambiguity | `CLARIFICATION_REQUIRED` / `HUMAN_REQUIRED` |
| Safety interruption | `SAFETY_INTERRUPTION_PROTOCOL.md` |
