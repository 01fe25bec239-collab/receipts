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

# IMPLEMENTATION_MILESTONES

Ordering derives from `BUILD_IMPLEMENTATION_DAG.md`. **No milestone is authorized by this package.**

## M0 — CONTRACT / SCHEMA FREEZE (BUILD-A1)

The cycle-breaking mechanism. Frozen before any manager wave.

### Machine-validatable M0 schemas (36)

`A3Handoff` · `A4Review` · `ActivationState` · `AvailabilityState` · `ContextEpoch` · `ContextManifest` · `DispatchAdmissionDecision` · `ExecutionGraph` · `ExecutorBinding` · `FeatureAdmissionDecision` · `FeatureCapabilitySet` · `GoalEvaluation` · `GraphEdge` · `GraphMutation` · `GraphNode` · `GraphNodeResult` · `GraphSnapshot` · `HostCapabilityReport` · `IntegrationDecision` · `IntegrationRequest` · `LogicalRole` · `ModelCapability` · `ModelObservation` · `NormalizedHostEvent` · `ProductEntitlement` · `ProviderPolicyEligibility` · `QuotaState` · `RepairCapsule` · `ReviewCapsule` · `ReviewRequest` · `RoutingDecision` · `RoutingRequest` · `SafetyInterruption` · `TaskCapsule` · `WorkspaceCheckpoint` · `WorkspaceHandle`

**Correction (V1.3.4).** `HostCapabilityReport` has a real file at `schemas/HostCapabilityReport.schema.json` and was wrongly classified as behavioural at V1.3.3, contradicting the package's own contents. It is a serialized, schema-validated payload — persisted, versioned and timestamped (`HOST_CAPABILITY_DISCOVERY.md`) — not merely an in-process port, so it belongs in the machine-schema set. This list and its count are derived from the actual `schemas/` directory, never asserted; `M0_SCHEMA_COUNT_MATCHES_ACTUAL` and `M0_SCHEMA_CLASSIFICATION_MISMATCHES = 0` enforce it.

### Non-schema / behavioural M0 contracts (7)

| Contract | Owner | Why behavioural |
|---|---|---|
| `RuntimeAdapter` | RUNTIME-ADAPTERS | Behaviour-only port; frozen as a typed interface plus conformance suite |
| `AssuranceProfile` | REVIEW-INTEGRATION | Policy semantics; the selector is already a constrained enum inside three schemas |
| `TaskDag` | ORCHESTRATION | **Reduced to a read-only compatibility view** over the precedence subgraph |
| `Provider` | RUNTIME-ADAPTERS | Registry identity vocabulary; no standalone wire format |
| `EntitlementVerifier` | STATE-CONTEXT | Verification port; the token itself is `ProductEntitlement` (schema) |
| `PolicyEligibilityEvaluator` | MODEL-ROUTING | Decision port; the record itself is `ProviderPolicyEligibility` (schema) |
| `GraphExecutionPolicy` | ORCHESTRATION | Dispatch policy behaviour; FREE and PRO differ in behaviour, not in a payload |

### Why the split

A schema for `GraphExecutionPolicy` would describe a policy identifier and validate nothing of substance while implying validation occurred. The same reasoning applies to every behavioural entry: these are ports and semantics, not payloads — unlike `HostCapabilityReport`, none of them has a serialized wire form in `schemas/`.

### Exit criteria

Every schema validates against Draft 2020-12; every contract has exactly one owner; each schema's `x-owner` agrees with `CONTRACT_CONSUMPTION_GRAPH.md`; no `provider_id`, `model_id`, `runtime_id`, `tier_id` or capability id is constrained by an enum; every behavioural contract has a written interface definition; the machine-schema/behavioural split matches the actual `schemas/` directory (`M0_SCHEMA_CLASSIFICATION_MISMATCHES = 0`); each manager's `HARD_BUILD_DEPENDENCIES` prose matches the DAG and resolves to a strictly earlier wave (`UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES = 0`) — note this checks *build* dependencies only, never `Consumed contracts`/`FROZEN_CONTRACT_DEPENDENCIES`, which are frozen here at M0 and so are safe at any wave by construction (S19).


## Manager waves

### W1 — `BUILD-A2-STATE-CONTEXT`

- **`BUILD-A2-STATE-CONTEXT`**: hard deps none


### W2 — `BUILD-A2-ORCHESTRATION`, `BUILD-A2-RUNTIME-ADAPTERS`, `BUILD-A2-WORKSPACE-EXECUTION`

- **`BUILD-A2-ORCHESTRATION`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1)
- **`BUILD-A2-RUNTIME-ADAPTERS`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1)
- **`BUILD-A2-WORKSPACE-EXECUTION`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1)


### W3 — `BUILD-A2-HOST-INTEGRATION`, `BUILD-A2-MODEL-ROUTING`, `BUILD-A2-REVIEW-INTEGRATION`

- **`BUILD-A2-HOST-INTEGRATION`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1), `BUILD-A2-ORCHESTRATION` (W2)
- **`BUILD-A2-MODEL-ROUTING`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1), `BUILD-A2-RUNTIME-ADAPTERS` (W2)
- **`BUILD-A2-REVIEW-INTEGRATION`**: hard deps `BUILD-A2-STATE-CONTEXT` (W1), `BUILD-A2-WORKSPACE-EXECUTION` (W2), `BUILD-A2-RUNTIME-ADAPTERS` (W2)


## Milestone detail

**M1 (W1) — Durable substrate.** `BUILD-A2-STATE-CONTEXT`. State store (SQLite, frozen per Q-03), repository interface, transactional crash-safe writes, append-only event log with redaction, logical roles, executor bindings with leases, context manifests, epochs, rehydration engine, startup recovery.
*Exit:* survives kill mid-write; role identity and history survive simulated executor replacement; rehydration rereads sources and detects a changed digest; no write path reachable from outside the core.

**M2 (W2) — Execution, adapters, orchestration in parallel.** All three depend only on M1.
- `WORKSPACE-EXECUTION`: worktree lifecycle, argv runner, timeouts, capture, checkpoints, recovery, write-scope verification.
- `RUNTIME-ADAPTERS`: adapter interface + conformance suite, Claude and Codex adapters, failure classification, delegated credentials.
- `ORCHESTRATION`: DAG with cycle detection, task state machine, capsules, scheduler admission, role lifecycle, budgets, subtask requests.
*Exit:* isolated execution on both adapters; killed process leaves no orphan; recovery captures without applying; colliding tasks serialize; forbidden state transitions rejected.

**M3 (W3) — Routing, assurance, hosts in parallel.**
- `MODEL-ROUTING`: registries, capability lifecycle, refresh, deterministic router, explainable decisions, availability/quota, frontier floor and failover, calibration collection.
- `REVIEW-INTEGRATION`: automatic A3→A4 dispatch, review capsules, verdicts, bounded repair, assurance profiles, provenance, both gates.
- `HOST-INTEGRATION`: both adapters, event bridge, `START_GOAL`, `WorktreeCreate` handler with fallback, renderers.
*Exit:* no model selected outside the registry; rate limit triggers failover without task loss; a commit added after review blocks acceptance; self-review impossible; all `PARITY_CAPABILITY_COUNT` parity rows (`HOST_PARITY_CONTRACT.md`, currently P-01…P-25) pass on both hosts.

**M4 — Integration checkpoint.** Parity conformance suite green; cross-manager integration tests; north-star demo `S1`–`S19` (an intentional demo subset, not the release gate itself). Full architecture scenario validation is a separate, broader release gate: every scenario in `SCENARIO_VALIDATION.md` (`SCENARIO_COUNT` scenarios) must validate, not merely the S1–S19 demo subset.

## Parallelism claim

W2 runs three managers concurrently and W3 runs three concurrently. **This is claimed only because the DAG proves it**: within each wave, no member depends on another member's concrete implementation. Wave-order violations: **0**, checked programmatically.
