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

# BUILD-A2-MODEL-ROUTING

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-MODEL-ROUTING` — Model Intelligence & Routing.

## Mission
Implement the subsystem that decides *who does the work*: the Model Intelligence Service, provider/model/runtime registries, capability lifecycle, refresh policy, local calibration, the deterministic router, expected-cost-to-acceptance estimation, and availability/quota state.

## Why long-lived
This subsystem carries the product's hardest invariant — **no executor selection from training memory** (I-4) — and its most volatile inputs. It needs a single owner who keeps the router deterministic and evidence-driven while the external model landscape churns underneath it.

## Owned subsystem
Model Intelligence Service · Provider/Model/Runtime registries · `MODEL_CAPABILITY` assessment and confidence classes · capability lifecycle state machine · refresh policy and freshness tracking · local calibration store · router · cost-to-acceptance estimator · availability and quota manager.

## Owned repository paths
`src/intelligence/**` · `src/routing/**` · `src/registry/**` · `src/availability/**` · owned schemas · **`docs/model-routing/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`AvailabilityState` · `Model` · `ModelCapability` · `ModelObservation` · `ModelRefresh` · `PolicyEligibilityEvaluator` · `ProviderPolicyEligibility` · `QuotaState` · `RoutingDecision` · `RoutingRequest`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `RuntimeAdapter` | `BUILD-A2-RUNTIME-ADAPTERS` |
| `Provider` | `BUILD-A2-RUNTIME-ADAPTERS` |
| `A4Review` | `BUILD-A2-REVIEW-INTEGRATION` |
| `FeatureAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |


## Reference-only
`TASK_CAPSULE`, `ASSURANCE_PROFILE`

## Forbidden ownership
Adapter implementations · credential storage · execution · review · state internals · host adapters.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.
- `BUILD-A2-RUNTIME-ADAPTERS` — **concrete implementation required.** Needs real capability probing and execution; a stub cannot report a runtime's true flags.

**Build wave: W3** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `RuntimeAdapter` — owned by `BUILD-A2-RUNTIME-ADAPTERS`; frozen at M0.
- `Provider` — owned by `BUILD-A2-RUNTIME-ADAPTERS`; frozen at M0.
- `A4Review` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `FeatureAdmissionDecision` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-REVIEW-INTEGRATION` — RoutingDecision
- ↔ `BUILD-A2-ORCHESTRATION` — RoutingDecision
- ↔ `BUILD-A2-RUNTIME-ADAPTERS` — capability probe, health
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories
Registry schema and persistence · capability lifecycle state machine · refresh policy and TTL engine · calibration store and metric computation · sample-size-weighted blending · cost-to-acceptance estimator · router filter pipeline · routing decision record · availability state machine and backoff · routing modes and user pinning · `UNKNOWN` propagation.

## Expected BUILD-A4 review categories
**Verification that no code path selects a model from anything but the registry** (I-4) · no hard-coded model names in control flow (I-17) · no vendor ranking constants (§29) · floor never violated by cost optimisation (I-9) · `UNKNOWN` never coerced to a default · small-sample statistics honesty · quota-scope assumptions absent unless verified.

## Frontier / economy policy
Frontier for the router, estimator, lifecycle, and calibration blending. Economy for registry documentation only.

## Security responsibility
Ensures routing never uses provider nationality or any safety-bypass heuristic (I-12); ensures a `POLICY_BLOCKED` provider is never treated as a rate limit; ensures credentials never enter routing records.

## Integration responsibility
Every dispatch must produce a persisted, explainable `ROUTING_DECISION`. Routing without a decision record is a defect.

## Context requirements
Initial: architecture, eight owned contracts, `ASSUMPTION_REGISTER` (its inputs are the volatile ones), adapter interface. Rehydration: on any provider-fact change, on adapter interface change, before frontier-critical work.

## Non-goals
Does not implement adapters · does not store credentials · does not execute or review · does not decide assurance depth.

## First proposed milestone
`M-ROUTE-1`: registries + capability lifecycle + deterministic router with hard filters and a recorded decision, using static seed evidence and no calibration.
