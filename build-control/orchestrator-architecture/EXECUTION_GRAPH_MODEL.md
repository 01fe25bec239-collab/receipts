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

# EXECUTION_GRAPH_MODEL

## The promotion

V1.2.3 *had* a DAG. V1.3 *is* graph-native. The graph stops being an internal scheduling structure and becomes the authoritative, durable, versioned artifact the user inspects, resumes, and reasons about.

```
USER GOAL / SPEC → GraphCompiler → ExecutionGraph → ExecutionPolicy → GraphScheduler
```

Why it matters commercially as well as technically: the graph is what a FREE user gets, and it is the same object a PRO user's distributed orchestration executes over. One artifact, two policies — which is what makes the upgrade path continuous rather than a migration.

## Entities

| Entity | Purpose | Schema |
|---|---|---|
| `ExecutionGraph` | Versioned plan: nodes, edges, policy, digest | yes |
| `GraphNode` | One unit of planned or executed work | yes |
| `GraphEdge` | Precedence **or** control relation | yes |
| `GraphMutation` | Auditable change producing a new version | yes |
| `GraphSnapshot` | Materialised state for display and resume | yes |
| `GraphNodeResult` | Evidence produced by a node | yes |
| `GraphExecutionPolicy` | FREE or PRO dispatch policy | behavioural |

**`GraphRun` was evaluated and rejected.** A "run" would duplicate what `graph_version` plus node states and `GraphNodeResult` already express, and would create a second identity for the same work. Long-horizon execution is continuous and resumable; there is no natural run boundary to model. Adding it would be a type for aesthetics.

**`GraphVersion` was evaluated and folded in.** Version is an integer on the graph plus a `GraphMutation` record carrying parent, actor, reason and digest. A separate entity would hold nothing the mutation does not.

## Node kinds

`GOAL` · `WORKSTREAM` · `TASK` · `ATTEMPT` · `IMPLEMENTATION` · `REVIEW` · `REPAIR` · `DETERMINISTIC_CHECK` · `ROUTING` · `INTEGRATION` · `HUMAN_GATE` · `GOAL_EVALUATION`

Kinds are an **extensible string**, not an enum — a new kind must not require a schema change. No graph is required to use every kind: a FREE graph typically uses `GOAL`, `TASK` and `DETERMINISTIC_CHECK` only.

## Node state

`PLANNED` → `READY` → `ADMITTED` → `DISPATCHED` → `RUNNING` → `AWAITING_REVIEW` → `PASSED` | `REJECTED` → `REPAIRING` → `ACCEPTED` → `INTEGRATED`, plus `BLOCKED`, `CANCELLED`, `HUMAN_REQUIRED`, and `LOCKED_REQUIRES_PRO`.

`ADMITTED` is new and load-bearing: it is the state a node reaches **after** passing feature admission and policy eligibility, and **before** any dispatch. It is what makes "no Pro dispatch for a FREE user" observable rather than asserted.

`LOCKED_REQUIRES_PRO` is a first-class node state, not a UI badge. A locked node is visible, carries its reason, and is never dispatched.

## Capability requirement per node

Each node carries `required_capabilities[]` — namespaced strings such as `graph.core` or `review.independent_a4`. Empty means FREE-executable.

This is deliberately **on the node**, not hard-coded into the scheduler. Admission asks "what does this node need?" and the entitlement service answers "may this installation do that?". Neither knows about tiers. Adding a Studio or Team tier later changes a catalog, not the engine.

## Relationship to provenance

The graph answers *what should execute and why*. Provenance answers *what actually executed, against which code state, with what evidence*. `GraphNodeResult` is the join: it carries `code_sha`, checks, review id and routing decision id, binding a planned node to the exact code state it ran against.

Exact-SHA binding from V1.2.3 is unchanged and now lives on the graph.
