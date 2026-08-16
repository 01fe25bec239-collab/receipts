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

# V1_3_4_TO_V1_3_6_IMPACT_MATRIX

**Correction type: control-document micro-patch. No product architecture changed.**

This pass targets seven specific consistency defects named in the BUILD-A1 succession instruction, all of them in `HostCapabilityReport`'s trust/coverage/inactive-reason semantics, Codex event-provenance wording, and the installation map's completeness. It does not reopen ExecutionGraph, FREE/PRO architecture, `ProductEntitlement`, `FeatureAdmissionDecision`/`DispatchAdmissionDecision`, `ProviderPolicyEligibility`, Model Intelligence, Runtime-A1/A2/A3/A4, A3→A4 repair, SQLite, the open-core boundary, provider-policy classifications, or the seven BUILD-A2 topology. **No new BUILD-A2.**

## What changed

| # | Defect at V1.3.4 | Change | Verified by |
|---|---|---|---|
| 1 | `hooks_trusted = null` was overloaded: it meant both "Codex has an explicit trust model but hasn't reported a concrete state" (invalid) and "the host has no trust model at all" (valid), with nothing in the schema able to tell the two apart | New host-neutral `hook_trust_required: boolean` field. `hook_trust_required = true` now requires `hooks_trusted` to be a concrete boolean — `null` is INVALID. `hook_trust_required = false` still permits `hooks_trusted = null` legitimately. `EMBEDDED` under an explicit trust model additionally requires `hooks_trusted = true`, not merely non-`false` | `HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED = 0` |
| 2 | `EMBEDDED` accepted `hook_coverage_class = PARTIAL` with no way to say whether that partial coverage was *sufficient* for this orchestrator's own required lifecycle events, as opposed to merely partial against the host's entire vendor hook surface | New `required_hook_coverage_satisfied: boolean` field, independent of `hook_coverage_class`. `EMBEDDED` now requires `required_hook_coverage_satisfied = true`; a host can still be `PARTIAL` against its full vendor surface while satisfying every event this orchestrator needs | `HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED = 0` |
| 3 | `inactive_reason` could contradict the underlying booleans it claims to summarize (e.g. `hooks_trusted = false`, `hooks_enabled = true`, `inactive_reason = HOOKS_DISABLED`) with nothing rejecting the mismatch, and no rule said which reason wins when more than one condition fails at once | Eight `allOf`/`if`/`then` rules added, one per `inactive_reason` value, each requiring both the reason's own condition and that every higher-precedence condition (plugin install → hooks support → hooks configured → trust → enabled → admin policy → coverage) is NOT also failing. A reason can no longer mask a higher-precedence failure | `HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED = 0` |
| 4 | `HOST_PARITY_CONTRACT.md` still described Codex's in-session integration as "Shallower; supervisor-mediated" as the ordinary path, contradicting the same package's own `HOST_POSTURE_AUTHORITY.json` (native plugin/hooks primary). `CODEX_HOST_ADAPTER.md` repeated the same framing and separately said the fallback "companion process" is what makes cross-host state work, when the shared core owns durable state in every posture | Both statements corrected: in-session integration is deep on both hosts when native hooks are trusted; SUPERVISED-only fallback is explicitly scoped as such. `CODEX_HOST_ADAPTER.md`'s cross-host-state section now states the shared core/state layer as the owner in all postures, with the companion process only accessing it in the SUPERVISED/HYBRID fallback | `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES = 0` |
| 5 | `NORMALIZED_HOST_EVENTS.md`'s event table had no way to distinguish a primary embedded host-hook source from a fallback/external-worker source, so `TOOL_EXECUTED`'s Codex `codex exec` JSONL source read as if it might be a fallback signal rather than the correct primary source for worker-lifecycle events | Table gained a `Source class` column (`HOST_HOOK` / `WORKER_DISPATCH` / `ELICITATION` / `CORE_DRIVEN`) and a legend distinguishing PRIMARY EMBEDDED HOST SOURCE from FALLBACK / EXTERNAL WORKER SOURCE. `codex exec` JSONL for worker events is confirmed correct regardless of host posture — not a sign of fallback. The table is now cross-checked, row by row, against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json` rather than trusted as free prose | `CURRENT_HOST_EVENT_SOURCE_MISMATCHES = 0` |
| 6 | `REPOSITORY_INSTALLATION_MAP.md`'s Mapping table and resulting-layout diagram omitted `evidence/**`, `evidence/regression/**`, and all five `fixtures/**` subtrees, even though `INSTALL_MANIFEST.sha256` lists installable files under every one of those paths | Seven rows added to the Mapping table (`evidence/**`, `evidence/regression/**`, `fixtures/admission/**`, `fixtures/graphs/**`, `fixtures/graphs-negative/**`, `fixtures/host_capability/**`, `fixtures/host_capability-negative/**`); resulting-layout diagram updated to show them. `evidence/validate_package.py` gained `check_installation_map()`, which parses the Mapping table and checks it against the manifest actually shipped, not a hand-maintained belief about what the manifest contains | `INSTALL_MANIFEST_UNMAPPED_PATHS = 0`, `INSTALLATION_MAP_AMBIGUOUS_PATHS = 0`, `INSTALLATION_MAP_PATH_MISMATCHES = 0` |
| 7 | Regression coverage did not include the four required `HostCapabilityReport` fixtures from the succession instruction (explicit-trust-required host reporting `null`; false-reporting `inactive_reason`; insufficient coverage under `EMBEDDED`; a no-trust-model host validating `EMBEDDED`), nor a case proving "Codex ... supervisor-mediated" as the ordinary path cannot pass the source validator | Four new `HostCapabilityReport` fixtures added (`05_trust_model_ambiguity_codex_null.json`, `06_inactive_reason_hooks_disabled_mismatch.json`, `07_insufficient_coverage_embedded.json` negative; `05_host_no_trust_model_embedded_valid.json` positive). Two new source-validator regression fixtures added, named to match `HOST_POSTURE_AUTHORITY.json`'s `scanned_documents` (`HOST_PARITY_CONTRACT.md` must FAIL, `CODEX_HOST_ADAPTER.md` counterpart must PASS) | `HOST_CAPABILITY_FIXTURE_COUNT = 5`, `HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT = 7`, `SOURCE_VALIDATOR_REGRESSION_FIXTURES = 12` |

## Contracts, schemas, topology — unchanged in substance

`HostCapabilityReport` gained two required fields (`hook_trust_required`, `required_hook_coverage_satisfied`) and more internal conditionals, but no owner change and no other schema touched. No contract changed owner. The BUILD DAG is numerically identical: 7 nodes, 10 edges, 0 cycles, 0 wave-order violations.

## Provider facts carried forward unchanged

Codex native plugin/hooks remains the current primary host path — this pass corrects the last two documents that still described the fallback as the ordinary path; it does not change the posture itself (established V1.3.1, made document-consistent V1.3.4). Plugin install does not imply hook trust. The Anthropic Free/Pro/Max third-party external-worker path remains `VERIFIED_DISALLOWED`. The OpenAI ChatGPT consumer third-party paid external-worker path remains `POLICY_NEEDS_REVIEW` and not routable. The OpenAI `USER_API` programmatic path remains the supported programmatic direction. No safety or provider-policy bypass was introduced.

## New files

| File | Why |
|---|---|
| `evidence/HOST_EVENT_SOURCE_AUTHORITY.json` | Structured, single authority for event → source-class → host-source provenance |
| `fixtures/host_capability/05_host_no_trust_model_embedded_valid.json` | Required regression fixture D: no-trust-model host, `EMBEDDED`, VALID |
| `fixtures/host_capability-negative/05_trust_model_ambiguity_codex_null.json` | Required regression fixture A: explicit-trust-required host, `hooks_trusted = null`, `EMBEDDED` — INVALID |
| `fixtures/host_capability-negative/06_inactive_reason_hooks_disabled_mismatch.json` | Required regression fixture B: `hooks_trusted = false`, `hooks_enabled = true`, `inactive_reason = HOOKS_DISABLED` — INVALID |
| `fixtures/host_capability-negative/07_insufficient_coverage_embedded.json` | Required regression fixture C: required coverage unsatisfied, `EMBEDDED` — INVALID |
| `evidence/regression/HOST_PARITY_CONTRACT.md` | "Shallower; supervisor-mediated" as the ordinary path — must FAIL |
| `evidence/regression/CODEX_HOST_ADAPTER.md` | Same wording correctly scoped to the SUPERVISED fallback — must PASS |
| `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md` | This document |

**[HISTORICAL]** The V1.3.4-era file `SOURCE_VERIFICATION_MATRIX_V1_3_4.md` was renamed to the current `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`; its content is unchanged (no new source claims were added this pass), only the revision stamp and filename moved. Net file change is therefore nine additions and zero deletions.

## The finding that mattered most

`hooks_trusted = null` looked like one condition (`no trust model`) but was actually two: `no trust model` and `trust model exists, state unreported`. A schema field with two meanings and one representation is a defect whether or not any current fixture happens to exercise the ambiguous case — the fix had to add a field that says *which* meaning applies, not merely tighten validation around the existing one. The same shape recurred in `hook_coverage_class`: `PARTIAL` conflated "partial against the vendor's full surface" with "insufficient for what we actually need," and only the second one should ever gate `EMBEDDED`.

## FREEZE_READY

`PENDING_FINAL_INDEPENDENT_REVIEW`. Every gate listed in `FREEZE_READINESS_REPORT.md` was computed from the final ZIP after it was written and reopened, and all of them are zero (or `YES`, where the gate is a match rather than a count). This package is a **candidate**, not final.
