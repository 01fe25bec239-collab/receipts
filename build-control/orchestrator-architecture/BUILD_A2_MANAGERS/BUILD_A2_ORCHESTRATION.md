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

# BUILD-A2-ORCHESTRATION

**Namespace:** BUILD-control. This manager implements the orchestrator repository. It is not a RUNTIME-A2.

## Identity
`BUILD-A2-ORCHESTRATION` — Orchestration Core.

## Mission
Implement the subsystem that turns a goal into durable, scheduled, bounded work: the goal orchestrator, the logical-role engine, the global DAG, the scheduler, capsule construction, concurrency control, subtask governance, budgets, and the global goal evaluator.

## Why long-lived
This is the semantic centre of the product. Its concepts (role, task, capsule, DAG, wave, completion) are consumed by every other manager, and their meaning must stay coherent across the whole build. A rotating owner would produce a DAG whose semantics drift.

## Owned subsystem
Goal orchestrator · RUNTIME-A1/A2 logical-role engine · global and workstream DAG · scheduler and admission · Task/Repair capsule construction · concurrency and write-collision analysis · `SUBTASK_REQUEST` governance · budgets · Global Goal Evaluator.

## Owned repository paths
`src/core/orchestration/**` · `src/core/goal/**` · `src/core/dag/**` · `src/core/capsules/**` · `src/core/scheduler/**` · owned schemas · **`docs/orchestration/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`DispatchAdmissionDecision` · `ExecutionGraph` · `FeatureAdmissionDecision` · `FeatureCapabilitySet` · `Goal` · `GoalEvaluation` · `GraphEdge` · `GraphExecutionPolicy` · `GraphMutation` · `GraphNode` · `GraphNodeResult` · `GraphSnapshot` · `RepairCapsule` · `SubtaskRequest` · `TaskCapsule` · `TaskDag`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `ReviewRequest` | `BUILD-A2-REVIEW-INTEGRATION` |
| `A4Review` | `BUILD-A2-REVIEW-INTEGRATION` |
| `IntegrationDecision` | `BUILD-A2-REVIEW-INTEGRATION` |
| `AssuranceProfile` | `BUILD-A2-REVIEW-INTEGRATION` |
| `RoutingRequest` | `BUILD-A2-MODEL-ROUTING` |
| `RoutingDecision` | `BUILD-A2-MODEL-ROUTING` |
| `ProviderPolicyEligibility` | `BUILD-A2-MODEL-ROUTING` |
| `WorkspaceHandle` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `LogicalRole` | `BUILD-A2-STATE-CONTEXT` |
| `ExecutorBinding` | `BUILD-A2-STATE-CONTEXT` |
| `ContextManifest` | `BUILD-A2-STATE-CONTEXT` |
| `ContextEpoch` | `BUILD-A2-STATE-CONTEXT` |
| `ProductEntitlement` | `BUILD-A2-STATE-CONTEXT` |
| `EntitlementVerifier` | `BUILD-A2-STATE-CONTEXT` |
| `ActivationState` | `BUILD-A2-STATE-CONTEXT` |
| `NormalizedHostEvent` | `BUILD-A2-HOST-INTEGRATION` |


## Reference-only
`RUNTIME_ADAPTER`, `NORMALIZED_HOST_EVENT`, `PROVENANCE`

## Forbidden ownership
Adapter code · registry/routing internals · workspace/git internals · review and gate internals · state persistence internals · host adapters · `build-control/**`.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.

**Build wave: W2** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `ReviewRequest` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `A4Review` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `IntegrationDecision` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `AssuranceProfile` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `RoutingRequest` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `RoutingDecision` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `ProviderPolicyEligibility` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `WorkspaceHandle` — owned by `BUILD-A2-WORKSPACE-EXECUTION`; frozen at M0.
- `LogicalRole` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ExecutorBinding` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ContextManifest` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ContextEpoch` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ProductEntitlement` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `EntitlementVerifier` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ActivationState` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `NormalizedHostEvent` — owned by `BUILD-A2-HOST-INTEGRATION`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-REVIEW-INTEGRATION` — emits `ReviewRequest` (contract owned by REVIEW-INTEGRATION, frozen at M0); receives `A4Review`
- ↔ `BUILD-A2-MODEL-ROUTING` — RoutingRequest for implementer/manager
- ↔ `BUILD-A2-WORKSPACE-EXECUTION` — provision workspace
- ↔ `BUILD-A2-HOST-INTEGRATION` — CoreView for rendering
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories
DAG engine + cycle detection · task state machine · scheduler admission (dependency, concurrency, write-collision, budget) · capsule construction and validation · role lifecycle and lease management · subtask request handling · budget enforcement · goal evaluator deterministic layer · goal evaluator semantic layer · anti-loop convergence check.

## Expected BUILD-A4 review categories
Cycle-detection correctness · state-transition legality (no forbidden shortcuts) · write-collision completeness · capsule validation completeness · budget enforcement · evaluator layering (deterministic before semantic) · absence of role/session conflation.

## Frontier / economy policy
Frontier for DAG semantics, scheduler admission, capsule validation, and the goal evaluator. Economy permitted only for subsystem documentation after decisions are frozen.

## Security responsibility
Ensures capsules never carry credentials; enforces that `allowed_write_paths` are always populated and never permissive by default; ensures no capsule can be dispatched with a stale epoch.

## Integration responsibility
Owns the semantics of "a task is ready" and "the goal is complete". Must not encroach on the acceptance/integration gate, which REVIEW-INTEGRATION owns.

## Context requirements
Initial: architecture, its six contracts, state repository API, `PRODUCT_DEFINITION`, invariants. Rehydration: on contract change, on interface freeze, before each build wave.

## Non-goals
Does not route · does not execute · does not review · does not persist (uses the state API) · does not adapt hosts.

## First proposed milestone
`M-ORCH-1`: DAG engine with cycle detection, task state machine, and capsule construction/validation — against the frozen state repository API, with no scheduler yet.
