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

# GRAPH_PROVENANCE_MODEL

## Two questions, two structures

| Question | Structure |
|---|---|
| What *should* execute, and why? | `ExecutionGraph` |
| What *actually* executed, against which code state, with what evidence? | Provenance relations |

Keeping them separate is what allows the plan to be revised without rewriting history, and history to be complete without bloating the plan.

## Relations

```
GraphNode ──produces──▶ GraphNodeResult ──binds──▶ CommitSHA
    │                          │
    │                          ├──▶ CheckRun (command, exit code, output ref)
    │                          ├──▶ Review ──▶ Finding
    │                          └──▶ RoutingDecision ──▶ Runtime + Model
    │
    ├──runs in──▶ Workspace (branch, worktree)
    └──feeds──▶ Integration ──▶ GoalEvaluation
```

## Exact code-state binding — unchanged from V1.2.3

Every result carries `code_sha`. A review of `abc` never validates `xyz`. The acceptance gate still verifies `review_sha == implementation_sha` and that no commit followed the review. Promoting the graph did not weaken this; it gave it a home.

## Reframed threat model, retained

The problem is **not** that agents lie. It is that multiple capable agents operate across sessions, providers, workspaces and code states, and integration needs durable machine-readable knowledge of what happened. That framing is carried forward from V1.2.3 unchanged.

## FREE provenance

FREE gets real provenance, not a stub: node results, exact SHAs, check commands and outcomes, workspace identity, and a readable history. What FREE does not get is advanced provenance — cross-provider attribution, routing-decision archaeology, and assurance-profile evidence chains — because those only exist when distributed orchestration does.

## No graph database

Relational storage in SQLite. A graph abstraction does not require a graph database, and introducing one would add an operational dependency to a local-first tool for no gain at this scale. See `STATE_AND_CHECKPOINT_ARCHITECTURE.md`.
