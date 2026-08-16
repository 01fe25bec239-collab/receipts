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

# MVP_SCOPE

## Two vertical slices, both required

FREE is **not** a later feature (§75). MVP must prove both paths end to end.

### FREE vertical slice

```
goal / SPEC.md
  → GraphCompiler → ExecutionGraph
  → single eligible host-native runtime
  → deterministic quality gate
  → selective retry on failure
  → graph completes, inspectable, resumable
```

Exit: works with **no product account**, **no network call to our entitlement service**, on **both hosts**, with accurate node state and no false PASS.

### PRO vertical slice

```
same graph, same graph_id
  → entitlement admits Pro capabilities
  → distributed roles instantiated
  → eligible-runtime routing
  → RUNTIME-A3 implementation
  → fresh independent RUNTIME-A4
  → rejection → bounded repair → pass
  → integration
  → host switch mid-goal
  → Global Goal Evaluator → COMPLETE
```

Exit: the FREE graph continues into PRO **without recompile or reset**; provider diversity is labelled honestly; a policy-ineligible runtime is never dispatched.

## In scope

Graph core, compiler, scheduler, versioning, mutation audit · FREE execution policy · PRO distributed policy · entitlement service client, signed verification, offline grace, admission · capability catalog UX on both hosts · provider policy eligibility gate · host capability discovery with both plugin packages · Claude and Codex worker adapters · Model Intelligence with eligibility filtering · workspaces, checkpoints, recovery · A3→A4→repair · provenance with exact-SHA binding · integration gate · Global Goal Evaluator · SQLite state including graph tables.

## Out of scope for MVP

Managed inference (§69 — explicitly deferred, and **not required by Pro**) · team cloud control plane · web dashboard · enterprise org licensing · centralised telemetry · knowledge graph (§119) · full provenance visualisation · marketplace paid checkout · Gemini/Kimi/Grok/DeepSeek adapters · HIGH_ASSURANCE broker re-execution.

## Why both tiers in MVP

Shipping PRO first would make FREE a retrofit, and the FREE→PRO continuity requirement (§38) would become the least-tested path in the product — while being the one every paying customer traverses. Shipping FREE first alone would leave the entitlement and policy gates unproven until after launch.

## MVP success criterion

Scenarios S20–S31 in `SCENARIO_VALIDATION.md` all pass, including the FREE-invokes-PRO refusal with zero provider dispatch, the licence-outage case, and the policy-disallowed provider case.
