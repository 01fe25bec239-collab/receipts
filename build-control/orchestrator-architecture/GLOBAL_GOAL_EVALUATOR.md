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

# GLOBAL_GOAL_EVALUATOR

## Why it exists

An orchestrator that stops when its task list empties has confused *finishing the plan* with *achieving the goal*. The plan was written by a model from a spec it may have partially understood. The evaluator re-measures against the **original specification**.

## States

| State | Meaning | Next |
|---|---|---|
| `COMPLETE` | Original acceptance criteria satisfied and verified | Render result; end |
| `INCOMPLETE` | Real gaps remain | Mutate DAG; continue |
| `BLOCKED` | Cannot proceed (dependency, budget, unavailable provider) | Surface with reason and resume path |
| `HUMAN_REQUIRED` | Judgement or authority beyond the system | Surface with exact question |

## Inputs

Original spec (verbatim); acceptance criteria (including `DERIVED` ones); authoritative DAG state; integration state and current integrated SHA; open A4 findings; deterministic check results; security review state; unresolved dependencies; current context epoch.

## Evaluation is layered — deterministic first

```
1. DETERMINISTIC     required workstreams complete? blocking tasks? blocking findings?
                     required checks green at the integrated SHA? epoch reconciled?
        │  any fail → INCOMPLETE / BLOCKED  (no model invoked)
        ▼
2. SEMANTIC          does the integrated result actually satisfy each criterion?
                     FRONTIER_REASONING; reads the spec and the repository
        │
        ▼
3. RECONCILE         disagreement between layers → INCOMPLETE, with the gap named
```

Deterministic checks run first because they are cheap, certain, and catch most incompleteness. The expensive semantic pass runs only on candidates that already survive the mechanical gate.

## Completion conditions

- all required workstreams complete;
- no blocking DAG tasks;
- no unresolved blocking A4 findings;
- required tests/checks pass at the integrated SHA;
- integration SHA is current (no accepted-but-unintegrated work);
- each original acceptance criterion satisfied with evidence;
- required security review complete, or explicitly `HUMAN_REQUIRED`;
- no unresolved critical dependency;
- context epoch reconciled.

## Completion is not a cheap-model opinion (§80)

The evaluator's semantic layer runs at `FRONTIER_REASONING`. An economy renderer may **present** the result; it may never **decide** it (I-13). A cheap model asked "is this done?" will usually say yes, which is precisely why it is excluded from the decision.

## When INCOMPLETE

The evaluator emits **specific gaps**, each with the criterion it fails and the evidence that shows the failure. A1 converts gaps into tasks and continues. "Seems incomplete" is not an output — an unactionable gap cannot be scheduled.

## Anti-loop protection

If successive evaluations produce the same unresolved gap after repair attempts, the evaluator escalates to `HUMAN_REQUIRED` rather than regenerating equivalent tasks forever. Convergence is checked, not assumed.

## Record

Every evaluation is persisted: inputs, layer results, state, gaps, evidence references, evaluator model and routing decision, timestamp, epoch. A completion claim is auditable after the fact, which is the only way it can be trusted later.
