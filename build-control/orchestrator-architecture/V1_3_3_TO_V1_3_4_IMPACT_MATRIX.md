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

# V1_3_3_TO_V1_3_4_IMPACT_MATRIX

**Correction type: control-document closure. No product architecture changed.**

This pass targets ten specific consistency defects named in the BUILD-A1 succession instruction. It does not reopen ExecutionGraph, FREE/PRO architecture, entitlement architecture, provider-policy architecture, Runtime-A1/A2/A3/A4, Model Intelligence, A3→A4 repair, SQLite, the open-core model, or the seven BUILD-A2 topology. **No new BUILD-A2.**

## What changed

| # | Defect at V1.3.3 | Change | Verified by |
|---|---|---|---|
| 1 | `CODEX_HOST_ADAPTER.md`'s architecture diagram showed the companion supervisor as the sole/primary path even though the same document's own prose already treated native hooks as verified. `NORMALIZED_HOST_EVENTS.md`'s Codex-source column still routed `HOST_SESSION_STARTED`/`_ENDING`/`CONTEXT_COMPACTED` through the supervisor even though `CODEX_HOST_ADAPTER.md`'s own event-derivation table already marked them `OBSERVED` via hooks. `HOST_ARCHITECTURE.md` still asserted "forcing a hook abstraction onto Codex would mean inventing a mechanism that does not exist." `SYSTEM_DIAGRAM.md`'s Codex box omitted plugin/hooks entirely. `MCP_POSITION.md` described MCP as bridging "the supervised Codex integration" without naming it a fallback. `SCENARIO_VALIDATION.md` S2 called the companion supervisor the emitter of the normal case; S21 said EMBEDDED is selected merely "if native hooks are discovered"; S37 used the stale `supports_hooks`/`plugin_hooks_trusted` field names. `HOST_PARITY_CONTRACT.md`'s asymmetry table listed Codex installation as "config + companion process" with no native-plugin row | All eight documents now show the native plugin + hooks path as primary, with supervised/hybrid explicitly labelled compatibility fallback; the stale "inventing a mechanism" sentence is removed; S21 states EMBEDDED's real coherence requirement instead of mere discovery; S37 and the parity table use canonical field/mechanism names | `evidence/HOST_POSTURE_AUTHORITY.json` + `CURRENT_HOST_POSTURE_MISMATCHES = 0` |
| 2 | `BUILD_A2_MANAGERS/BUILD_A2_HOST_INTEGRATION.md` instructed future implementers to build a companion supervisor, `codex exec` driving, and JSONL event derivation as the Codex **primary** host integration, and its owned-paths list still named `integrations/codex/**` | Owned subsystem and Expected-BUILD-A3 sections corrected: native plugin shell primary, hook trust/enablement/admin-policy handling, `HostCapabilityReport` probing, supervised/hybrid fallback named as fallback. Owned paths updated to `plugins/codex/**` (native) and `integrations/codex-fallback/**` (fallback) | Manual review; charter no longer contradicts `HOST_ARCHITECTURE.md` |
| 3 | `BUILD_A2_OWNERSHIP_MATRIX.md`'s ownership check only proved collision-freedom (`PATH_OWNER_COLLISIONS = 0`), never completeness. `plugins/codex/**`, `tests/parity/**` ownership, and every `src/pro/**` subtree were entirely absent from the table; `REPOSITORY_LAYOUT_PROPOSAL.md` listed Codex's plugin path ambiguously ("plugin package OR supervisor companion") and `src/pro/**` as one undifferentiated proprietary tree | `REPOSITORY_LAYOUT_PROPOSAL.md` gained a canonical "Required path authority" table; `BUILD_A2_OWNERSHIP_MATRIX.md` gained matching rows plus a "BUILD-A1-controlled directories" table for `architecture/**`/`build-control/**`. Each `src/pro/**` subtree resolves to the manager owning the corresponding public capability, never one blanket Pro owner. Found and fixed one genuine pre-existing gap: `src/core/dag/**` was named in the layout but had no owner row | `UNOWNED_REQUIRED_PATHS = 0`, `AMBIGUOUS_REQUIRED_PATHS = 0`, `REQUIRED_PATH_OWNER_MISMATCHES = 0` |
| 4 | `schemas/HostCapabilityReport.schema.json` had no conditional logic, so `selected_mode = EMBEDDED` with `hooks_trusted = false` validated cleanly — an impossible state | `allOf`/`if`/`then` conditionals added: `plugin_installed` implies `plugin_supported`; `EMBEDDED` requires the concrete capabilities the hook-dependent path needs (`hooks_trusted` true-or-null, never false); any non-`EMBEDDED` mode requires a real `inactive_reason`. Four positive and four negative fixtures added under `fixtures/host_capability/` and `fixtures/host_capability-negative/`, including the required regression case | `HOST_CAPABILITY_INVALID_STATE_ACCEPTED = 0`, `HOST_CAPABILITY_VALID_STATE_REJECTED = 0` |
| 5 | `HOST_CAPABILITY_DISCOVERY.md` used stale field names (`supports_plugin_manifest`, `supports_hooks`, `plugin_hooks_trusted`, `hooks_feature_enabled`) that disagreed with the schema's actual property names | Field list rewritten to the schema's exact vocabulary (`plugin_supported`, `plugin_installed`, `hooks_supported`, `hooks_configured`, `hooks_trusted`, `hooks_enabled`, `hooks_allowed_by_admin_policy`, `hook_coverage_class`, `selected_mode`, `inactive_reason`, `source_claim_id`) | `HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES = 0` |
| 6 | `IMPLEMENTATION_MILESTONES.md` classified `HostCapabilityReport` as a non-schema behavioural M0 contract while `schemas/HostCapabilityReport.schema.json` existed in the same package — the package contradicted its own document | Reclassified into the machine-schema set (35→36); behavioural set corrected (8→7). Both counts, and their agreement with the actual `schemas/` directory, are now derived, never hand-typed | `M0_SCHEMA_CLASSIFICATION_MISMATCHES = 0`, `M0_SCHEMA_COUNT_MATCHES_ACTUAL = YES`, `M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL = YES` |
| 7 | `HOST_PARITY_CONTRACT.md` defines P-01…P-25 (25 rows), but `IMPLEMENTATION_MILESTONES.md` still said "all 18 parity rows pass" and the conformance scaffold stopped at `p18_cross_host_resume.spec` | Both now derived from the capability table (`PARITY_CAPABILITY_COUNT = 25`); the scaffold names `p25_provider_status.spec`. `M4` now distinguishes the `S1`–`S19` north-star demo subset from the full-scenario release gate | `PARITY_DISPLAYED_COUNT_MISMATCHES = 0`, `PARITY_CONFORMANCE_COVERAGE_MISSING = 0` |
| 8 | `HOST_CAPABILITY_DISCOVERY.md` still described the §7 Codex re-verification as not having happened this pass, while the source registry has carried `C-01`/`C-02` as `VERIFIED_CURRENT_SELF_FETCHED` since V1.3.1 — and the source validator's `STALE_RESEARCH_UNAVAILABLE_ASSERTIONS` pattern only matched one word order for that claim, missing the reversed phrasing (subject named before the negated verb) | Stale sentence replaced with a pointer to the closure already stated elsewhere in the same document. Validator pattern extended to catch the reversed word order. A new regression fixture (`evidence/regression/F10_...md`) reproduces the missed phrasing and asserts it must FAIL | `STALE_RESEARCH_UNAVAILABLE_ASSERTIONS = 0`, `SOURCE_VALIDATOR_REGRESSION_FIXTURES = 10`, `SOURCE_VALIDATOR_FALSE_NEGATIVE_FIXTURES = 0` |
| 9 | No single structured authority for current host posture existed; each document's posture claim could drift independently of the others with nothing to check it against | `evidence/HOST_POSTURE_AUTHORITY.json` created: primary/fallback per host, with the documents it governs enumerated. `evidence/validate_sources.py` checks every CURRENT_NORMATIVE block in those documents for a direct posture contradiction or an unlabelled "companion" mention | `CURRENT_HOST_POSTURE_MISMATCHES = 0` |
| 10 | **[HISTORICAL]** The generated source matrix still carried the V1.3.3 filename inside a V1.3.4 candidate | Renamed to `SOURCE_VERIFICATION_MATRIX_V1_3_4.md`; every current-pointer reference updated in the same pass. `V1_3_2_TO_V1_3_3_IMPACT_MATRIX.md`'s own historical mentions of the V1.3.3 filename were left as-is (accurate at the time they describe) and given their own `[HISTORICAL]` row-level marker per the DEFECT E rule | `CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES = 0` |

## Contracts, schemas, topology — unchanged in substance

One schema (`HostCapabilityReport`) gained internal conditionals but no new required field and no owner change. No contract changed owner except the newly-added `src/core/dag/**` path row, which corrects an omission rather than moving ownership — `src/core/dag/**` was always described as ORCHESTRATION's in `REPOSITORY_LAYOUT_PROPOSAL.md`; it simply had no row in `BUILD_A2_OWNERSHIP_MATRIX.md` until now. No BUILD-A2 boundary moved. The BUILD DAG is numerically identical: 7 nodes, 10 edges, 0 cycles, 0 wave-order violations.

## Provider facts carried forward unchanged

Codex native plugin/hooks remains the current primary host path — this pass makes every document say so consistently, it does not establish it for the first time (that happened at V1.3.1/V1.3.3). Plugin install does not imply hook trust. Hooks can be disabled; `allow_managed_hooks_only` may exclude plugin hooks; specialized tool paths may bypass ordinary hook coverage; the shared core remains the entitlement and security authority. The Anthropic Free/Pro/Max third-party external-worker path remains `VERIFIED_DISALLOWED`. The OpenAI ChatGPT consumer third-party paid external-worker path remains `POLICY_NEEDS_REVIEW` and not routable. The OpenAI `USER_API` programmatic path remains the supported programmatic direction. No safety or provider-policy bypass was introduced.

## New files

| File | Why |
|---|---|
| `evidence/HOST_POSTURE_AUTHORITY.json` | Structured, single authority for current host posture |
| `fixtures/host_capability/*.json` (4) | Positive `HostCapabilityReport` fixtures |
| `fixtures/host_capability-negative/*.json` (4) | Negative fixtures, including the required `hooks_trusted=false` + `EMBEDDED` regression case |
| `evidence/regression/F10_stale_research_could_not_be_performed.md` | Reproduces the reversed-word-order stale-research phrasing; must FAIL |
| `V1_3_3_TO_V1_3_4_IMPACT_MATRIX.md` | This document |

**[HISTORICAL]** The V1.3.3-era file `SOURCE_VERIFICATION_MATRIX_V1_3_3.md` was renamed to the current `SOURCE_VERIFICATION_MATRIX_V1_3_4.md`. Net file change is therefore six additions and zero deletions.

## The finding that mattered most

`PATH_OWNER_COLLISIONS = 0` had been treated as proof the ownership matrix was correct. It only proves the matrix never contradicts itself — it is silent about a path the architecture requires that never got a row at all. `src/core/dag/**` had been missing since at least V1.3, undetected because nothing checked for absence, only disagreement. The same completeness gap existed for every path this pass adds: `plugins/codex/**`, `integrations/codex-fallback/**`, `tests/parity/**`, `tests/security/**`, and all three `src/pro/**` subtrees. The general lesson: a collision check and a completeness check are different properties, and a validator that only proves one must never be read as proving the other.

## FREEZE_READY

`PENDING_FINAL_INDEPENDENT_REVIEW`. Not `PENDING_FINAL_INDEPENDENT_REVIEW` because every gate is asserted — because every gate listed in `FREEZE_READINESS_REPORT.md` was computed from the final ZIP after it was written and reopened, and all of them are zero (or `YES`, where the gate is a match rather than a count).
