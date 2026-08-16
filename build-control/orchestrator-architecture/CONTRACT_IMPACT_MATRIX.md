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

# CONTRACT_IMPACT_MATRIX

## Rule

Contracts are not silently transferred. Each old contract gets an explicit disposition. **Versions do not carry over**: a revised concept enters the new architecture as a *new* contract at `0.1.0-draft`, so nothing inherits a frozen `1.0.0` it did not earn in this domain.

## Old contracts (21, frozen at `CONTRACT_FREEZE_SHA`)

| Contract | Old purpose | Disposition | New BUILD-A2 owner | Reason |
|---|---|---|---|---|
| `CONTRACT_CORE_001` CodeStateFingerprint | Exact code-state identity | **REVISE** | WORKSPACE-EXECUTION | Simplified to SHA + dirty state; digest retained only for uncommitted identification |
| `CONTRACT_CORE_002` Task + TaskState | Task model | **REVISE** | ORCHESTRATION | Superseded by the DAG task model + Task Capsule |
| `CONTRACT_CORE_003` Claim + ClaimStatus | Four MVP claim types | **RETIRE** | — | Claims replaced by acceptance criteria + A4 verdicts |
| `CONTRACT_EVIDENCE_001` Evidence families | Deterministic vs probabilistic | **REVISE** | REVIEW-INTEGRATION | Reframed as evidence sources in `Provenance` |
| `CONTRACT_POLICY_001` VerificationPolicy | LIGHT/STANDARD profiles | **REVISE** | REVIEW-INTEGRATION | Becomes `ASSURANCE_PROFILE` |
| `CONTRACT_POLICY_002` Admission | Admission decision | **REVISE** | REVIEW-INTEGRATION | Becomes `INTEGRATION_DECISION` |
| `CONTRACT_CONFIG_002` `.receipts/policy.yaml` | Policy config | **SUPERSEDE** | ORCHESTRATION | Replaced by project orchestration config |
| `CONTRACT_LEDGER_001` LedgerEvent | Event + hash chain | **REVISE** | STATE-CONTEXT | Event model kept; hash chain deferred |
| `CONTRACT_LEDGER_002` Append-only invariants | Storage invariants | **KEEP** (re-owned) | STATE-CONTEXT | Append-only discipline carries over directly |
| `CONTRACT_EXPORT_001` Portable export | Independent verification | **DEFER** | STATE-CONTEXT | Valuable, not MVP |
| `CONTRACT_RUNNER_001` VerificationRecipe | Human-approved commands | **RETIRE** | — | Verification plan now issued in the capsule |
| `CONTRACT_RUNNER_002` ExecutionReceipt | Execution record | **REVISE** | WORKSPACE-EXECUTION | Becomes execution evidence |
| `CONTRACT_CONFIG_001` `.receipts/recipes.yaml` | Recipe config | **RETIRE** | — | No standalone recipe file |
| `CONTRACT_PLUGIN_001` Hook normalization | Claude hook → broker | **REVISE** | HOST-INTEGRATION | Becomes `NORMALIZED_HOST_EVENT` (host-neutral) |
| `CONTRACT_PLUGIN_002` Hook decision | Broker → hook | **REVISE** | HOST-INTEGRATION | Folded into `HOST_ADAPTER` |
| `CONTRACT_CLI_001` CLI semantics | Command surface + typed errors | **REVISE** | HOST-INTEGRATION | Typed error model survives; command surface replaced |
| `CONTRACT_REVIEW_001` ReviewRequest | Review input | **REVISE** | REVIEW-INTEGRATION | Becomes `REVIEW_CAPSULE` |
| `CONTRACT_REVIEW_002` ReviewResult/Finding | Review output | **REVISE** | REVIEW-INTEGRATION | Becomes `A4_REVIEW` |
| `CONTRACT_REVIEW_003` ReviewProvider | 4-op provider interface | **SUPERSEDE** | RUNTIME-ADAPTERS | Becomes `RUNTIME_ADAPTER` |
| `CONTRACT_CONFIG_003` `.receipts/providers.yaml` | Provider config | **REVISE** | RUNTIME-ADAPTERS | Becomes provider/credential configuration |
| `CONTRACT_OVERRIDE_001` Override/Waiver | Human break-glass | **REVISE** | REVIEW-INTEGRATION | Becomes `HUMAN_REQUIRED` + user authority at the gate |

**Totals (derived programmatically from the 21 rows above, V1.2):** KEEP 1 · REVISE 14 · SUPERSEDE 2 · RETIRE 3 · DEFER 1 = **21**.

> **V1.1 correction.** The V1.1 summary read *REVISE 13 · SUPERSEDE 3*, which did not match its own table. The rows were correct; the hand-typed summary was not. **No disposition was changed to fit the bad summary.** `CONTRACT_REVIEW_003` and `CONTRACT_CONFIG_002` are the two SUPERSEDE rows; every other reworked contract is REVISE.

## New contracts (§90)

Created only where a real interface boundary exists. **No contract bureaucracy.**

| Contract | Owner | Purpose |
|---|---|---|
| `HOST_PARITY` | HOST-INTEGRATION | The capability set both hosts must satisfy |
| `HOST_ADAPTER` | HOST-INTEGRATION | Adapter interface |
| `NORMALIZED_HOST_EVENT` | HOST-INTEGRATION | Host → core event vocabulary |
| `GOAL` | ORCHESTRATION | Goal identity, spec refs, acceptance criteria |
| `GOAL_EVALUATION` | ORCHESTRATION | Evaluator inputs/outputs |
| `LOGICAL_ROLE` | STATE-CONTEXT | Durable role identity |
| `EXECUTOR_BINDING` | STATE-CONTEXT | Role ↔ executor association |
| `TASK_CAPSULE` | ORCHESTRATION | A3 context contract |
| `REPAIR_CAPSULE` | ORCHESTRATION | Repair context contract |
| `REVIEW_REQUEST` | REVIEW-INTEGRATION | ORCHESTRATION→REVIEW boundary; the acceptor owns it |
| `REVIEW_CAPSULE` | REVIEW-INTEGRATION | A4 context contract |
| `TASK_DAG` | ORCHESTRATION | Graph, edges, states |
| `SUBTASK_REQUEST` | ORCHESTRATION | Controlled discovery |
| `PROVIDER` | RUNTIME-ADAPTERS | Provider identity, auth, policy |
| `MODEL` | MODEL-ROUTING | Model identity and tiers |
| `RUNTIME_ADAPTER` | RUNTIME-ADAPTERS | Agent runtime interface |
| `MODEL_CAPABILITY` | MODEL-ROUTING | Capability + provenance + confidence |
| `MODEL_OBSERVATION` | MODEL-ROUTING | Local calibration record |
| `MODEL_REFRESH` | MODEL-ROUTING | Freshness/trigger policy |
| `ROUTING_REQUEST` | MODEL-ROUTING | Capability-first request |
| `ROUTING_DECISION` | MODEL-ROUTING | Explainable decision record |
| `AVAILABILITY_STATE` | MODEL-ROUTING | Normalized availability |
| `QUOTA_STATE` | MODEL-ROUTING | Quota, `retry_after`, `UNKNOWN` |
| `WORKSPACE` | WORKSPACE-EXECUTION | Branch/worktree handle |
| `CHECKPOINT` | WORKSPACE-EXECUTION | Recovery snapshot |
| `A3_HANDOFF` | REVIEW-INTEGRATION | Structured implementation handoff |
| `A4_REVIEW` | REVIEW-INTEGRATION | Structured verdict |
| `REPAIR_REQUEST` | REVIEW-INTEGRATION | Repair issuance |
| `CONTEXT_MANIFEST` | STATE-CONTEXT | Authoritative context refs |
| `CONTEXT_EPOCH` | STATE-CONTEXT | Invalidation marker |
| `PROVENANCE` | REVIEW-INTEGRATION | Evidence ↔ SHA binding |
| `ASSURANCE_PROFILE` | REVIEW-INTEGRATION | Verification depth |
| `INTEGRATION_REQUEST` | REVIEW-INTEGRATION | Evidence submitted to a gate |
| `INTEGRATION_DECISION` | REVIEW-INTEGRATION | Gate outcome and provenance chain |
| `SAFETY_INTERRUPTION` | REVIEW-INTEGRATION | Pending/blocked safety state |

**35 individual new contracts / interfaces**, each with exactly one owner. No contract is ownerless or multiply owned — verified in `BUILD_A2_OWNERSHIP_MATRIX.md` and recomputed in `PACKAGE_VALIDATION_REPORT.md`.

> **V1.2.1 correction.** V1.2 stated *33 contracts* while `CONTRACT_CONSUMPTION_GRAPH.md` derived a different figure. Two causes: `INTEGRATION_REQUEST / INTEGRATION_DECISION` was a single table row representing **two** contracts with **two** schemas, and `ReviewRequest` was referenced as a boundary without ever being defined. Both are fixed: the row is split, `ReviewRequest` is now a normative contract with an owner and a schema, and all counts are derived rather than typed. Where the distinction matters, this package reports `CONTRACT_TABLE_ROWS` and `INDIVIDUAL_CONTRACTS_OR_INTERFACES` separately.
