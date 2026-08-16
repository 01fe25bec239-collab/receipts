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

# BUILD-A2-REVIEW-INTEGRATION

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-REVIEW-INTEGRATION` — Review, Assurance & Integration.

## Mission
Implement the product's headline loop and its strongest gate: automatic A3→A4 dispatch, the review protocol, the bounded repair controller, assurance profiles, provenance and code-state binding, the security review pipeline, safety-interruption handling, and the acceptance/integration gates.

## Why long-lived
This manager owns the meaning of "accepted". Every quality property the product claims is enforced here, and the definitions of blocking findings, exact-SHA binding, and gate conditions must stay stable while everything around them changes.

## Owned subsystem
A3→A4 controller (automatic dispatch) · review protocol and verdict schema · repair controller with bounds and escalation · assurance profiles · provenance records and staleness · security review pipeline · safety-interruption state machine · A2 acceptance gate · A1 integration gate · calibration feedback emission.

## Owned repository paths
`src/assurance/**` · `src/review/**` · `src/integration/**` · `src/security/**` · owned schemas · **`docs/review-integration/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`A3Handoff` · `A4Review` · `AssuranceProfile` · `IntegrationDecision` · `IntegrationRequest` · `Provenance` · `RepairRequest` · `ReviewCapsule` · `ReviewRequest` · `SafetyInterruption`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `TaskCapsule` | `BUILD-A2-ORCHESTRATION` |
| `RepairCapsule` | `BUILD-A2-ORCHESTRATION` |
| `GraphNode` | `BUILD-A2-ORCHESTRATION` |
| `GraphNodeResult` | `BUILD-A2-ORCHESTRATION` |
| `RoutingRequest` | `BUILD-A2-MODEL-ROUTING` |
| `RoutingDecision` | `BUILD-A2-MODEL-ROUTING` |
| `ModelObservation` | `BUILD-A2-MODEL-ROUTING` |
| `WorkspaceHandle` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `WorkspaceCheckpoint` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `RuntimeAdapter` | `BUILD-A2-RUNTIME-ADAPTERS` |
| `ContextEpoch` | `BUILD-A2-STATE-CONTEXT` |
| `DispatchAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |


## Reference-only
`NORMALIZED_HOST_EVENT`, `MODEL_CAPABILITY`

## Forbidden ownership
DAG semantics · routing internals · adapter implementations · workspace/git internals · state internals · host adapters.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.
- `BUILD-A2-WORKSPACE-EXECUTION` — **concrete implementation required.** Needs real worktrees and SHAs; evidence bound to a fake workspace is not evidence.
- `BUILD-A2-RUNTIME-ADAPTERS` — **concrete implementation required.** Needs real capability probing and execution; a stub cannot report a runtime's true flags.

**Build wave: W3** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `TaskCapsule` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `RepairCapsule` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `GraphNode` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `GraphNodeResult` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `RoutingRequest` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `RoutingDecision` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `ModelObservation` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `WorkspaceHandle` — owned by `BUILD-A2-WORKSPACE-EXECUTION`; frozen at M0.
- `WorkspaceCheckpoint` — owned by `BUILD-A2-WORKSPACE-EXECUTION`; frozen at M0.
- `RuntimeAdapter` — owned by `BUILD-A2-RUNTIME-ADAPTERS`; frozen at M0.
- `ContextEpoch` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `DispatchAdmissionDecision` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-ORCHESTRATION` — A4Review verdict, task state transition
- ↔ `BUILD-A2-MODEL-ROUTING` — RoutingRequest for reviewer/repair executor
- ↔ `BUILD-A2-MODEL-ROUTING` — ModelObservation calibration feedback
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories

Receives a `ReviewRequest` from ORCHESTRATION, constructs the `ReviewCapsule` itself, dispatches a fresh A4, returns an `A4Review`. Owning both the request and the capsule keeps review construction entirely inside this manager.
Automatic A4 dispatch on A3 completion · review capsule construction (excluding A3 conversational history) · verdict schema and validation · independence enforcement (fresh session; `distinct_provider` policy) · reviewer reproduction of acceptance checks · repair controller with bounds and escalation ladder · attempt/revision identity and history preservation · assurance profile engine · provenance record + SHA identity chain · staleness detection · A2 acceptance gate (nine checks) · A1 integration gate (ten conditions) · security review pipeline orchestration and finding normalisation · safety-interruption state machine and the narrowed-retry test.

## Expected BUILD-A4 review categories
`review_sha == implementation_sha` enforced · **no commit added after review** check present and unbypassable · self-review structurally impossible · blocking-finding floor cannot be configured away · repair bound enforced with escalation · rejected SHAs never deleted · **no provider-shopping path exists after `POLICY_BLOCKED`** · security failure never yields `PASS` · gate never implements its own fixes.

## Frontier / economy policy
Frontier throughout — this manager writes the code that decides quality. Economy only for user-facing documentation of the loop after semantics are frozen.

## Security responsibility
**Owns the security requirements other managers implement.** Defines the security acceptance tests that RUNTIME-ADAPTERS, WORKSPACE-EXECUTION, and STATE-CONTEXT must pass. May block acceptance; may never implement around another manager.

Also owns the safety-bypass prohibition (I-12) in code, not just in prose.

## Integration responsibility
Owns both gates. Owns the provenance chain that makes an integration decision reconstructible (I-19).

## Context requirements
Initial: repair loop, A3/A4 lifecycles, assurance profiles, provenance, integration gate, security review, safety interruption, security trust model. Rehydration: on any assurance or security semantics change, before every gate implementation task.

## Non-goals
Does not implement code under review · does not route (it requests) · does not create workspaces · does not decide goal completion (ORCHESTRATION's evaluator does, using this manager's outputs).

## First proposed milestone
`M-REVIEW-1`: automatic A3→A4 dispatch, review capsule, verdict schema, exact-SHA binding, and the A2 acceptance gate with all nine checks.
