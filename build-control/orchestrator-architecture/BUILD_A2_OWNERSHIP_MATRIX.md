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

# BUILD_A2_OWNERSHIP_MATRIX

**Ownership is deterministic.** Every path resolves to exactly one owner by prefix match, longest prefix wins. Validated programmatically in `PACKAGE_VALIDATION_REPORT.md`.

**V1.3.4: completeness, not just collision-freedom.** `PATH_OWNER_COLLISIONS = 0` alone only proves no two rows in this table disagree — it says nothing about a repository path this architecture requires (`REPOSITORY_LAYOUT_PROPOSAL.md`) that never got a row at all. Required paths are now derived from that layout document's manager annotations and checked against this table: `UNOWNED_REQUIRED_PATHS = 0` (every required path has a row), `AMBIGUOUS_REQUIRED_PATHS = 0` (never more than one), `REQUIRED_PATH_OWNER_MISMATCHES = 0` (the resolved owner matches the layout's own annotation).

## Source ownership

| Path prefix | Owner |
|---|---|
| `src/core/graph/**` · `src/core/orchestration/**` · `src/core/goal/**` · `src/core/dag/**` · `src/core/capsules/**` · `src/core/scheduler/**` · `src/core/entitlement/admission/**` | BUILD-A2-ORCHESTRATION |
| `src/intelligence/**` · `src/routing/**` · `src/registry/**` · `src/availability/**` · `src/routing/policy_eligibility/**` | BUILD-A2-MODEL-ROUTING |
| `src/adapters/runtime/**` · `src/adapters/credentials/**` | BUILD-A2-RUNTIME-ADAPTERS |
| `src/workspace/**` · `src/execution/**` | BUILD-A2-WORKSPACE-EXECUTION |
| `src/review/**` · `src/assurance/**` · `src/integration/**` · `src/security/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/state/**` · `src/context/**` · `src/events/**` · `src/state/entitlement/**` | BUILD-A2-STATE-CONTEXT |
| `src/hosts/**` · `plugins/claude/**` · `plugins/codex/**` · `integrations/codex-fallback/**` · `tests/parity/**` | BUILD-A2-HOST-INTEGRATION |
| `tests/security/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/pro/orchestration/**` | BUILD-A2-ORCHESTRATION |
| `src/pro/model-routing/**` | BUILD-A2-MODEL-ROUTING |
| `src/pro/review-integration/**` | BUILD-A2-REVIEW-INTEGRATION |

## Documentation ownership — deterministic by directory (V1.2 correction)

V1.1 said *"`docs/**` → the engineering manager owning the described subsystem"* and then claimed no path had two owners. That was **ambiguous, not deterministic**: nothing resolved a path mechanically. Replaced with directory-scoped ownership.

| Path | Owner |
|---|---|
| `docs/orchestration/**` | BUILD-A2-ORCHESTRATION |
| `docs/model-routing/**` | BUILD-A2-MODEL-ROUTING |
| `docs/runtime-adapters/**` | BUILD-A2-RUNTIME-ADAPTERS |
| `docs/workspace-execution/**` | BUILD-A2-WORKSPACE-EXECUTION |
| `docs/review-integration/**` | BUILD-A2-REVIEW-INTEGRATION |
| `docs/state-context/**` | BUILD-A2-STATE-CONTEXT |
| `docs/host-integration/**` | BUILD-A2-HOST-INTEGRATION |

A subsystem's documentation lives in its own directory and is written as an economy BUILD-A3 task under that manager. **There is still no docs manager** (§87) — directory scoping gives determinism without creating one.

## Shared top-level files — `BUILD_A1_CONTROLLED_SHARED_FILE`

| Path | Control authority |
|---|---|
| `README.md` | **BUILD-A1** |
| `ARCHITECTURE.md` | **BUILD-A1** |
| `SECURITY.md` | **BUILD-A1** |
| `CHANGELOG.md` | **BUILD-A1** |

These describe the system as a whole, so no single engineering manager can own them without either claiming authority over others' subsystems or fragmenting the file. BUILD-A1 holds control authority; managers contribute content through a change request, and BUILD-A1 accepts or rejects. This is a control decision, **not** a documentation manager — BUILD-A1 already owns cross-cutting control artifacts.

## BUILD-A1-controlled directories

| Path prefix | Control authority |
|---|---|
| `architecture/**` | **BUILD-A1** |
| `build-control/**` | **BUILD-A1** |

These are the explicit BUILD-A1-controlled class referenced by `REQUIRED_PATH_OWNER_MISMATCHES`: a required path resolving here is not unowned — it resolves to BUILD-A1 control authority rather than to a BUILD-A2 engineering manager.

## Contract ownership — authoritative (V1.3)

Derived from the same source as `CONTRACT_CONSUMPTION_GRAPH.md` and each schema's `x-owner`.

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
