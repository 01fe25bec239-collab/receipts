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

# REQUIREMENTS_TRACEABILITY_MATRIX

Every major requirement from the reopen instruction, mapped to where it is satisfied and who owns it. `§` numbers refer to the instruction's sections.

**Status legend:** `SPECIFIED` = fully designed in this package. `SPECIFIED-PARTIAL` = designed with a named open question that is still open in `OPEN_QUESTIONS.md`. `DEFERRED` = deliberately out of MVP with a recorded reason.

**One matrix, one truth (V1.3.3).** Every requirement row below — across all three waves — is CURRENT. There is exactly one coverage summary and it is derived from every current row. Statuses are reconciled against `OPEN_QUESTIONS.md`: a row may name an open question, never a resolved one (`TRACEABILITY_STALE_RESOLVED_QUESTION_REFS = 0`). The superseded V1.2-only statistics are preserved at the foot of this page as an explicitly marked historical snapshot and are **not** a second current summary.

## Wave 1 — reopen instruction (§2–§98)

| § | Requirement | Architecture section | Runtime subsystem | BUILD-A2 owner | Contract / interface | Validation | Status |
|---|---|---|---|---|---|---|---|
| 2 | Formal architecture reopen | `ARCHITECTURE_REOPEN_001.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 3 | Source reconciliation | `RECONCILIATION_REPORT_V1_1.md` + `ASSUMPTION_REGISTER`, `CONFLICT_RESOLUTION_LOG` | — | BUILD-A1 | — | Snapshot reconciliation | **SPECIFIED — gap closed in V1.1** |
| 4 | New product definition | `PRODUCT_DEFINITION.md` | Goal Orchestrator | ORCHESTRATION | `GOAL` | S1 | SPECIFIED |
| 5 | Non-goals | `NON_GOALS.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 6 | Two namespaces not conflated | `NEW_SYSTEM_ARCHITECTURE §0` | — | all | — | §99 check | SPECIFIED |
| 7 | Long-horizon goal orchestration | `GOAL_ORCHESTRATOR.md` | Goal Orchestrator | ORCHESTRATION | `GOAL`, `TASK_DAG` | S1, S18 | SPECIFIED |
| 8 | `START_GOAL` on both hosts | `HOST_PARITY_CONTRACT.md` | Host adapters | HOST-INTEGRATION | `HOST_PARITY` | S1, S2 | SPECIFIED-PARTIAL (Q-01 command syntax) |
| 9 | Hard host-parity invariant | `HOST_PARITY_CONTRACT.md` | Parity conformance suite | HOST-INTEGRATION | `HOST_PARITY` | S1–S4 | SPECIFIED |
| 10–11 | Host-specific activation | `CLAUDE_HOST_ADAPTER.md`, `CODEX_HOST_ADAPTER.md` | Host adapters | HOST-INTEGRATION | `HOST_ADAPTER` | S1, S2 | SPECIFIED |
| 11 | Normalized host events | `NORMALIZED_HOST_EVENTS.md` | Event bridge | HOST-INTEGRATION | `NORMALIZED_HOST_EVENT` | S1–S4 | SPECIFIED |
| 12 | One shared core | `NEW_SYSTEM_ARCHITECTURE §1`, I-16 | Core | ORCHESTRATION | — | §99 check | SPECIFIED |
| 13 | Cross-host resume | `CROSS_HOST_RESUME.md` | State + host adapters | STATE-CONTEXT | `CONTEXT_MANIFEST` | S3, S4 | SPECIFIED |
| 14 | Durable RUNTIME-A1 | `RUNTIME_A1_LOGICAL_ROLE.md` | Role engine | ORCHESTRATION | `LOGICAL_ROLE` | S8 | SPECIFIED |
| 15 | Durable RUNTIME-A2 | `RUNTIME_A2_LOGICAL_ROLE.md` | Role engine | ORCHESTRATION | `LOGICAL_ROLE` | S7 | SPECIFIED |
| 16 | No docs-only RUNTIME-A2 | `RUNTIME_A2_LOGICAL_ROLE.md §Docs` | Role engine | ORCHESTRATION | — | S12 | SPECIFIED |
| 17 | Ephemeral RUNTIME-A3 | `RUNTIME_A3_LIFECYCLE.md` | A3 controller | REVIEW-INTEGRATION | `TASK_CAPSULE`, `A3_HANDOFF` | S6, S9 | SPECIFIED |
| 18 | Ephemeral RUNTIME-A4 | `RUNTIME_A4_LIFECYCLE.md` | A4 controller | REVIEW-INTEGRATION | `REVIEW_CAPSULE`, `A4_REVIEW` | S6 | SPECIFIED |
| 19–20 | Automatic bounded repair loop | `A3_A4_REPAIR_LOOP.md` | Repair controller | REVIEW-INTEGRATION | `REPAIR_REQUEST` | S6 | SPECIFIED |
| 21 | Task Capsule | `TASK_CAPSULE_SPEC.md` | Capsule factory | ORCHESTRATION | `TASK_CAPSULE` | S1 | SPECIFIED |
| 22 | Repair Capsule | `REPAIR_CAPSULE_SPEC.md` | Capsule factory | ORCHESTRATION | `REPAIR_CAPSULE` | S6 | SPECIFIED |
| 23 | Review Capsule | `REVIEW_CAPSULE_SPEC.md` | Capsule factory | REVIEW-INTEGRATION | `REVIEW_CAPSULE` | S6 | SPECIFIED |
| 23 | Review dispatch boundary | `BUILD_IMPLEMENTATION_DAG.md`, `schemas/ReviewRequest.schema.json` | A3→A4 controller | REVIEW-INTEGRATION | `REVIEW_REQUEST` | S19 | SPECIFIED |
| 24 | Global DAG + cycle detection | `TASK_DAG_AND_SCHEDULER.md` | DAG engine | ORCHESTRATION | `TASK_DAG` | S1 | SPECIFIED |
| 25 | Parallel execution | `CONCURRENCY_MODEL.md` | Scheduler | ORCHESTRATION | — | S10 | SPECIFIED |
| 26 | Controlled subtask requests | `SUBTASK_REQUEST_PROTOCOL.md` | DAG engine | ORCHESTRATION | `SUBTASK_REQUEST` | S18 | SPECIFIED |
| 27 | Model Intelligence Service | `MODEL_INTELLIGENCE_ARCHITECTURE.md` | MIS | MODEL-ROUTING | `MODEL_CAPABILITY` | S5 | SPECIFIED |
| 28 | No selection from training memory | `MODEL_INTELLIGENCE_ARCHITECTURE §Invariant`, I-4 | MIS + Router | MODEL-ROUTING | `ROUTING_REQUEST` | S5 | SPECIFIED |
| 29–30 | No hard-coded vendor quality; names are config | `ROUTING_POLICY.md`, I-17 | Router | MODEL-ROUTING | — | S5 | SPECIFIED |
| 31 | Dynamic fact verification | `ASSUMPTION_REGISTER.md`, `MODEL_REFRESH_POLICY.md` | MIS | MODEL-ROUTING | `MODEL_REFRESH` | S5 | SPECIFIED |
| 32–33 | New-model discovery + lifecycle | `MODEL_CAPABILITY_LIFECYCLE.md` | MIS | MODEL-ROUTING | `MODEL_CAPABILITY` | S5 | SPECIFIED |
| 34 | Capability evidence provenance | `MODEL_CAPABILITY_SCHEMA.md` | MIS | MODEL-ROUTING | `MODEL_CAPABILITY` | S5 | SPECIFIED |
| 35 | Local calibration | `MODEL_CALIBRATION.md` | Calibration store | MODEL-ROUTING | `MODEL_OBSERVATION` | S11 | SPECIFIED |
| 36 | Expected cost to accepted result | `EXPECTED_COST_TO_ACCEPTED_RESULT.md` | Estimator | MODEL-ROUTING | `ROUTING_DECISION` | S11 | SPECIFIED |
| 37–38 | Capability-first, explainable routing | `ROUTING_POLICY.md`, `ROUTING_DECISION_SCHEMA.md` | Router | MODEL-ROUTING | `ROUTING_REQUEST/DECISION` | S5, S11 | SPECIFIED |
| 39–40 | Routing modes + user pinning | `ROUTING_POLICY.md §Modes` | Router | MODEL-ROUTING | `ROUTING_REQUEST` | S5 | SPECIFIED |
| 41–42 | Frontier floor + failover | `QUALITY_COST_POLICY.md` | Router | MODEL-ROUTING | — | S16 | SPECIFIED |
| 43–44 | Economy routing + rendering | `QUALITY_COST_POLICY.md §Economy` | Router + renderer | MODEL-ROUTING | — | S12 | SPECIFIED |
| 45–46 | Provider≠Model≠Runtime; adapter iface | `PROVIDER_MODEL_REGISTRY.md`, `RUNTIME_ADAPTER_INTERFACE.md` | Registry + adapters | RUNTIME-ADAPTERS | `RUNTIME_ADAPTER` | S5 | SPECIFIED |
| 47 | Adapter maturity tiers | `RUNTIME_ADAPTER_INTERFACE.md §Tiers` | Adapters | RUNTIME-ADAPTERS | — | — | SPECIFIED |
| 48 | Refresh policy | `MODEL_REFRESH_POLICY.md` | MIS | MODEL-ROUTING | `MODEL_REFRESH` | S5 | SPECIFIED |
| 49–52 | Provider connections, modes, credential broker | `PROVIDER_CREDENTIAL_ARCHITECTURE.md`, `PERSONAL_LOCAL_MODE.md`, `PRODUCT_TEAM_MODE.md` | Credential broker | RUNTIME-ADAPTERS | `PROVIDER` | S5 | SPECIFIED |
| 53 | Availability/quota state | `RATE_LIMIT_AND_AVAILABILITY.md` | Availability manager | MODEL-ROUTING | `AVAILABILITY_STATE`, `QUOTA_STATE` | S6, S16 | SPECIFIED |
| 54 | A3 rate-limit failover | `SESSION_FAILOVER_ARCHITECTURE.md` | Repair + router | REVIEW-INTEGRATION | `REPAIR_CAPSULE` | S6 | SPECIFIED |
| 55 | A3 crash before commit | `WORKSPACE_RECOVERY.md` | Workspace manager | WORKSPACE-EXECUTION | `CHECKPOINT` | S9 | SPECIFIED |
| 56–58 | Durable A1/A2 identity + failover policy | `SESSION_FAILOVER_ARCHITECTURE.md` | Role engine + bindings | STATE-CONTEXT | `EXECUTOR_BINDING` | S7, S8 | SPECIFIED |
| 59–62 | Context rehydration, manifest, epoch, triggers | `CONTEXT_REHYDRATION_ARCHITECTURE.md`, `CONTEXT_MANIFEST_SPEC.md` | Rehydration engine | STATE-CONTEXT | `CONTEXT_MANIFEST`, `CONTEXT_EPOCH` | S17 | SPECIFIED |
| 63–65 | Git/worktree model, remote policy, not-a-sandbox | `WORKSPACE_EXECUTION_ARCHITECTURE.md`, `REMOTE_BRANCH_POLICY.md` | Workspace manager | WORKSPACE-EXECUTION | `WORKSPACE` | S10 | SPECIFIED |
| 66 | ADR-001 classification | `ADR_IMPACT_MATRIX.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 67–69 | Reframed provenance; worker checks are evidence | `REVIEW_VERIFICATION_PROVENANCE.md` | Provenance | REVIEW-INTEGRATION | `PROVENANCE` | S6 | SPECIFIED |
| 70 | Assurance profiles | `ASSURANCE_PROFILES.md` | Assurance engine | REVIEW-INTEGRATION | `ASSURANCE_PROFILE` | S13 | SPECIFIED |
| 71 | A4 independence | `RUNTIME_A4_LIFECYCLE.md` | A4 controller | REVIEW-INTEGRATION | `A4_REVIEW` | S6 | SPECIFIED |
| 72 | Security review pipeline | `SECURITY_REVIEW_ARCHITECTURE.md` | Security pipeline | REVIEW-INTEGRATION | `A4_REVIEW` | S13–S15 | SPECIFIED-PARTIAL (Q-06 tool set) |
| 73–77 | Safety interruption, policy blocked, no bypass, HUMAN_REQUIRED | `SAFETY_INTERRUPTION_PROTOCOL.md` | Safety state machine | REVIEW-INTEGRATION | `SAFETY_INTERRUPTION` | S13–S15 | SPECIFIED-PARTIAL (Q-07 detectability) |
| 78 | Integration gate | `INTEGRATION_GATE_ARCHITECTURE.md` | Gate | REVIEW-INTEGRATION | `INTEGRATION_REQUEST/DECISION` | S1 | SPECIFIED |
| 79–81 | Global Goal Evaluator | `GLOBAL_GOAL_EVALUATOR.md` | Evaluator | ORCHESTRATION | `GOAL_EVALUATION` | S18 | SPECIFIED |
| 82–83 | Durable state + event log | `STATE_AND_CHECKPOINT_ARCHITECTURE.md`, `EVENT_MODEL.md` | State store | STATE-CONTEXT | — | S3, S17 | SPECIFIED |
| 84 | Compute efficiency | `EXPECTED_COST_TO_ACCEPTED_RESULT.md`, `CONCURRENCY_MODEL.md` | Scheduler + router | MODEL-ROUTING | — | S11 | SPECIFIED |
| 85–88 | BUILD-A2 topology + specs | `BUILD_A2_DECOMPOSITION.md`, `BUILD_A2_MANAGERS/` | — | BUILD-A1 | — | Review | SPECIFIED |
| 89–90 | Contract reassessment + new contracts | `CONTRACT_IMPACT_MATRIX.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 91 | MCP position | `MCP_POSITION.md` | Host/adapters | HOST-INTEGRATION | — | — | SPECIFIED |
| 92 | Security/trust boundaries | `SECURITY_TRUST_MODEL.md` | cross-cutting | REVIEW-INTEGRATION | — | S13–S15 | SPECIFIED |
| 94 | Differentiation | `PRODUCT_DEFINITION.md §defensible` | — | BUILD-A1 | — | Review | SPECIFIED |
| 95–96 | MVP scope + north-star demo | `MVP_SCOPE.md`, `SCENARIO_VALIDATION.md` | — | BUILD-A1 | — | S1–S18 | SPECIFIED |
| 97 | Failure/falsification criteria | `FAILURE_CRITERIA.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 98 | Scenario validation | `SCENARIO_VALIDATION.md` | — | BUILD-A1 | — | S1–S18 | SPECIFIED |

## Wave 2 — V1.3 graph, tiers, entitlement, policy

| § | Requirement | Architecture section | Runtime subsystem | BUILD-A2 owner | Contract / interface | Validation | Status |
|---|---|---|---|---|---|---|---|
| 13 | ExecutionGraph first-class | `EXECUTION_GRAPH_MODEL.md` | Graph core | ORCHESTRATION | `ExecutionGraph` | S20 | SPECIFIED |
| 13 | Graph mutation audit | `GRAPH_MUTATION_PROTOCOL.md` | Graph core | ORCHESTRATION | `GraphMutation` | S33 | SPECIFIED |
| 13 | Graph provenance | `GRAPH_PROVENANCE_MODEL.md` | Provenance | ORCHESTRATION | `GraphNodeResult` | S30 | SPECIFIED |
| 23 | FREE tier is a real product | `FREE_PRO_PRODUCT_ARCHITECTURE.md` | Policy | ORCHESTRATION | `GraphExecutionPolicy` | S20, S21 | SPECIFIED |
| 26 | One engine, two policies | `GRAPH_EXECUTION_POLICIES.md` | Policy | ORCHESTRATION | `GraphExecutionPolicy` | S24 | SPECIFIED |
| 33 | Capability catalog | `FEATURE_CAPABILITY_MATRIX.md` | Admission | ORCHESTRATION | `FeatureCapabilitySet` | S23 | SPECIFIED |
| 35 | Product entitlement separate from provider auth | `PRODUCT_ENTITLEMENT_ARCHITECTURE.md` | Entitlement | STATE-CONTEXT | `ProductEntitlement` | S24, S25 | SPECIFIED |
| 40 | Entitlement admission | `ENTITLEMENT_ADMISSION_PROTOCOL.md` | Admission | ORCHESTRATION | `FeatureAdmissionDecision` | S22, S31 | SPECIFIED |
| 44 | Offline and expiry policy | `LICENSE_ACTIVATION_AND_OFFLINE_POLICY.md` | Entitlement | STATE-CONTEXT | `ProductEntitlement` | S26, S27 | SPECIFIED |
| 45 | Provider policy eligibility | `PROVIDER_POLICY_ELIGIBILITY.md` | Routing | MODEL-ROUTING | `ProviderPolicyEligibility` | S28, S29 | SPECIFIED-PARTIAL (Q-V13-04) |
| 51 | Open-core boundary | `OPEN_CORE_COMMERCIAL_BOUNDARY.md` | — | BUILD-A1 | — | S35 | SPECIFIED-PARTIAL (Q-V13-06) |
| 57 | Host capability discovery | `HOST_CAPABILITY_DISCOVERY.md` | Host adapters | HOST-INTEGRATION | `HostCapabilityReport` | S21, S34 | SPECIFIED |
| 58 | Claude plugin packaging | `CLAUDE_PLUGIN_PACKAGING.md` | Host adapters | HOST-INTEGRATION | `HostAdapter` | S20 | SPECIFIED |
| 58 | Codex plugin packaging | `CODEX_PLUGIN_PACKAGING.md` | Host adapters | HOST-INTEGRATION | `HostAdapter` | S21 | SPECIFIED |
| 61 | Product command surface | `PRODUCT_COMMAND_SURFACE.md` | Host adapters | HOST-INTEGRATION | — | S23 | SPECIFIED-PARTIAL (Q-01) |
| 63 | Plugin distribution | `PLUGIN_DISTRIBUTION_ARCHITECTURE.md` | — | HOST-INTEGRATION | — | S35 | SPECIFIED |
| 76 | Graph runtime layering | `GRAPH_RUNTIME_ARCHITECTURE.md` | Core | ORCHESTRATION | — | S20 | SPECIFIED |
| 116 | V1.2.3 → V1.3 impact | `V1_2_3_TO_V1_3_IMPACT_MATRIX.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 7 | Source verification and closure | `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`, `evidence/SOURCE_CLAIM_REGISTRY.json` | — | BUILD-A1 | — | Review | SPECIFIED |
| 7 | Consumer-subscription third-party worker policy (`C-12`) | `PROVIDER_POLICY_ELIGIBILITY.md` | Routing | MODEL-ROUTING | `ProviderPolicyEligibility` | S39 | SPECIFIED-PARTIAL (Q-V13-04) |
| 7 | Anthropic paid-marketplace checkout (`C-13-ANTHROPIC`) | `PLUGIN_DISTRIBUTION_ARCHITECTURE.md` | — | HOST-INTEGRATION | — | S46 | SPECIFIED-PARTIAL (Q-V13-09-ANTHROPIC) |

## Wave 3 — V1.3.1 source closure, axis separation, activation provenance

| § | Requirement | Architecture section | Runtime subsystem | BUILD-A2 owner | Contract / interface | Validation | Status |
|---|---|---|---|---|---|---|---|
| 1 | Source closure — reviewer + self-fetched primary sources | `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`, `evidence/SOURCE_CLAIM_REGISTRY.json` | — | BUILD-A1 | — | Review | SPECIFIED |
| 2 | Single source-claim registry, all statuses reconciled | `evidence/validate_sources.py` | — | BUILD-A1 | — | S36–S46 | SPECIFIED |
| 3 | Codex native plugin/hook path primary | `CODEX_PLUGIN_PACKAGING.md`, `CODEX_HOST_ADAPTER.md` | Host adapters | HOST-INTEGRATION | `HostCapabilityReport` | S36, S37 | SPECIFIED |
| 4 | Provider participation is conditional, not marketed | `FREE_PRO_PRODUCT_ARCHITECTURE.md` | Routing | MODEL-ROUTING | `ProviderPolicyEligibility` | S38, S39, S40 | SPECIFIED |
| 5 | Host-native vs external-worker contexts distinguished | `PROVIDER_POLICY_ELIGIBILITY.md` | Routing | MODEL-ROUTING | `ProviderPolicyEligibility` | S38, S40 | SPECIFIED |
| 6 | Activation provenance state model | `PRODUCT_ENTITLEMENT_ARCHITECTURE.md` | Entitlement | STATE-CONTEXT | `ActivationState` | S41, S42 | SPECIFIED |
| 7 | FeatureAdmissionDecision narrowed to the entitlement axis | `ENTITLEMENT_ADMISSION_PROTOCOL.md` | Admission | ORCHESTRATION | `FeatureAdmissionDecision` | S43 | SPECIFIED |
| 7 | DispatchAdmissionDecision composes all six axes | `ENTITLEMENT_ADMISSION_PROTOCOL.md` | Admission | ORCHESTRATION | `DispatchAdmissionDecision` | S44 | SPECIFIED |
| 8 | GraphEdge class exclusivity | `EXECUTION_GRAPH_MODEL.md` | Graph core | ORCHESTRATION | `GraphEdge` | S45 | SPECIFIED |
| 9 | Graph-engineering prior art reviewed | `FREE_PRO_PRODUCT_ARCHITECTURE.md` | — | BUILD-A1 | — | Review | SPECIFIED |
| 10 | OpenAI External Checkout, entitlement stays ours | `PLUGIN_DISTRIBUTION_ARCHITECTURE.md` | — | HOST-INTEGRATION | `ProductEntitlement` | S46 | SPECIFIED |
| 13 | Cross-artifact source consistency validator | `evidence/validate_sources.py` | — | BUILD-A1 | — | Review | SPECIFIED |

## Coverage summary

**Derived from every current requirement row on this page (all three waves).** This is the only current summary; `TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES = 0` is enforced.

| Status | Count |
|---|---:|
| `SPECIFIED` | 90 |
| `SPECIFIED-PARTIAL` | 8 |
| `DEFERRED` | 0 |
| `BLOCKED` | 0 |
| **TOTAL** | **98** |

The eight `SPECIFIED-PARTIAL` rows and the still-open question each depends on:

| Row | Open question | Why still partial |
|---|---|---|
| `START_GOAL` on both hosts | `Q-01` | Host command syntax not fixed |
| Product command surface | `Q-01` | Same question, same milestone |
| Security review pipeline | `Q-06` | Deterministic tool set is a per-project config decision |
| Safety interruption / policy blocked / `HUMAN_REQUIRED` | `Q-07` | Provider detectability signal may not exist; `UNKNOWN` + `HUMAN_REQUIRED` is the safe answer |
| Provider policy eligibility | `Q-V13-04` | OpenAI consumer external-worker path genuinely unresolved |
| Consumer-subscription third-party worker policy (`C-12`) | `Q-V13-04` | Same unresolved policy question |
| Open-core boundary | `Q-V13-06` | Pro module distribution mechanism untested |
| Anthropic paid-marketplace checkout (`C-13-ANTHROPIC`) | `Q-V13-09-ANTHROPIC` | No evidence of first-party paid checkout |

**Resolved this pass — four rows promoted to `SPECIFIED`.** Host capability discovery, Claude plugin packaging and Codex plugin packaging no longer name `Q-V13-01` / `Q-V13-02`, because `C-01`, `C-02`, `C-04` and `C-05` supply the current primary-source evidence those rows were waiting on. Provider policy eligibility narrowed from `Q-V13-03/04/05` to `Q-V13-04` alone: `Q-V13-03` is resolved as `VERIFIED_DISALLOWED` (a closed answer, not an open question) and `Q-V13-05` split into two resolved provider-specific rows. Only the OpenAI consumer external-worker question genuinely remains.

Nothing was deferred silently; `DEFERRED_CAPABILITIES.md` carries a revisit trigger for each deferred item.

## [HISTORICAL] V1.2 traceability snapshot

**[HISTORICAL]** The figures below describe the V1.2 package, when this matrix held 64 rows. They are preserved for audit continuity and are **superseded** by the current coverage summary above. They assert nothing about the current package.

| Status | Count (V1.2 only) |
|---|---:|
| `SPECIFIED` | 61 |
| `SPECIFIED-PARTIAL` | 3 |
| `DEFERRED` | 0 |
| `BLOCKED` | 0 |
| **TOTAL** | **64** |

**[HISTORICAL]** The three V1.2 partial rows were `START_GOAL` on both hosts, Security review pipeline, and Safety interruption — corresponding to `Q-01`, `Q-06` and `Q-07`.

**[HISTORICAL] V1.1 correction.** The V1.1 summary claimed *four* partial rows and included `Q-03`. `Q-03` was never marked partial in the matrix — it was a separate open question about the state store, not a traceability gap. `Q-03` was resolved in V1.2: `MVP_STATE_STORE = SQLite`.

**Why this snapshot was a defect at V1.3.2.** It was presented as *the* coverage summary while the matrix had already grown to 98 rows, so the page stated 64 rows and the package validation report stated 98 — a direct contradiction inside one candidate. The fix is not a corrected number; it is the separation of one current summary from one dated snapshot, with the snapshot explicitly marked so it contributes no current assertion.
