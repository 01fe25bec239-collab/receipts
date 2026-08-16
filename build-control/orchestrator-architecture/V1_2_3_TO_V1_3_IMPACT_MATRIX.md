<!--
MultiAgent Orchestrator Architecture — HISTORICAL SNAPSHOT
DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT
SNAPSHOT: V1.3
Issued by: BUILD-A1-BOOTSTRAP
Status: PRESERVED HISTORICAL RECORD — NOT current architectural truth.
Supersession record for V1.2.3 to V1.3. Superseded chain continues in V1_3_TO_V1_3_1_IMPACT_MATRIX.md and V1_3_1_TO_V1_3_2_IMPACT_MATRIX.md.
This document records what was believed at the time it was written. Where it
disagrees with a CURRENT_NORMATIVE document, the current document governs.
It contributes NO current evidence assertion to normative validation.
-->

# V1_2_3_TO_V1_3_IMPACT_MATRIX

Classification: `UNCHANGED` · `REVISED` · `SUPERSEDED` · `RETIRED` · `NEW_COMPANION`

## Specifically identified items (§116)

| Item | Status | Detail |
|---|---|---|
| **TaskDag** | **SUPERSEDED** | Replaced by `ExecutionGraph`. Retained as a **reduced compatibility view**: a read-only projection exposing ready-task and dependency-state queries over the precedence subgraph. It no longer owns scheduling semantics, node identity, or mutation. Remains an M0 behavioural interface with that narrowed responsibility. See `TASK_DAG_AND_SCHEDULER.md`. |
| **Codex supervised-only architecture** | **SUPERSEDED** | Replaced by `HOST_CAPABILITY_DISCOVERY`. Supervision remains a fully supported mode, no longer the only one. |
| **A-14** ("Codex has no lifecycle-hook system") | **REVISED** | Preserved as a historical observation dated 2026-08-13; removed as an architectural constant. Nothing in V1.3 depends on it either way. The claim that Codex now has plugins is `USER_DECLARED`, not verified. |
| **PERSONAL_LOCAL_MODE** | **REVISED** | Subsumed by credential modes (`PERSONAL_LOCAL_CLI`) plus `ProviderPolicyEligibility`. The single-user local posture survives; "mode" is no longer the axis that decides permission. |
| **PRODUCT_TEAM_MODE** | **REVISED** | Now expressed as credential modes `USER_API` / `ENTERPRISE_GATEWAY` plus execution contexts. Still deferred for MVP. |
| **Provider credential architecture** | **REVISED** | Delegation-beats-extraction rule unchanged. Adds the policy-eligibility axis and the four-way separation. |
| **Host parity** | **REVISED** | Parity contract becomes plan-aware: FREE and PRO parity sets, both hosts. |
| **MVP scope** | **REVISED** | Must now prove a FREE vertical slice **and** a PRO vertical slice. |

## Full artifact classification

| Artifact | Status |
|---|---|
| `PRODUCT_DEFINITION` | **REVISED** — graph-native, two tiers |
| `MVP_SCOPE` | **REVISED** — two vertical slices |
| `NEW_SYSTEM_ARCHITECTURE` | **REVISED** — admission layer added above routing |
| `SYSTEM_DIAGRAM` | **REVISED** |
| `TASK_DAG_AND_SCHEDULER` | **SUPERSEDED** by graph documents; retained as compatibility-view spec |
| `HOST_ARCHITECTURE` | **REVISED** — discovery-driven posture |
| `HOST_PARITY_CONTRACT` | **REVISED** — plan-aware |
| `CLAUDE_HOST_ADAPTER` | **REVISED** · `NEW_COMPANION`: `CLAUDE_PLUGIN_PACKAGING` |
| `CODEX_HOST_ADAPTER` | **REVISED** · `NEW_COMPANION`: `CODEX_PLUGIN_PACKAGING` |
| `CROSS_HOST_RESUME` | **REVISED** — adds shared entitlement and graph version |
| `NORMALIZED_HOST_EVENTS` | **UNCHANGED** |
| `PROVIDER_CREDENTIAL_ARCHITECTURE` | **REVISED** · `NEW_COMPANION`: `PROVIDER_POLICY_ELIGIBILITY` |
| `RUNTIME_ADAPTER_INTERFACE` | **UNCHANGED** |
| `MODEL_INTELLIGENCE_ARCHITECTURE` | **REVISED** — eligibility filtering before candidate generation |
| `PROVIDER_MODEL_REGISTRY` | **REVISED** |
| `ROUTING_POLICY` | **REVISED** — admission and eligibility precede scoring |
| `RATE_LIMIT_AND_AVAILABILITY` | **REVISED** — availability no longer overloads policy or licence |
| `RUNTIME_ROLE_MODEL` | **REVISED** — policy-dependent instantiation |
| `RUNTIME_A1/A2/A3/A4` documents | **UNCHANGED** for PRO; FREE uses `GraphCoordinator` |
| `STATE_AND_CHECKPOINT_ARCHITECTURE` | **REVISED** — graph tables, entitlement cache |
| `EVENT_MODEL` | **REVISED** — graph and entitlement events |
| `A3_A4_REPAIR_LOOP` | **REVISED** — expressed as graph expansion |
| `REVIEW_VERIFICATION_PROVENANCE` | **REVISED** · `NEW_COMPANION`: `GRAPH_PROVENANCE_MODEL` |
| `GLOBAL_GOAL_EVALUATOR` | **REVISED** — appends via `GraphMutation` |
| `SECURITY_TRUST_MODEL` | **REVISED** — licensing never downgrades safety |
| `SAFETY_INTERRUPTION_PROTOCOL` | **UNCHANGED** |
| `INTEGRATION_GATE_ARCHITECTURE` | **UNCHANGED** |
| `ASSURANCE_PROFILES` | **REVISED** — FREE floor vs PRO profiles |
| `CONTRACT_IMPACT_MATRIX`, `CONTRACT_CONSUMPTION_GRAPH`, `RUNTIME_INTERACTION_GRAPH` | **REVISED** — new contracts |
| `BUILD_A2_DECOMPOSITION`, `BUILD_A2_OWNERSHIP_MATRIX` | **REVISED** — new ownership, same seven managers |
| `BUILD_IMPLEMENTATION_DAG` | **REVISED** — regenerated; **7 nodes / 10 edges / 0 cycles unchanged** |
| `IMPLEMENTATION_MILESTONES` | **REVISED** — M0 regenerated |
| `REPOSITORY_LAYOUT_PROPOSAL` | **REVISED** — graph, entitlement, public/proprietary split |
| `ASSUMPTION_REGISTER` | **REVISED** · `NEW_COMPANION`: `SOURCE_VERIFICATION_MATRIX_V1_3` |
| `OPEN_QUESTIONS`, `DEFERRED_CAPABILITIES`, `NON_GOALS` | **REVISED** |
| `REQUIREMENTS_TRACEABILITY_MATRIX`, `SCENARIO_VALIDATION` | **REVISED** — new families and scenarios |
| `HISTORICAL_BASELINE_ERRATA`, `REPOSITORY_RECONCILIATION_REPORT` | **UNCHANGED** |
| `ADR_IMPACT_MATRIX`, `OLD_RECEIPTS_REUSE_MATRIX` | **UNCHANGED** |

**No silent semantic drift.** Every V1.2.3 artifact has a status.
