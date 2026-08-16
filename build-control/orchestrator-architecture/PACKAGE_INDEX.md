<!--
MultiAgent Orchestrator Architecture — V1.3.7 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_7_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validators: evidence/validate_sources.py and
evidence/validate_package.py (both exit non-zero on failure).
-->
<!-- FINAL_PACKAGE_GATES: {"ADMISSION_FIXTURE_FAILURES": 0, "AMBIGUOUS_REQUIRED_PATHS": 0, "BUILD_A2_COUNT_MISMATCH": 0, "BUILD_DAG_CYCLES": 0, "CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS": 0, "CONTRACT_OWNER_COLLISIONS": 0, "DISPLAYED_DERIVED_COUNT_MISMATCHES": 0, "EVENT_AUTHORITY_DUPLICATE_EVENTS": 0, "EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES": 0, "EVENT_AUTHORITY_SCHEMA_INVALID": 0, "EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES": 0, "FEATURE_ADMISSION_PROVIDER_OUTCOMES": 0, "FEATURE_ADMISSION_SAFETY_OUTCOMES": 0, "FINAL_REPORT_PACKAGE_GATE_MISMATCHES": 0, "GRAPH_SCHEMA_FAILURES": 0, "HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED": 0, "HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED": 0, "HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED": 0, "HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED": 0, "HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES": 0, "HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES": 0, "HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED": 0, "HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED": 0, "HOST_CAPABILITY_INVALID_STATE_ACCEPTED": 0, "HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED": 0, "HOST_CAPABILITY_VALID_STATE_REJECTED": 0, "INSTALLATION_MAP_AMBIGUOUS_PATHS": 0, "INSTALLATION_MAP_PATH_MISMATCHES": 0, "INSTALL_MANIFEST_UNMAPPED_PATHS": 0, "INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED": 0, "M0_SCHEMA_CLASSIFICATION_MISMATCHES": 0, "MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS": 0, "MANAGER_OWNED_CONTRACT_SET_MISMATCHES": 0, "MANIFEST_HASH_FAILURES": 0, "MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED": 0, "MODE_SELECTION_STATE_MISMATCHES": 0, "NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES": 0, "OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES": 0, "OPEN_QUESTION_STATUS_SOURCE_MISMATCHES": 0, "PARITY_CONFORMANCE_COVERAGE_MISSING": 0, "PARITY_DISPLAYED_COUNT_MISMATCHES": 0, "PATH_OWNER_COLLISIONS": 0, "PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED": 0, "PRECEDENCE_DAG_CYCLE_COUNT": 0, "REQUIRED_PATH_OWNER_MISMATCHES": 0, "SCHEMA_INVALID": 0, "SCHEMA_OWNER_MISMATCH": 0, "SELF_OWNED_EXTERNAL_CONSUMPTIONS": 0, "STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED": 0, "TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH": 0, "TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES": 0, "TRACEABILITY_STALE_RESOLVED_QUESTION_REFS": 0, "TRACEABILITY_STATUS_COUNT_MISMATCHES": 0, "UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED": 0, "UNOWNED_REQUIRED_PATHS": 0, "UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES": 0, "VALIDATION_DEPENDENCY_FAIL_OPEN": 0, "VENDOR_ENUM_VIOLATIONS": 0, "WAVE_ORDER_VIOLATIONS": 0, "ZIP_SAFETY_VIOLATIONS": 0} -->
FINAL_ZIP_VALIDATE_SOURCES_EXIT = 0
FINAL_ZIP_REGRESSION_EXIT = 0
FINAL_ZIP_VALIDATE_PACKAGE_EXIT = 0
FINAL_REPORT_PACKAGE_GATE_MISMATCHES = 0

# PACKAGE_INDEX

**Package:** `MultiAgent_Orchestrator_Architecture_V1_3_7_CANDIDATE`
**Revision:** V1.3.7 — HostCapabilityReport implication validation and fail-closed validator dependencies.
**Status:** `FREEZE_READY = PENDING_FINAL_INDEPENDENT_REVIEW`. Not installed, not frozen. `NEW_ARCHITECTURE_FREEZE_SHA` **NOT ASSIGNED**.

**This is not a redesign.** The product architecture accepted at V1.3.2 is carried forward unchanged. This revision corrects control documents and the tooling that checks them. See `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md`.

## Derived package facts

Every number below is measured from the **final ZIP**, after it was written and reopened — not from a staging directory. `evidence/build_package.py` renders this page from the same measurement pass that `evidence/validate_package.py` then checks it against.

| Fact | Key | Value |
|---|---|---:|
| ZIP entries (files + directories) | `ZIP_ENTRY_COUNT` | 224 |
| Files | `FILE_COUNT` | 214 |
| Directories | `DIRECTORY_COUNT` | 10 |
| Package manifest entries | `PACKAGE_MANIFEST_ENTRY_COUNT` | 213 |
| Install manifest entries | `INSTALL_MANIFEST_ENTRY_COUNT` | 212 |
| Schemas | `SCHEMA_COUNT` | 36 |
| Individual contracts / interfaces | `INDIVIDUAL_CONTRACTS` | 51 |
| Scenarios | `SCENARIO_COUNT` | 46 |
| Graph fixtures (positive) | `GRAPH_FIXTURE_COUNT` | 7 |
| Graph fixtures (negative) | `GRAPH_NEGATIVE_FIXTURE_COUNT` | 4 |
| Admission fixtures | `ADMISSION_FIXTURE_COUNT` | 7 |
| Source claims in registry | `SOURCE_CLAIM_COUNT` | 24 |
| Provider policy matrix rows | `POLICY_MATRIX_ROWS` | 8 |
| BUILD-A2 managers | `BUILD_A2_COUNT` | 7 |
| BUILD DAG nodes | `BUILD_DAG_NODES` | 7 |
| BUILD DAG edges | `BUILD_DAG_EDGES` | 10 |
| Traceability rows | `TRACE_ROWS` | 98 |
| Traceability SPECIFIED | `TRACE_SPECIFIED` | 90 |
| Traceability SPECIFIED-PARTIAL | `TRACE_PARTIAL` | 8 |
| Open-question rows / questions | `OPEN_QUESTION_ROWS` | 20 |
| Open questions represented | `OPEN_QUESTION_TOTAL` | 26 |
| CURRENT_NORMATIVE documents | `CURRENT_NORMATIVE_DOCS` | 110 |
| HISTORICAL_SNAPSHOT documents | `HISTORICAL_SNAPSHOT_DOCS` | 5 |

The document-authority count is measured over exactly the file set
`evidence/validate_sources.py` scans, so the two agree by construction *and* by
check: the validator independently reported
`CURRENT_NORMATIVE_DOCS_SCANNED = 110` on this same archive.

## Current source authority

`evidence/SOURCE_CLAIM_REGISTRY.json` — the single authority for volatile vendor claims.
`SOURCE_VERIFICATION_MATRIX_V1_3_6.md` — generated from it.
`evidence/validate_sources.py` — enforces agreement; **exits non-zero on any failure**.
`evidence/validate_package.py` — measures the package and checks every displayed number.
`evidence/run_regression.py` — proves both validators catch what earlier revisions missed.
`evidence/build_package.py` — the generation sequence that produced this archive.

**On the matrix filename.** The generated source matrix now travels under the
current revision name, `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`. It is regenerated
in this pass, and carrying an older revision's filename inside a V1.3.6 candidate
invites exactly the kind of stale-reference confusion this revision exists to
remove. Renaming was safe **only** because every reference was updated in the
same pass and a gate now enforces it: `CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES`.
The superseded filename is recorded in `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md` under
an explicit historical marker.

## Path ownership, M0 classification and parity — derived, not asserted

| Fact | Key | Value |
|---|---|---:|
| Required implementation paths | `REQUIRED_PATH_COUNT` | 35 |
| HostCapabilityReport fixtures (valid) | `HOST_CAPABILITY_FIXTURE_COUNT` | 8 |
| HostCapabilityReport fixtures (invalid) | `HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT` | 13 |
| M0 machine-schema count (actual, from `schemas/`) | `M0_SCHEMA_COUNT_ACTUAL` | 36 |
| M0 behavioural-contract count (actual) | `M0_BEHAVIOURAL_COUNT_ACTUAL` | 7 |
| Host parity capability rows | `PARITY_CAPABILITY_COUNT` | 25 |

`M0_SCHEMA_COUNT_MATCHES_ACTUAL` = YES; `M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL` = YES. `HostCapabilityReport` was reclassified from behavioural to machine-schema at V1.3.4 — it has a real file at `schemas/HostCapabilityReport.schema.json`, so classifying it otherwise contradicted the package's own contents.

## Current host posture

| Host | Primary | Fallback |
|---|---|---|
| Claude Code | **EMBEDDED** — native plugin + hooks (`C-04`, `C-05`) | supervised / hybrid |
| Codex | **EMBEDDED** — native plugin + hooks (`C-01`, `C-02`) | supervised / hybrid |

Supervised and hybrid are **compatibility fallbacks only**, selected by runtime capability discovery when hooks are unsupported, unconfigured, untrusted, disabled, excluded by admin policy, or insufficient in coverage. **A-14 is RETIRED as current architecture.**

Installing a plugin does not automatically trust its hooks. New or changed hook definitions/hashes are marked for review and skipped until trusted (`C-02a`). `PLUGIN_INSTALLED != HOOKS_ACTIVE`.

## Document authority

Every source-bearing document declares `DOCUMENT_AUTHORITY`. Historical snapshots preserve what was believed at the time and contribute **no** current evidence assertion.

## Read in this order

0. `FREEZE_READINESS_REPORT.md` — every gate with its computed value
1. `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md` — what this revision changed and what it did not
2. `SOURCE_VERIFICATION_MATRIX_V1_3_6.md` + `evidence/SOURCE_CLAIM_REGISTRY.json` — evidence authority
3. `PACKAGE_VALIDATION_REPORT.md` — derived counts and validator results
4. `EXECUTION_GRAPH_MODEL.md` · `GRAPH_EXECUTION_POLICIES.md` — the graph and the one-engine/two-policies decision
5. `FREE_PRO_PRODUCT_ARCHITECTURE.md` · `FEATURE_CAPABILITY_MATRIX.md` — the tiers
6. `PRODUCT_ENTITLEMENT_ARCHITECTURE.md` · `ENTITLEMENT_ADMISSION_PROTOCOL.md` — licensing and admission axes
7. `PROVIDER_POLICY_ELIGIBILITY.md` — the conservative routing gate
8. `HOST_ARCHITECTURE.md` · `CODEX_PLUGIN_PACKAGING.md` · `HOST_CAPABILITY_DISCOVERY.md` — host posture
9. `OPEN_QUESTIONS.md` · `REQUIREMENTS_TRACEABILITY_MATRIX.md` — what is answered and what is not
10. `REPOSITORY_LAYOUT_PROPOSAL.md` · `BUILD_A2_OWNERSHIP_MATRIX.md` — path ownership completeness
