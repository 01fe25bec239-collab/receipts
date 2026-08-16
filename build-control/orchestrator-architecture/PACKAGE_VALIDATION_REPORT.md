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

# PACKAGE_VALIDATION_REPORT

**All values computed from the final ZIP.** This report and `PACKAGE_INDEX.md` are rendered from one measurement pass, and that pass is then re-run against the archive after it is written. `DISPLAYED_DERIVED_COUNT_MISMATCHES` is now **checked**, not asserted — `evidence/validate_package.py` reads these files back as text and compares every displayed integer to the value it measured.

## V1.3.7 validator closure

| Metric | Value | Gate |
|---|---:|---|
| `REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE` | YES | YES — **PASS** |
| `SCHEMA_VALIDATION_EXECUTED` | YES | YES — **PASS** |
| `MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED` | 0 | 0 — **PASS** |
| `VALIDATION_DEPENDENCY_FAIL_OPEN` | 0 | 0 — **PASS** |
| `FINAL_REPORT_PACKAGE_GATE_MISMATCHES` | 0 | 0 — **PASS** |

## Source validator — parser corrected, self-probed

| Metric | Value | Gate |
|---|---:|---|
| `CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE` | 0 | 0 — **PASS** |
| `SOURCE_MATRIX_STATUS_MISMATCHES` | 0 | 0 — **PASS** |
| `STALE_A14_CURRENT_ASSERTIONS` | 0 | 0 — **PASS** |
| `CONTRADICTORY_VENDOR_STATUS_ASSERTIONS` | 0 | 0 — **PASS** |
| `STALE_RESEARCH_UNAVAILABLE_ASSERTIONS` | 0 | 0 — **PASS** |
| `STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS` | 0 | 0 — **PASS** |
| `POLICY_NEEDS_REVIEW_ROUTABLE_PATHS` | 0 | 0 — **PASS** |
| `VERIFIED_DISALLOWED_ROUTABLE_PATHS` | 0 | 0 — **PASS** |
| `POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES` | 0 | 0 — **PASS** |
| `UNDECLARED_DOCUMENT_AUTHORITY` | 0 | 0 — **PASS** |
| `OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS` | 0 | 0 — **PASS** (new) |
| `CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES` | 0 | 0 — **PASS** (new) |
| `SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS` | 0 | 0 — **PASS** (new) |
| `CURRENT_HOST_POSTURE_MISMATCHES` | 0 | 0 — **PASS** (new V1.3.4) |
| `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` | 0 | 0 — **PASS** (new V1.3.6) |
| `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` | 0 | 0 — **PASS** (new V1.3.6) |
| Validator exit code | 0 | non-zero on any failure — **PASS** |

`CURRENT_HOST_POSTURE_MISMATCHES` checks `evidence/HOST_POSTURE_AUTHORITY.json` — a single structured authority for each host's primary posture and fallbacks — against every document that describes host posture: a direct contradiction (e.g. "Codex ... primary ... SUPERVISED"), or the fallback mechanism ("companion") appearing with no nearby signal that it is a fallback. `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` narrows that same authority to the specific stale phrasing this pass found — "supervisor-mediated"/"shallower" describing the ordinary path rather than the SUPERVISED fallback — and is a subset of `CURRENT_HOST_POSTURE_MISMATCHES`, never larger. `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` checks `NORMALIZED_HOST_EVENTS.md`'s event table row-by-row against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json`, so the normative event-provenance table is validated against one structured authority rather than trusted as free prose.

Documents scanned: `CURRENT_NORMATIVE_DOCS` = 110; `HISTORICAL_SNAPSHOT_DOCS` = 5 excluded by declaration. The validator independently reported `CURRENT_NORMATIVE_DOCS_SCANNED` = 110 over the same archive, so the displayed count and the validator count are equal rather than coincidentally similar.

### DEFECT D — the header-comment bypass

V1.3.2 skipped any paragraph block starting `<!--`. With no blank line after `-->`, the metadata comment and the live prose were one block, so the prose was never read. Reproduced against the V1.3.2 validator before repair: it reported `STALE_A14_CURRENT_ASSERTIONS = 0` and exited 0 on a document that restates the retired A-14 claim in plain prose.

The parser now removes comments *as comments* and scans everything else. Only an explicit `[HISTORICAL]` marker exempts a block.

### DEFECT E — block scope leaking across table rows

Found while verifying the DEFECT D fix, by reading what the corrected validator still failed to flag. `blocks()` grouped an entire markdown table into a single unit, so a `[HISTORICAL]` marker in **any one row** exempted every other row of that table. A row asserting a currently false claim passed because a sibling row mentioned history — DEFECT A's proximity exemption at a smaller scale. Reproduced against the V1.3.2 validator, which reported `STALE_A14_CURRENT_ASSERTIONS = 0` and exited 0.

Table rows and list items are now their own units. A row is exempt only if that row declares itself historical, which is why several rows in `V1_3_1_TO_V1_3_2_IMPACT_MATRIX.md` now carry their own marker. Fixture `F9` locks it.

## Validator regression suite

| Metric | Value | Gate |
|---|---:|---|
| `SOURCE_VALIDATOR_REGRESSION_FIXTURES` | 12 | — |
| `SOURCE_VALIDATOR_REGRESSION_PASSED` | 12 | all — **PASS** |
| `SOURCE_VALIDATOR_FALSE_NEGATIVE_FIXTURES` | 0 | 0 — **PASS** |
| `SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS` | 0 | 0 — **PASS** |
| Missing validation dependency regression | PASS | fail closed — **PASS** |
| `BUILD_FAILS_ON_PACKAGE_GATE_FAILURE` | YES | YES — **PASS** |

F1 reproduces the V1.3.1 DEFECT A case. F4 confirms a declared historical snapshot is exempt. F6 confirms an undeclared document is flagged. **F7 reproduces DEFECT D and must FAIL; F8 proves an accurate current statement directly after a metadata comment still PASSES; F9 reproduces DEFECT E and must FAIL.** **F10 (V1.3.4) proves the research-closure pattern is caught in "subject ... could not be performed" order, not only "could not perform ... subject" order — must FAIL.** **New at V1.3.6: `HOST_PARITY_CONTRACT.md` reproduces "Shallower; supervisor-mediated" presented as the ordinary Codex path — must FAIL against `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES`; its `CODEX_HOST_ADAPTER.md` counterpart proves the same wording correctly scoped to the SUPERVISED fallback still PASSES.** Each fixture asserts the named gate, not merely the exit code, so no fixture can pass for the wrong reason.

## Cross-document truth

| Metric | Value | Gate |
|---|---:|---|
| `OPEN_QUESTION_STATUS_SOURCE_MISMATCHES` | 0 | 0 — **PASS** |
| `OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES` | 0 | 0 — **PASS** |
| `TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH` | 0 | 0 — **PASS** |
| `TRACEABILITY_STATUS_COUNT_MISMATCHES` | 0 | 0 — **PASS** |
| `TRACEABILITY_STALE_RESOLVED_QUESTION_REFS` | 0 | 0 — **PASS** |
| `TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES` | 0 | 0 — **PASS** |
| `CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS` | 0 | 0 — **PASS** |

`OPEN_QUESTIONS.md` now carries `OPEN_QUESTION_ROWS` = 20 rows representing `OPEN_QUESTION_TOTAL` = 26 questions, of which `OQ_RESOLVED` = 8 are resolved against current registry evidence. `VERIFIED_DISALLOWED` is treated as a resolved answer, not an open question — the path stays disabled either way.

`REQUIREMENTS_TRACEABILITY_MATRIX.md` is one current matrix: `TRACE_ROWS` = 98 rows, `TRACE_SPECIFIED` = 90, `TRACE_PARTIAL` = 8, `TRACE_DEFERRED` = 0, `TRACE_BLOCKED` = 0. The V1.2 figures survive only as an explicitly marked historical snapshot. No current row names a question that `OPEN_QUESTIONS.md` reports as resolved.

## Manager ownership

| Metric | Value | Gate |
|---|---:|---|
| `MANAGER_OWNED_CONTRACT_SET_MISMATCHES` | 0 | 0 — **PASS** |
| `MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS` | 0 | 0 — **PASS** |
| `SELF_OWNED_EXTERNAL_CONSUMPTIONS` | 0 | 0 — **PASS** |
| `CONTRACT_OWNER_COLLISIONS` | 0 | 0 — **PASS** |
| `PATH_OWNER_COLLISIONS` | 0 | 0 — **PASS** |

Each charter's normative list was compared to the canonical ownership map **in both directions**. Ownership follows normative shape authority, never data-flow direction: `ModelObservation` is produced by REVIEW-INTEGRATION and owned by MODEL-ROUTING, and `CONTRACT_CONSUMPTION_GRAPH.md` now states that invariant instead of a producer-ownership rule its own data flow contradicts.

## Path ownership completeness (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `REQUIRED_PATH_COUNT` | 35 | — |
| `UNOWNED_REQUIRED_PATHS` | 0 | 0 — **PASS** |
| `AMBIGUOUS_REQUIRED_PATHS` | 0 | 0 — **PASS** |
| `REQUIRED_PATH_OWNER_MISMATCHES` | 0 | 0 — **PASS** |
| `UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES` | 0 | 0 — **PASS** |

`PATH_OWNER_COLLISIONS = 0` only proves the ownership matrix never disagrees with itself; it says nothing about a required path — `plugins/codex/**`, `src/pro/orchestration/**`, `tests/parity/**`, and others — that never received a row at all. `REPOSITORY_LAYOUT_PROPOSAL.md`'s "Required path authority" table is the canonical, machine-parsed source of what is required; `BUILD_A2_OWNERSHIP_MATRIX.md`'s "Source ownership" and "BUILD-A1-controlled directories" tables are the canonical resolution. Every `src/pro/**` subtree resolves to the same manager that owns the corresponding public capability, never to one undifferentiated Pro owner. This pass found and fixed one genuine pre-existing gap: `src/core/dag/**` was named in the repository layout but absent from the ownership matrix.

## M0 schema/behavioural classification (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `M0_SCHEMA_COUNT_ACTUAL` | 36 | — |
| `M0_BEHAVIOURAL_COUNT_ACTUAL` | 7 | — |
| `M0_SCHEMA_CLASSIFICATION_MISMATCHES` | 0 | 0 — **PASS** |
| `M0_SCHEMA_COUNT_MATCHES_ACTUAL` | YES | YES — **PASS** |
| `M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL` | YES | YES — **PASS** |

`IMPLEMENTATION_MILESTONES.md` classified `HostCapabilityReport` as behavioural at V1.3.3 while `schemas/HostCapabilityReport.schema.json` shipped in the same package — a direct contradiction between the milestone document and the package's own contents. It is now in the machine-schema set (36, was 35) and out of the behavioural set (7, was 8); both counts are derived from `schemas/` and the milestone document's own tables, never hand-typed.

## HostCapabilityReport schema coherence (V1.3.4; trust model, coverage gate and reason precedence new at V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
| `HOST_CAPABILITY_FIXTURE_COUNT` | 8 | — |
| `HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT` | 13 | — |
| `HOST_CAPABILITY_INVALID_STATE_ACCEPTED` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_VALID_STATE_REJECTED` | 0 | 0 — **PASS** |
| `PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED` | 0 | 0 — **PASS** |
| `HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED` | 0 | 0 — **PASS** |
| `HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED` | 0 | 0 — **PASS** |
| `HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED` | 0 | 0 — **PASS** |
| `STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED` | 0 | 0 — **PASS** |
| `UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED` | 0 | 0 — **PASS** |
| `HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED` | 0 | 0 — **PASS** |
| `MODE_SELECTION_STATE_MISMATCHES` | 0 | 0 — **PASS** |
| `HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES` | 0 | 0 — **PASS** |

`schemas/HostCapabilityReport.schema.json` now carries `allOf`/`if`/`then` conditionals: `plugin_installed` implies `plugin_supported`; `selected_mode = EMBEDDED` requires `plugin_installed`, `hooks_supported`, `hooks_configured`, `hooks_enabled`, `hooks_allowed_by_admin_policy`, `hook_trust_required` and `required_hook_coverage_satisfied` all present and coherent, `hooks_trusted` true-or-null (or exactly `true` when `hook_trust_required = true`, never `false`); any non-EMBEDDED mode requires a real `inactive_reason`. **New at V1.3.6:** `hook_trust_required: boolean` disambiguates `hooks_trusted = null` (explicit-trust host with unreported state, INVALID) from a genuine no-trust-model host (VALID). `required_hook_coverage_satisfied: boolean` is now independent of `hook_coverage_class`, so `EMBEDDED` gates on "our required events are covered," not "coverage isn't literally NONE." Eight precedence-ordered rules require each `inactive_reason` to match its own condition and to not mask a higher-precedence failure (plugin install → hooks support → hooks configured → trust → enabled → admin policy → coverage). `fixtures/host_capability-negative/01_embedded_with_untrusted_hooks.json` is the V1.3.4 required regression fixture: Codex, plugin installed, hooks configured, `hooks_trusted = false`, `selected_mode = EMBEDDED` — correctly rejected. Four new fixtures cover the V1.3.6 required cases: explicit-trust host reporting `null` under `EMBEDDED` (INVALID), `hooks_trusted = false` with `inactive_reason = HOOKS_DISABLED` (INVALID — trust takes precedence), insufficient required coverage under `EMBEDDED` (INVALID), and a no-trust-model host validating cleanly under `EMBEDDED` (VALID). `HOST_CAPABILITY_DISCOVERY.md`'s field vocabulary matches the schema's property names exactly, including the two new fields.

## Host event provenance (new, V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
| `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` | 0 | 0 — **PASS** |
| `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` | 0 | 0 — **PASS** |
| `EVENT_AUTHORITY_SCHEMA_INVALID` | 0 | 0 — **PASS** |
| `EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES` | 0 | 0 — **PASS** |
| `EVENT_AUTHORITY_DUPLICATE_EVENTS` | 0 | 0 — **PASS** |
| `EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES` | 0 | 0 — **PASS** |
| `NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES` | 0 | 0 — **PASS** |

`NORMALIZED_HOST_EVENTS.md`'s event table gained a `Source class` column (`HOST_HOOK` / `WORKER_DISPATCH` / `ELICITATION` / `CORE_DRIVEN`) distinguishing a PRIMARY EMBEDDED HOST SOURCE from a FALLBACK / EXTERNAL WORKER SOURCE, and is now checked row-by-row against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json` rather than trusted as free prose. `codex exec` JSONL remains the correct source for worker-lifecycle events (`TASK_*`, `TOOL_EXECUTED` on Codex) in every host posture — this was never a fallback signal, and the table now says so explicitly. `HOST_PARITY_CONTRACT.md` and `CODEX_HOST_ADAPTER.md`'s remaining "Shallower; supervisor-mediated" / "shallower than Claude Code's" phrasing — describing the ordinary in-session path rather than the SUPERVISED-only fallback — is corrected and now caught structurally by `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` if it recurs.

## Installation map completeness (new, V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
| `INSTALL_MANIFEST_UNMAPPED_PATHS` | 0 | 0 — **PASS** |
| `INSTALLATION_MAP_AMBIGUOUS_PATHS` | 0 | 0 — **PASS** |
| `INSTALLATION_MAP_PATH_MISMATCHES` | 0 | 0 — **PASS** |

`REPOSITORY_INSTALLATION_MAP.md`'s Mapping table omitted `evidence/**`, `evidence/regression/**`, and all five `fixtures/**` subtrees, even though `INSTALL_MANIFEST.sha256` lists installable files under every one of them. Seven rows and a resulting-layout diagram update close the gap. `evidence/validate_package.py` gained `check_installation_map()`, which parses the Mapping table into glob rules and checks every path actually present in `INSTALL_MANIFEST.sha256` against them — unmapped paths, ambiguous multi-rule matches, and a repository path that isn't exactly the install root plus the package path are all gates now, derived from the shipped manifest rather than asserted.

## Host parity (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `PARITY_CAPABILITY_COUNT` | 25 | — |
| `PARITY_DISPLAYED_COUNT_MISMATCHES` | 0 | 0 — **PASS** |
| `PARITY_CONFORMANCE_COVERAGE_MISSING` | 0 | 0 — **PASS** |

`HOST_PARITY_CONTRACT.md` defines P-01…P-25. `IMPLEMENTATION_MILESTONES.md` and the conformance-suite scaffold both said "18" and stopped at `p18`; both are now derived from the capability table (25), not typed. `M4` now distinguishes the `S1`–`S19` north-star demo subset from full architecture scenario validation, which covers all `SCENARIO_COUNT` = 46 scenarios.

## Schemas and contracts

| Metric | Value | Gate |
|---|---:|---|
| `SCHEMA_COUNT` | 36 | — |
| `SCHEMA_INVALID` | 0 | 0 — **PASS** |
| `SCHEMA_OWNER_MISMATCH` | 0 | 0 — **PASS** |
| `VENDOR_ENUM_VIOLATIONS` | 0 | 0 — **PASS** |
| `INDIVIDUAL_CONTRACTS` | 51 | — |
| `FEATURE_ADMISSION_PROVIDER_OUTCOMES` | 0 | 0 — **PASS** |
| `FEATURE_ADMISSION_SAFETY_OUTCOMES` | 0 | 0 — **PASS** |

## Graph and admission fixtures

| Metric | Value | Gate |
|---|---:|---|
| `GRAPH_FIXTURE_COUNT` | 7 | — |
| `GRAPH_SCHEMA_FAILURES` | 0 | 0 — **PASS** |
| `PRECEDENCE_DAG_CYCLE_COUNT` | 0 | 0 — **PASS** |
| `GRAPH_NEGATIVE_FIXTURE_COUNT` | 4 | — |
| `INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED` | 0 | 0 — **PASS** |
| `ADMISSION_FIXTURE_COUNT` | 7 | — |
| `ADMISSION_FIXTURE_FAILURES` | 0 | 0 — **PASS** |

## BUILD DAG — unchanged

| Metric | Value | Gate |
|---|---:|---|
| `BUILD_A2_COUNT` | 7 | 7 — **PASS** |
| `BUILD_A2_MANAGER_FILES` | 7 | 7 — **PASS** |
| `BUILD_DAG_NODES` | 7 | — |
| `BUILD_DAG_EDGES` | 10 | — |
| `BUILD_DAG_CYCLES` | 0 | 0 — **PASS** |
| `WAVE_ORDER_VIOLATIONS` | 0 | 0 — **PASS** |

```
STATE-CONTEXT → WORKSPACE-EXECUTION → RUNTIME-ADAPTERS → ORCHESTRATION → MODEL-ROUTING → REVIEW-INTEGRATION → HOST-INTEGRATION
```

## ZIP and manifests — measured from the final archive

| Metric | Value | V1.3.2 displayed | Gate |
|---|---:|---:|---|
| `ZIP_ENTRY_COUNT` | 224 | 182 (wrong) | matches archive — **PASS** |
| `FILE_COUNT` | 214 | 174 (wrong) | matches archive — **PASS** |
| `DIRECTORY_COUNT` | 10 | not displayed | matches archive — **PASS** |
| `PACKAGE_MANIFEST_ENTRY_COUNT` | 213 | 173 (wrong) | matches archive — **PASS** |
| `INSTALL_MANIFEST_ENTRY_COUNT` | 212 | 172 (wrong) | matches archive — **PASS** |
| `MANIFEST_HASH_FAILURES` | 0 | 0 — **PASS** |
| `ZIP_SAFETY_VIOLATIONS` | 0 | 0 — **PASS** |
| `DISPLAYED_DERIVED_COUNT_MISMATCHES` | 0 | 0 — **PASS** |

Every hash in both manifests was verified against the extracted archive, and the manifest membership was checked in both directions: the package manifest covers every file except itself, and the install manifest covers every file except itself and the package manifest.

## Scenarios and registry

| Metric | Value |
|---|---:|
| `SCENARIO_COUNT` | 46 |
| `SOURCE_CLAIM_COUNT` | 24 |
| `POLICY_MATRIX_ROWS` | 8 |
