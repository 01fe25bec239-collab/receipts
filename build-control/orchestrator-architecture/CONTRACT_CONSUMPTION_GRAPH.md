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

# CONTRACT_CONSUMPTION_GRAPH

**Scope: interface and schema ownership and consumption.** Reciprocal consumption is permitted when the interface is frozen at M0 — a contract dependency is not a build dependency.

## Ownership — exactly one owner per contract

| Contract / interface | Owner |
|---|---|
| `A3Handoff` | `BUILD-A2-REVIEW-INTEGRATION` |
| `A4Review` | `BUILD-A2-REVIEW-INTEGRATION` |
| `ActivationState` | `BUILD-A2-STATE-CONTEXT` |
| `AssuranceProfile` | `BUILD-A2-REVIEW-INTEGRATION` |
| `AvailabilityState` | `BUILD-A2-MODEL-ROUTING` |
| `ContextEpoch` | `BUILD-A2-STATE-CONTEXT` |
| `ContextManifest` | `BUILD-A2-STATE-CONTEXT` |
| `DispatchAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |
| `EntitlementVerifier` | `BUILD-A2-STATE-CONTEXT` |
| `ExecutionGraph` | `BUILD-A2-ORCHESTRATION` |
| `ExecutorBinding` | `BUILD-A2-STATE-CONTEXT` |
| `FeatureAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |
| `FeatureCapabilitySet` | `BUILD-A2-ORCHESTRATION` |
| `Goal` | `BUILD-A2-ORCHESTRATION` |
| `GoalEvaluation` | `BUILD-A2-ORCHESTRATION` |
| `GraphEdge` | `BUILD-A2-ORCHESTRATION` |
| `GraphExecutionPolicy` | `BUILD-A2-ORCHESTRATION` |
| `GraphMutation` | `BUILD-A2-ORCHESTRATION` |
| `GraphNode` | `BUILD-A2-ORCHESTRATION` |
| `GraphNodeResult` | `BUILD-A2-ORCHESTRATION` |
| `GraphSnapshot` | `BUILD-A2-ORCHESTRATION` |
| `HostAdapter` | `BUILD-A2-HOST-INTEGRATION` |
| `HostCapabilityReport` | `BUILD-A2-HOST-INTEGRATION` |
| `HostParity` | `BUILD-A2-HOST-INTEGRATION` |
| `IntegrationDecision` | `BUILD-A2-REVIEW-INTEGRATION` |
| `IntegrationRequest` | `BUILD-A2-REVIEW-INTEGRATION` |
| `LogicalRole` | `BUILD-A2-STATE-CONTEXT` |
| `Model` | `BUILD-A2-MODEL-ROUTING` |
| `ModelCapability` | `BUILD-A2-MODEL-ROUTING` |
| `ModelObservation` | `BUILD-A2-MODEL-ROUTING` |
| `ModelRefresh` | `BUILD-A2-MODEL-ROUTING` |
| `NormalizedHostEvent` | `BUILD-A2-HOST-INTEGRATION` |
| `PolicyEligibilityEvaluator` | `BUILD-A2-MODEL-ROUTING` |
| `ProductEntitlement` | `BUILD-A2-STATE-CONTEXT` |
| `Provenance` | `BUILD-A2-REVIEW-INTEGRATION` |
| `Provider` | `BUILD-A2-RUNTIME-ADAPTERS` |
| `ProviderPolicyEligibility` | `BUILD-A2-MODEL-ROUTING` |
| `QuotaState` | `BUILD-A2-MODEL-ROUTING` |
| `RepairCapsule` | `BUILD-A2-ORCHESTRATION` |
| `RepairRequest` | `BUILD-A2-REVIEW-INTEGRATION` |
| `ReviewCapsule` | `BUILD-A2-REVIEW-INTEGRATION` |
| `ReviewRequest` | `BUILD-A2-REVIEW-INTEGRATION` |
| `RoutingDecision` | `BUILD-A2-MODEL-ROUTING` |
| `RoutingRequest` | `BUILD-A2-MODEL-ROUTING` |
| `RuntimeAdapter` | `BUILD-A2-RUNTIME-ADAPTERS` |
| `SafetyInterruption` | `BUILD-A2-REVIEW-INTEGRATION` |
| `SubtaskRequest` | `BUILD-A2-ORCHESTRATION` |
| `TaskCapsule` | `BUILD-A2-ORCHESTRATION` |
| `TaskDag` | `BUILD-A2-ORCHESTRATION` |
| `WorkspaceCheckpoint` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `WorkspaceHandle` | `BUILD-A2-WORKSPACE-EXECUTION` |

**Derived:** 51 individual contracts / interfaces · owner collisions = **0** · ownerless = **0**.

## Consumption

**Every row is a contract owned by a *different* manager.** A manager never appears as consumer of a contract it owns. Validated as `SELF_OWNED_EXTERNAL_CONSUMPTIONS = 0`.

| Consumer | Consumes (owned elsewhere) |
|---|---|
| `BUILD-A2-STATE-CONTEXT` | — |
| `BUILD-A2-WORKSPACE-EXECUTION` | `TaskCapsule`, `GraphNode` |
| `BUILD-A2-RUNTIME-ADAPTERS` | `WorkspaceHandle`, `TaskCapsule`, `RepairCapsule`, `ReviewCapsule`, `DispatchAdmissionDecision` |
| `BUILD-A2-ORCHESTRATION` | `ReviewRequest`, `A4Review`, `IntegrationDecision`, `AssuranceProfile`, `RoutingRequest`, `RoutingDecision`, `ProviderPolicyEligibility`, `WorkspaceHandle`, `LogicalRole`, `ExecutorBinding`, `ContextManifest`, `ContextEpoch`, `ProductEntitlement`, `EntitlementVerifier`, `ActivationState`, `NormalizedHostEvent` |
| `BUILD-A2-MODEL-ROUTING` | `RuntimeAdapter`, `Provider`, `A4Review`, `FeatureAdmissionDecision` |
| `BUILD-A2-REVIEW-INTEGRATION` | `TaskCapsule`, `RepairCapsule`, `GraphNode`, `GraphNodeResult`, `RoutingRequest`, `RoutingDecision`, `ModelObservation`, `WorkspaceHandle`, `WorkspaceCheckpoint`, `RuntimeAdapter`, `ContextEpoch`, `DispatchAdmissionDecision` |
| `BUILD-A2-HOST-INTEGRATION` | `RoutingDecision`, `IntegrationDecision`, `A4Review`, `GoalEvaluation`, `LogicalRole`, `WorkspaceHandle`, `ExecutionGraph`, `GraphSnapshot`, `FeatureCapabilitySet`, `FeatureAdmissionDecision`, `ProductEntitlement`, `ActivationState` |

## V1.3.1 additions

| Contract | Owner | Reason |
|---|---|---|
| `DispatchAdmissionDecision` | **ORCHESTRATION** | It composes the six axes and authorises dispatch — that is core admission authority, alongside `FeatureAdmissionDecision`. Placing it in routing would let routing grant itself permission |
| `ActivationState` | **STATE-CONTEXT** | Activation provenance is durable local state with the same integrity requirements as the entitlement cache it disambiguates |

`RUNTIME-ADAPTERS` and `REVIEW-INTEGRATION` now consume `DispatchAdmissionDecision` rather than `FeatureAdmissionDecision` directly: **every provider dispatch consumes the composed decision**, never the entitlement axis alone.

## V1.3 ownership decisions

| Contract family | Owner | Reason |
|---|---|---|
| `ExecutionGraph`, `GraphNode`, `GraphEdge`, `GraphMutation`, `GraphSnapshot`, `GraphNodeResult`, `GraphExecutionPolicy`, `TaskDag` | **ORCHESTRATION** | Graph semantics are orchestration semantics. Splitting them into a new manager would have required an eighth BUILD-A2 and broken the accepted topology |
| `FeatureCapabilitySet`, `FeatureAdmissionDecision` | **ORCHESTRATION** | §91 — normative feature admission is core authority, sitting on the dispatch path |
| `ProductEntitlement`, `EntitlementVerifier` | **STATE-CONTEXT** | Token persistence, verification and cache are durable-state mechanics with the same integrity requirements as other state |
| `ProviderPolicyEligibility`, `PolicyEligibilityEvaluator` | **MODEL-ROUTING** | §92 — the router decides and consumes; RUNTIME-ADAPTERS supplies evidence through a frozen contract, avoiding a concrete build cycle |
| `HostCapabilityReport` | **HOST-INTEGRATION** | Host probing is host knowledge |

**Host Integration deliberately does not own entitlement truth** merely because it renders the login screen. It consumes `ProductEntitlement`, `FeatureCapabilitySet` and `FeatureAdmissionDecision` for display only.

## Ownership rules

**The invariant, stated once:**

> **CONTRACT OWNERSHIP FOLLOWS NORMATIVE SHAPE AUTHORITY, NOT DATA-FLOW DIRECTION.**

The owner of a contract is the manager that defines and versions its shape — the one whose domain the contract encodes. Which manager emits the bytes, and in which direction they travel, is irrelevant to ownership. A manager may own a contract it never produces, and produce a contract it does not own.

`ModelObservation` is the worked example. REVIEW-INTEGRATION **produces** it and MODEL-ROUTING **consumes** it, yet MODEL-ROUTING **owns** it: calibration observation is routing's domain vocabulary, and routing is the manager that decides what an observation must contain for calibration to mean anything. Ownership sits with shape authority, against the direction of flow.

**Patterns, not laws.** The two shorthands below describe cases where shape authority and a flow role happen to coincide. They are descriptive of those rows and must never be applied as universal rules — doing so is precisely what produced the earlier false generalisation that "event contracts are owned by the producer", which `ModelObservation` contradicts under this package's own declared data flow.

| Pattern | Where it holds | Why shape authority lands there |
|---|---|---|
| Request contracts owned by the **acceptor** | `ReviewRequest` → REVIEW-INTEGRATION · `RoutingRequest` → MODEL-ROUTING | The acceptor defines what a well-formed request must contain in order to be serviceable |
| Host-event contracts owned by the **producer** | `NormalizedHostEvent` → HOST-INTEGRATION | Normalising heterogeneous host events *is* the producing manager's domain; the normalised shape is its deliverable |
| Neither pattern applies | `ModelObservation` → MODEL-ROUTING (produced by REVIEW-INTEGRATION) | Consumer holds shape authority; flow direction is not consulted |

Cross-check: `RUNTIME_INTERACTION_GRAPH.md` records producer → consumer direction and owner as **separate columns**, and its rows agree with the ownership table above. Validated as `CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS = 0`.
