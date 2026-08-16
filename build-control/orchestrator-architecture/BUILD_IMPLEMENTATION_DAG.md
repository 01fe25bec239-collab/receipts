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

# BUILD_IMPLEMENTATION_DAG

**Scope: HARD build dependencies only.** An edge `A → B` means *A requires the concrete implementation of B before A can be implemented*. This graph **MUST** be acyclic and is validated programmatically.

Contract consumption and runtime interaction are **different relations** and live in `CONTRACT_CONSUMPTION_GRAPH.md` and `RUNTIME_INTERACTION_GRAPH.md`. Conflating them is what produced the false "no cycles" claim in V1.1.

## Why V1.1 was cyclic

V1.1's single matrix mixed all three relations. It recorded `ORCHESTRATION → REVIEW-INTEGRATION` and `REVIEW-INTEGRATION → ORCHESTRATION`, and likewise between `MODEL-ROUTING` and `REVIEW-INTEGRATION` — producing direct 2-cycles and the 3-cycle `REVIEW-INTEGRATION → ORCHESTRATION → MODEL-ROUTING → REVIEW-INTEGRATION`. The matrix nonetheless asserted "No cycles." **That assertion was false.**

Those reciprocal arrows were real *runtime* collaborations and *contract* consumptions, not concrete build dependencies. Once separated, no hard cycle exists.

## Cycle-breaking mechanism: dependency inversion through frozen contracts

```
ORCHESTRATION ──emits ReviewRequest──▶ REVIEW-INTEGRATION
                                            │ constructs ReviewCapsule (its own artifact)
                                            │ dispatches a fresh ephemeral RUNTIME-A4
              ◀──returns A4Review───────────┘
```

`ReviewRequest` is a **normative contract owned by `BUILD-A2-REVIEW-INTEGRATION`** with a schema in `schemas/ReviewRequest.schema.json`, frozen at M0. ORCHESTRATION emits it; REVIEW-INTEGRATION accepts it, builds the `ReviewCapsule` itself, and returns an `A4Review`.

The capsule is deliberately **not** the boundary: it is the reviewer-facing artifact, assembled by its owner, so ORCHESTRATION never needs to know how a review is constructed. Neither module imports the other. Both depend only on schemas frozen by BUILD-A1 at **M0**. Likewise:

```
REVIEW-INTEGRATION ──RoutingRequest──▶ MODEL-ROUTING
                   ◀──RoutingDecision──
                   ──ModelObservation──▶  (calibration feedback)
```

The invariant, now normative:

> **Runtime collaboration may be bidirectional. Concrete BUILD implementation dependencies must form a DAG.**

## Nodes and hard edges

| Node | Hard build dependencies | Wave |
|---|---|---|
| `BUILD-A2-STATE-CONTEXT` | — | W1 |
| `BUILD-A2-WORKSPACE-EXECUTION` | `BUILD-A2-STATE-CONTEXT` | W2 |
| `BUILD-A2-RUNTIME-ADAPTERS` | `BUILD-A2-STATE-CONTEXT` | W2 |
| `BUILD-A2-ORCHESTRATION` | `BUILD-A2-STATE-CONTEXT` | W2 |
| `BUILD-A2-MODEL-ROUTING` | `BUILD-A2-STATE-CONTEXT`, `BUILD-A2-RUNTIME-ADAPTERS` | W3 |
| `BUILD-A2-REVIEW-INTEGRATION` | `BUILD-A2-STATE-CONTEXT`, `BUILD-A2-WORKSPACE-EXECUTION`, `BUILD-A2-RUNTIME-ADAPTERS` | W3 |
| `BUILD-A2-HOST-INTEGRATION` | `BUILD-A2-STATE-CONTEXT`, `BUILD-A2-ORCHESTRATION` | W3 |

**Derived:** nodes = 7 · edges = 10 · cycles = **0**

## Topological order

```
STATE-CONTEXT → WORKSPACE-EXECUTION → RUNTIME-ADAPTERS → ORCHESTRATION → MODEL-ROUTING → REVIEW-INTEGRATION → HOST-INTEGRATION
```

## Waves

### W1

**`BUILD-A2-STATE-CONTEXT`**

*Hard build dependencies*

- `NO_DEPENDENCY` — DAG source.

*M0-frozen external contract dependencies*

- **None.**


### W2

**`BUILD-A2-ORCHESTRATION`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `ReviewRequest` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `A4Review` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `IntegrationDecision` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `AssuranceProfile` (behavioural interface), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RoutingRequest` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RoutingDecision` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ProviderPolicyEligibility` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `WorkspaceHandle` (schema), owned by `BUILD-A2-WORKSPACE-EXECUTION` (W2, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `LogicalRole` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ExecutorBinding` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ContextManifest` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ContextEpoch` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ProductEntitlement` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `EntitlementVerifier` (behavioural interface), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ActivationState` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `NormalizedHostEvent` (schema), owned by `BUILD-A2-HOST-INTEGRATION` (W3, later wave).

**`BUILD-A2-RUNTIME-ADAPTERS`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `WorkspaceHandle` (schema), owned by `BUILD-A2-WORKSPACE-EXECUTION` (W2, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `TaskCapsule` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RepairCapsule` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ReviewCapsule` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, later wave).
- `BUILD_A1_FROZEN_CONTRACT` — `DispatchAdmissionDecision` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, same wave).

**`BUILD-A2-WORKSPACE-EXECUTION`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `TaskCapsule` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `GraphNode` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, same wave).


### W3

**`BUILD-A2-HOST-INTEGRATION`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).
- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-ORCHESTRATION` (W2).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `RoutingDecision` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `IntegrationDecision` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `A4Review` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `GoalEvaluation` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `LogicalRole` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `WorkspaceHandle` (schema), owned by `BUILD-A2-WORKSPACE-EXECUTION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ExecutionGraph` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `GraphSnapshot` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `FeatureCapabilitySet` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `FeatureAdmissionDecision` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ProductEntitlement` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ActivationState` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).

**`BUILD-A2-MODEL-ROUTING`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).
- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-RUNTIME-ADAPTERS` (W2).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `RuntimeAdapter` (behavioural interface), owned by `BUILD-A2-RUNTIME-ADAPTERS` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `Provider` (behavioural interface), owned by `BUILD-A2-RUNTIME-ADAPTERS` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `A4Review` (schema), owned by `BUILD-A2-REVIEW-INTEGRATION` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `FeatureAdmissionDecision` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).

**`BUILD-A2-REVIEW-INTEGRATION`**

*Hard build dependencies*

- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-STATE-CONTEXT` (W1).
- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-WORKSPACE-EXECUTION` (W2).
- `PRIOR_CONCRETE_IMPLEMENTATION` — `BUILD-A2-RUNTIME-ADAPTERS` (W2).

*M0-frozen external contract dependencies*

- `BUILD_A1_FROZEN_CONTRACT` — `TaskCapsule` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RepairCapsule` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `GraphNode` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `GraphNodeResult` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RoutingRequest` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RoutingDecision` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ModelObservation` (schema), owned by `BUILD-A2-MODEL-ROUTING` (W3, same wave).
- `BUILD_A1_FROZEN_CONTRACT` — `WorkspaceHandle` (schema), owned by `BUILD-A2-WORKSPACE-EXECUTION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `WorkspaceCheckpoint` (schema), owned by `BUILD-A2-WORKSPACE-EXECUTION` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `RuntimeAdapter` (behavioural interface), owned by `BUILD-A2-RUNTIME-ADAPTERS` (W2, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `ContextEpoch` (schema), owned by `BUILD-A2-STATE-CONTEXT` (W1, earlier wave).
- `BUILD_A1_FROZEN_CONTRACT` — `DispatchAdmissionDecision` (schema), owned by `BUILD-A2-ORCHESTRATION` (W2, earlier wave).


### How to read these two lists

A **hard build dependency** requires concrete implementation and forces wave ordering. An **M0-frozen contract dependency** requires only the interface and forces nothing.

Generated from `CONTRACT_CONSUMPTION_GRAPH.md`. No manager lists a contract it owns.

### V1.3.1 note

`DispatchAdmissionDecision` and `ActivationState` added no hard dependency: the former is owned by ORCHESTRATION and consumed as a frozen contract by RUNTIME-ADAPTERS and REVIEW-INTEGRATION; the latter is owned by STATE-CONTEXT, which depends on nothing. **The DAG remains 7 nodes / 10 edges / 0 cycles** — derived, not preserved.

## Wave-order validation

Every hard dependency of every manager resolves to a strictly earlier wave. Programmatically checked: **wave-order violations = 0**.

Separately validated: for every external contract dependency whose owner sits in the **same or a later wave** than its consumer, that contract is frozen at M0. Result: `UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES = 0`. This is the check that makes the W2/W3 split honest — twenty such dependencies exist, and all are interface-only.

## Note on HOST-INTEGRATION

`HOST-INTEGRATION` sits in W3 by hard dependency (it needs concrete `ORCHESTRATION` core operations). Its **parity conformance suite** is a later *integration checkpoint*, not a build dependency — it gates release, not wave entry. Treating it as a dependency would have forced it to W4 and understated available parallelism.
