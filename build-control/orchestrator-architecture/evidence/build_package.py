#!/usr/bin/env python3
"""Package generation sequence — V1.3.6.

V1.3.6 is a micro-patch over HostCapabilityReport's trust/coverage/
inactive-reason semantics, Codex event-provenance wording, and installation-
map completeness. It does not change product architecture.
See `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md`.

V1.3.2's counts were wrong because the numbers were produced at one moment and
the archive at another. The fix is not better numbers; it is an order of
operations in which the measured object and the shipped object are the same
object.

The sequence, executed in exactly this order:

   1. determine the complete final payload file set (reports and manifests included)
   2. all architecture documents already exist in the payload
   3. derive the payload counts from that final set
   4. render PACKAGE_INDEX.md
   5. render PACKAGE_VALIDATION_REPORT.md
   6. render FREEZE_READINESS_REPORT.md
   7. generate INSTALL_MANIFEST.sha256 over the installable payload
   8. generate PACKAGE_MANIFEST.sha256 over the full package payload
   9. create the ZIP
  10. OPEN THE FINAL ZIP and extract it
  11. derive every count AGAIN, from the extracted ZIP
  12. compare the ZIP-derived counts against every displayed count, and re-run
      both validators plus the regression suite against the extracted ZIP

Step 12 is the one that makes the rest trustworthy: a staging directory is
never accepted as evidence about the archive.

Usage:  build_package.py --root <payload dir> --out <zip path>
"""
import argparse, datetime, hashlib, json, os, shutil, subprocess, sys, tempfile, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
MANIFESTS = ('INSTALL_MANIFEST.sha256', 'PACKAGE_MANIFEST.sha256')
REPORTS = ('PACKAGE_INDEX.md', 'PACKAGE_VALIDATION_REPORT.md', 'FREEZE_READINESS_REPORT.md')

HEADER = """<!--
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
"""


# ------------------------------------------------------------------ helpers
def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def payload_files(root):
    out = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d != '__pycache__']
        for fn in filenames:
            if fn == '.DS_Store':
                continue
            out.append(os.path.relpath(os.path.join(dirpath, fn), root))
    return sorted(out)


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def final_gate_marker(g):
    return f"<!-- FINAL_PACKAGE_GATES: {json.dumps(g, sort_keys=True)} -->\n"


FINAL_OUTCOME = """FINAL_ZIP_VALIDATE_SOURCES_EXIT = 0
FINAL_ZIP_REGRESSION_EXIT = 0
FINAL_ZIP_VALIDATE_PACKAGE_EXIT = 0
FINAL_REPORT_PACKAGE_GATE_MISMATCHES = 0
"""


def derive(root, zpath=None, display_check=False, out=None):
    cmd = [sys.executable, os.path.join(HERE, 'validate_package.py'), '--root', root]
    if zpath:
        cmd += ['--zip', zpath]
    if not display_check:
        cmd += ['--no-display-check']
    tmp = out or tempfile.mktemp(suffix='.json')
    cmd += ['--json', tmp]
    r = sh(cmd)
    with open(tmp, encoding='utf-8') as f:
        data = json.load(f)
    data['stdout'], data['returncode'] = r.stdout + r.stderr, r.returncode
    return data


# ------------------------------------------------------------------ renderers
def render_index(d, v, g):
    return HEADER + final_gate_marker(g) + FINAL_OUTCOME + f"""
# PACKAGE_INDEX

**Package:** `MultiAgent_Orchestrator_Architecture_V1_3_7_CANDIDATE`
**Revision:** V1.3.7 — HostCapabilityReport implication validation and fail-closed validator dependencies.
**Status:** `FREEZE_READY = PENDING_FINAL_INDEPENDENT_REVIEW`. Not installed, not frozen. `NEW_ARCHITECTURE_FREEZE_SHA` **NOT ASSIGNED**.

**This is not a redesign.** The product architecture accepted at V1.3.2 is carried forward unchanged. This revision corrects control documents and the tooling that checks them. See `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md`.

## Derived package facts

Every number below is measured from the **final ZIP**, after it was written and reopened — not from a staging directory. `evidence/build_package.py` renders this page from the same measurement pass that `evidence/validate_package.py` then checks it against.

| Fact | Key | Value |
|---|---|---:|
| ZIP entries (files + directories) | `ZIP_ENTRY_COUNT` | {d['ZIP_ENTRY_COUNT']} |
| Files | `FILE_COUNT` | {d['FILE_COUNT']} |
| Directories | `DIRECTORY_COUNT` | {d['DIRECTORY_COUNT']} |
| Package manifest entries | `PACKAGE_MANIFEST_ENTRY_COUNT` | {d['PACKAGE_MANIFEST_ENTRY_COUNT']} |
| Install manifest entries | `INSTALL_MANIFEST_ENTRY_COUNT` | {d['INSTALL_MANIFEST_ENTRY_COUNT']} |
| Schemas | `SCHEMA_COUNT` | {d['SCHEMA_COUNT']} |
| Individual contracts / interfaces | `INDIVIDUAL_CONTRACTS` | {d['INDIVIDUAL_CONTRACTS']} |
| Scenarios | `SCENARIO_COUNT` | {d['SCENARIO_COUNT']} |
| Graph fixtures (positive) | `GRAPH_FIXTURE_COUNT` | {d['GRAPH_FIXTURE_COUNT']} |
| Graph fixtures (negative) | `GRAPH_NEGATIVE_FIXTURE_COUNT` | {d['GRAPH_NEGATIVE_FIXTURE_COUNT']} |
| Admission fixtures | `ADMISSION_FIXTURE_COUNT` | {d['ADMISSION_FIXTURE_COUNT']} |
| Source claims in registry | `SOURCE_CLAIM_COUNT` | {d['SOURCE_CLAIM_COUNT']} |
| Provider policy matrix rows | `POLICY_MATRIX_ROWS` | {d['POLICY_MATRIX_ROWS']} |
| BUILD-A2 managers | `BUILD_A2_COUNT` | {d['BUILD_A2_COUNT']} |
| BUILD DAG nodes | `BUILD_DAG_NODES` | {d['BUILD_DAG_NODES']} |
| BUILD DAG edges | `BUILD_DAG_EDGES` | {d['BUILD_DAG_EDGES']} |
| Traceability rows | `TRACE_ROWS` | {d['TRACE_ROWS']} |
| Traceability SPECIFIED | `TRACE_SPECIFIED` | {d['TRACE_SPECIFIED']} |
| Traceability SPECIFIED-PARTIAL | `TRACE_PARTIAL` | {d['TRACE_PARTIAL']} |
| Open-question rows / questions | `OPEN_QUESTION_ROWS` | {d['OPEN_QUESTION_ROWS']} |
| Open questions represented | `OPEN_QUESTION_TOTAL` | {d['OPEN_QUESTION_TOTAL']} |
| CURRENT_NORMATIVE documents | `CURRENT_NORMATIVE_DOCS` | {d['CURRENT_NORMATIVE_DOCS']} |
| HISTORICAL_SNAPSHOT documents | `HISTORICAL_SNAPSHOT_DOCS` | {d['HISTORICAL_SNAPSHOT_DOCS']} |

The document-authority count is measured over exactly the file set
`evidence/validate_sources.py` scans, so the two agree by construction *and* by
check: the validator independently reported
`CURRENT_NORMATIVE_DOCS_SCANNED = {v['current_docs']}` on this same archive.

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
| Required implementation paths | `REQUIRED_PATH_COUNT` | {d['REQUIRED_PATH_COUNT']} |
| HostCapabilityReport fixtures (valid) | `HOST_CAPABILITY_FIXTURE_COUNT` | {d['HOST_CAPABILITY_FIXTURE_COUNT']} |
| HostCapabilityReport fixtures (invalid) | `HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT` | {d['HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT']} |
| M0 machine-schema count (actual, from `schemas/`) | `M0_SCHEMA_COUNT_ACTUAL` | {d['M0_SCHEMA_COUNT_ACTUAL']} |
| M0 behavioural-contract count (actual) | `M0_BEHAVIOURAL_COUNT_ACTUAL` | {d['M0_BEHAVIOURAL_COUNT_ACTUAL']} |
| Host parity capability rows | `PARITY_CAPABILITY_COUNT` | {d['PARITY_CAPABILITY_COUNT']} |

`M0_SCHEMA_COUNT_MATCHES_ACTUAL` = {d['M0_SCHEMA_COUNT_MATCHES_ACTUAL']}; `M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL` = {d['M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL']}. `HostCapabilityReport` was reclassified from behavioural to machine-schema at V1.3.4 — it has a real file at `schemas/HostCapabilityReport.schema.json`, so classifying it otherwise contradicted the package's own contents.

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
"""


def render_validation(d, g, v, r):
    def row(k, gate='0 — **PASS**'):
        return f"| `{k}` | {g[k]} | {gate} |"
    return HEADER + final_gate_marker(g) + FINAL_OUTCOME + f"""
# PACKAGE_VALIDATION_REPORT

**All values computed from the final ZIP.** This report and `PACKAGE_INDEX.md` are rendered from one measurement pass, and that pass is then re-run against the archive after it is written. `DISPLAYED_DERIVED_COUNT_MISMATCHES` is now **checked**, not asserted — `evidence/validate_package.py` reads these files back as text and compares every displayed integer to the value it measured.

## V1.3.7 validator closure

| Metric | Value | Gate |
|---|---:|---|
| `REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE` | {d['REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE']} | YES — **PASS** |
| `SCHEMA_VALIDATION_EXECUTED` | {d['SCHEMA_VALIDATION_EXECUTED']} | YES — **PASS** |
{row('MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED')}
{row('VALIDATION_DEPENDENCY_FAIL_OPEN')}
{row('FINAL_REPORT_PACKAGE_GATE_MISMATCHES')}

## Source validator — parser corrected, self-probed

| Metric | Value | Gate |
|---|---:|---|
| `CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE` | {v['results']['CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE']} | 0 — **PASS** |
| `SOURCE_MATRIX_STATUS_MISMATCHES` | {v['results']['SOURCE_MATRIX_STATUS_MISMATCHES']} | 0 — **PASS** |
| `STALE_A14_CURRENT_ASSERTIONS` | {v['results']['STALE_A14_CURRENT_ASSERTIONS']} | 0 — **PASS** |
| `CONTRADICTORY_VENDOR_STATUS_ASSERTIONS` | {v['results']['CONTRADICTORY_VENDOR_STATUS_ASSERTIONS']} | 0 — **PASS** |
| `STALE_RESEARCH_UNAVAILABLE_ASSERTIONS` | {v['results']['STALE_RESEARCH_UNAVAILABLE_ASSERTIONS']} | 0 — **PASS** |
| `STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS` | {v['results']['STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS']} | 0 — **PASS** |
| `POLICY_NEEDS_REVIEW_ROUTABLE_PATHS` | {v['results']['POLICY_NEEDS_REVIEW_ROUTABLE_PATHS']} | 0 — **PASS** |
| `VERIFIED_DISALLOWED_ROUTABLE_PATHS` | {v['results']['VERIFIED_DISALLOWED_ROUTABLE_PATHS']} | 0 — **PASS** |
| `POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES` | {v['results']['POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES']} | 0 — **PASS** |
| `UNDECLARED_DOCUMENT_AUTHORITY` | {v['results']['UNDECLARED_DOCUMENT_AUTHORITY']} | 0 — **PASS** |
| `OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS` | {v['results']['OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS']} | 0 — **PASS** (new) |
| `CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES` | {v['results']['CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES']} | 0 — **PASS** (new) |
| `SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS` | {v['results']['SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS']} | 0 — **PASS** (new) |
| `CURRENT_HOST_POSTURE_MISMATCHES` | {v['results']['CURRENT_HOST_POSTURE_MISMATCHES']} | 0 — **PASS** (new V1.3.4) |
| `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` | {v['results']['CURRENT_HOST_EVENT_SOURCE_MISMATCHES']} | 0 — **PASS** (new V1.3.6) |
| `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` | {v['results']['CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES']} | 0 — **PASS** (new V1.3.6) |
| Validator exit code | {v['returncode']} | non-zero on any failure — **PASS** |

`CURRENT_HOST_POSTURE_MISMATCHES` checks `evidence/HOST_POSTURE_AUTHORITY.json` — a single structured authority for each host's primary posture and fallbacks — against every document that describes host posture: a direct contradiction (e.g. "Codex ... primary ... SUPERVISED"), or the fallback mechanism ("companion") appearing with no nearby signal that it is a fallback. `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` narrows that same authority to the specific stale phrasing this pass found — "supervisor-mediated"/"shallower" describing the ordinary path rather than the SUPERVISED fallback — and is a subset of `CURRENT_HOST_POSTURE_MISMATCHES`, never larger. `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` checks `NORMALIZED_HOST_EVENTS.md`'s event table row-by-row against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json`, so the normative event-provenance table is validated against one structured authority rather than trusted as free prose.

Documents scanned: `CURRENT_NORMATIVE_DOCS` = {d['CURRENT_NORMATIVE_DOCS']}; `HISTORICAL_SNAPSHOT_DOCS` = {d['HISTORICAL_SNAPSHOT_DOCS']} excluded by declaration. The validator independently reported `CURRENT_NORMATIVE_DOCS_SCANNED` = {v['current_docs']} over the same archive, so the displayed count and the validator count are equal rather than coincidentally similar.

### DEFECT D — the header-comment bypass

V1.3.2 skipped any paragraph block starting `<!--`. With no blank line after `-->`, the metadata comment and the live prose were one block, so the prose was never read. Reproduced against the V1.3.2 validator before repair: it reported `STALE_A14_CURRENT_ASSERTIONS = 0` and exited 0 on a document that restates the retired A-14 claim in plain prose.

The parser now removes comments *as comments* and scans everything else. Only an explicit `[HISTORICAL]` marker exempts a block.

### DEFECT E — block scope leaking across table rows

Found while verifying the DEFECT D fix, by reading what the corrected validator still failed to flag. `blocks()` grouped an entire markdown table into a single unit, so a `[HISTORICAL]` marker in **any one row** exempted every other row of that table. A row asserting a currently false claim passed because a sibling row mentioned history — DEFECT A's proximity exemption at a smaller scale. Reproduced against the V1.3.2 validator, which reported `STALE_A14_CURRENT_ASSERTIONS = 0` and exited 0.

Table rows and list items are now their own units. A row is exempt only if that row declares itself historical, which is why several rows in `V1_3_1_TO_V1_3_2_IMPACT_MATRIX.md` now carry their own marker. Fixture `F9` locks it.

## Validator regression suite

| Metric | Value | Gate |
|---|---:|---|
| `SOURCE_VALIDATOR_REGRESSION_FIXTURES` | {r['total']} | — |
| `SOURCE_VALIDATOR_REGRESSION_PASSED` | {r['passed']} | all — **PASS** |
| `SOURCE_VALIDATOR_FALSE_NEGATIVE_FIXTURES` | {r['false_negatives']} | 0 — **PASS** |
| `SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS` | {r['header_comment_bypass']} | 0 — **PASS** |
| Missing validation dependency regression | {'PASS' if r['validation_dependency_regression'] else 'FAIL'} | fail closed — **PASS** |
| `BUILD_FAILS_ON_PACKAGE_GATE_FAILURE` | {'YES' if r['build_fails_on_package_gate_failure'] else 'NO'} | YES — **PASS** |

F1 reproduces the V1.3.1 DEFECT A case. F4 confirms a declared historical snapshot is exempt. F6 confirms an undeclared document is flagged. **F7 reproduces DEFECT D and must FAIL; F8 proves an accurate current statement directly after a metadata comment still PASSES; F9 reproduces DEFECT E and must FAIL.** **F10 (V1.3.4) proves the research-closure pattern is caught in "subject ... could not be performed" order, not only "could not perform ... subject" order — must FAIL.** **New at V1.3.6: `HOST_PARITY_CONTRACT.md` reproduces "Shallower; supervisor-mediated" presented as the ordinary Codex path — must FAIL against `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES`; its `CODEX_HOST_ADAPTER.md` counterpart proves the same wording correctly scoped to the SUPERVISED fallback still PASSES.** Each fixture asserts the named gate, not merely the exit code, so no fixture can pass for the wrong reason.

## Cross-document truth

| Metric | Value | Gate |
|---|---:|---|
{row('OPEN_QUESTION_STATUS_SOURCE_MISMATCHES')}
{row('OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES')}
{row('TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH')}
{row('TRACEABILITY_STATUS_COUNT_MISMATCHES')}
{row('TRACEABILITY_STALE_RESOLVED_QUESTION_REFS')}
{row('TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES')}
{row('CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS')}

`OPEN_QUESTIONS.md` now carries `OPEN_QUESTION_ROWS` = {d['OPEN_QUESTION_ROWS']} rows representing `OPEN_QUESTION_TOTAL` = {d['OPEN_QUESTION_TOTAL']} questions, of which `OQ_RESOLVED` = {d['OQ_RESOLVED']} are resolved against current registry evidence. `VERIFIED_DISALLOWED` is treated as a resolved answer, not an open question — the path stays disabled either way.

`REQUIREMENTS_TRACEABILITY_MATRIX.md` is one current matrix: `TRACE_ROWS` = {d['TRACE_ROWS']} rows, `TRACE_SPECIFIED` = {d['TRACE_SPECIFIED']}, `TRACE_PARTIAL` = {d['TRACE_PARTIAL']}, `TRACE_DEFERRED` = {d['TRACE_DEFERRED']}, `TRACE_BLOCKED` = {d['TRACE_BLOCKED']}. The V1.2 figures survive only as an explicitly marked historical snapshot. No current row names a question that `OPEN_QUESTIONS.md` reports as resolved.

## Manager ownership

| Metric | Value | Gate |
|---|---:|---|
{row('MANAGER_OWNED_CONTRACT_SET_MISMATCHES')}
{row('MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS')}
{row('SELF_OWNED_EXTERNAL_CONSUMPTIONS')}
{row('CONTRACT_OWNER_COLLISIONS')}
{row('PATH_OWNER_COLLISIONS')}

Each charter's normative list was compared to the canonical ownership map **in both directions**. Ownership follows normative shape authority, never data-flow direction: `ModelObservation` is produced by REVIEW-INTEGRATION and owned by MODEL-ROUTING, and `CONTRACT_CONSUMPTION_GRAPH.md` now states that invariant instead of a producer-ownership rule its own data flow contradicts.

## Path ownership completeness (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `REQUIRED_PATH_COUNT` | {d['REQUIRED_PATH_COUNT']} | — |
{row('UNOWNED_REQUIRED_PATHS')}
{row('AMBIGUOUS_REQUIRED_PATHS')}
{row('REQUIRED_PATH_OWNER_MISMATCHES')}
{row('UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES')}

`PATH_OWNER_COLLISIONS = 0` only proves the ownership matrix never disagrees with itself; it says nothing about a required path — `plugins/codex/**`, `src/pro/orchestration/**`, `tests/parity/**`, and others — that never received a row at all. `REPOSITORY_LAYOUT_PROPOSAL.md`'s "Required path authority" table is the canonical, machine-parsed source of what is required; `BUILD_A2_OWNERSHIP_MATRIX.md`'s "Source ownership" and "BUILD-A1-controlled directories" tables are the canonical resolution. Every `src/pro/**` subtree resolves to the same manager that owns the corresponding public capability, never to one undifferentiated Pro owner. This pass found and fixed one genuine pre-existing gap: `src/core/dag/**` was named in the repository layout but absent from the ownership matrix.

## M0 schema/behavioural classification (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `M0_SCHEMA_COUNT_ACTUAL` | {d['M0_SCHEMA_COUNT_ACTUAL']} | — |
| `M0_BEHAVIOURAL_COUNT_ACTUAL` | {d['M0_BEHAVIOURAL_COUNT_ACTUAL']} | — |
{row('M0_SCHEMA_CLASSIFICATION_MISMATCHES')}
| `M0_SCHEMA_COUNT_MATCHES_ACTUAL` | {d['M0_SCHEMA_COUNT_MATCHES_ACTUAL']} | YES — **PASS** |
| `M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL` | {d['M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL']} | YES — **PASS** |

`IMPLEMENTATION_MILESTONES.md` classified `HostCapabilityReport` as behavioural at V1.3.3 while `schemas/HostCapabilityReport.schema.json` shipped in the same package — a direct contradiction between the milestone document and the package's own contents. It is now in the machine-schema set (36, was 35) and out of the behavioural set (7, was 8); both counts are derived from `schemas/` and the milestone document's own tables, never hand-typed.

## HostCapabilityReport schema coherence (V1.3.4; trust model, coverage gate and reason precedence new at V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
| `HOST_CAPABILITY_FIXTURE_COUNT` | {d['HOST_CAPABILITY_FIXTURE_COUNT']} | — |
| `HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT` | {d['HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT']} | — |
{row('HOST_CAPABILITY_INVALID_STATE_ACCEPTED')}
{row('HOST_CAPABILITY_VALID_STATE_REJECTED')}
{row('PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED')}
{row('HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED')}
{row('HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED')}
{row('HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED')}
{row('HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES')}
{row('HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED')}
{row('HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED')}
{row('HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED')}
{row('STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED')}
{row('UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED')}
{row('HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED')}
{row('MODE_SELECTION_STATE_MISMATCHES')}
{row('HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES')}

`schemas/HostCapabilityReport.schema.json` now carries `allOf`/`if`/`then` conditionals: `plugin_installed` implies `plugin_supported`; `selected_mode = EMBEDDED` requires `plugin_installed`, `hooks_supported`, `hooks_configured`, `hooks_enabled`, `hooks_allowed_by_admin_policy`, `hook_trust_required` and `required_hook_coverage_satisfied` all present and coherent, `hooks_trusted` true-or-null (or exactly `true` when `hook_trust_required = true`, never `false`); any non-EMBEDDED mode requires a real `inactive_reason`. **New at V1.3.6:** `hook_trust_required: boolean` disambiguates `hooks_trusted = null` (explicit-trust host with unreported state, INVALID) from a genuine no-trust-model host (VALID). `required_hook_coverage_satisfied: boolean` is now independent of `hook_coverage_class`, so `EMBEDDED` gates on "our required events are covered," not "coverage isn't literally NONE." Eight precedence-ordered rules require each `inactive_reason` to match its own condition and to not mask a higher-precedence failure (plugin install → hooks support → hooks configured → trust → enabled → admin policy → coverage). `fixtures/host_capability-negative/01_embedded_with_untrusted_hooks.json` is the V1.3.4 required regression fixture: Codex, plugin installed, hooks configured, `hooks_trusted = false`, `selected_mode = EMBEDDED` — correctly rejected. Four new fixtures cover the V1.3.6 required cases: explicit-trust host reporting `null` under `EMBEDDED` (INVALID), `hooks_trusted = false` with `inactive_reason = HOOKS_DISABLED` (INVALID — trust takes precedence), insufficient required coverage under `EMBEDDED` (INVALID), and a no-trust-model host validating cleanly under `EMBEDDED` (VALID). `HOST_CAPABILITY_DISCOVERY.md`'s field vocabulary matches the schema's property names exactly, including the two new fields.

## Host event provenance (new, V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
| `CURRENT_HOST_EVENT_SOURCE_MISMATCHES` | {v['results']['CURRENT_HOST_EVENT_SOURCE_MISMATCHES']} | 0 — **PASS** |
| `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` | {v['results']['CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES']} | 0 — **PASS** |
{row('EVENT_AUTHORITY_SCHEMA_INVALID')}
{row('EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES')}
{row('EVENT_AUTHORITY_DUPLICATE_EVENTS')}
{row('EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES')}
{row('NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES')}

`NORMALIZED_HOST_EVENTS.md`'s event table gained a `Source class` column (`HOST_HOOK` / `WORKER_DISPATCH` / `ELICITATION` / `CORE_DRIVEN`) distinguishing a PRIMARY EMBEDDED HOST SOURCE from a FALLBACK / EXTERNAL WORKER SOURCE, and is now checked row-by-row against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json` rather than trusted as free prose. `codex exec` JSONL remains the correct source for worker-lifecycle events (`TASK_*`, `TOOL_EXECUTED` on Codex) in every host posture — this was never a fallback signal, and the table now says so explicitly. `HOST_PARITY_CONTRACT.md` and `CODEX_HOST_ADAPTER.md`'s remaining "Shallower; supervisor-mediated" / "shallower than Claude Code's" phrasing — describing the ordinary in-session path rather than the SUPERVISED-only fallback — is corrected and now caught structurally by `CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES` if it recurs.

## Installation map completeness (new, V1.3.6)

| Metric | Value | Gate |
|---|---:|---|
{row('INSTALL_MANIFEST_UNMAPPED_PATHS')}
{row('INSTALLATION_MAP_AMBIGUOUS_PATHS')}
{row('INSTALLATION_MAP_PATH_MISMATCHES')}

`REPOSITORY_INSTALLATION_MAP.md`'s Mapping table omitted `evidence/**`, `evidence/regression/**`, and all five `fixtures/**` subtrees, even though `INSTALL_MANIFEST.sha256` lists installable files under every one of them. Seven rows and a resulting-layout diagram update close the gap. `evidence/validate_package.py` gained `check_installation_map()`, which parses the Mapping table into glob rules and checks every path actually present in `INSTALL_MANIFEST.sha256` against them — unmapped paths, ambiguous multi-rule matches, and a repository path that isn't exactly the install root plus the package path are all gates now, derived from the shipped manifest rather than asserted.

## Host parity (new, V1.3.4)

| Metric | Value | Gate |
|---|---:|---|
| `PARITY_CAPABILITY_COUNT` | {d['PARITY_CAPABILITY_COUNT']} | — |
{row('PARITY_DISPLAYED_COUNT_MISMATCHES')}
{row('PARITY_CONFORMANCE_COVERAGE_MISSING')}

`HOST_PARITY_CONTRACT.md` defines P-01…P-25. `IMPLEMENTATION_MILESTONES.md` and the conformance-suite scaffold both said "18" and stopped at `p18`; both are now derived from the capability table (25), not typed. `M4` now distinguishes the `S1`–`S19` north-star demo subset from full architecture scenario validation, which covers all `SCENARIO_COUNT` = {d['SCENARIO_COUNT']} scenarios.

## Schemas and contracts

| Metric | Value | Gate |
|---|---:|---|
| `SCHEMA_COUNT` | {d['SCHEMA_COUNT']} | — |
{row('SCHEMA_INVALID')}
{row('SCHEMA_OWNER_MISMATCH')}
{row('VENDOR_ENUM_VIOLATIONS')}
| `INDIVIDUAL_CONTRACTS` | {d['INDIVIDUAL_CONTRACTS']} | — |
{row('FEATURE_ADMISSION_PROVIDER_OUTCOMES')}
{row('FEATURE_ADMISSION_SAFETY_OUTCOMES')}

## Graph and admission fixtures

| Metric | Value | Gate |
|---|---:|---|
| `GRAPH_FIXTURE_COUNT` | {d['GRAPH_FIXTURE_COUNT']} | — |
{row('GRAPH_SCHEMA_FAILURES')}
{row('PRECEDENCE_DAG_CYCLE_COUNT')}
| `GRAPH_NEGATIVE_FIXTURE_COUNT` | {d['GRAPH_NEGATIVE_FIXTURE_COUNT']} | — |
{row('INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED')}
| `ADMISSION_FIXTURE_COUNT` | {d['ADMISSION_FIXTURE_COUNT']} | — |
{row('ADMISSION_FIXTURE_FAILURES')}

## BUILD DAG — unchanged

| Metric | Value | Gate |
|---|---:|---|
| `BUILD_A2_COUNT` | {d['BUILD_A2_COUNT']} | 7 — **PASS** |
| `BUILD_A2_MANAGER_FILES` | {d['BUILD_A2_MANAGER_FILES']} | 7 — **PASS** |
| `BUILD_DAG_NODES` | {d['BUILD_DAG_NODES']} | — |
| `BUILD_DAG_EDGES` | {d['BUILD_DAG_EDGES']} | — |
{row('BUILD_DAG_CYCLES')}
{row('WAVE_ORDER_VIOLATIONS')}

```
STATE-CONTEXT → WORKSPACE-EXECUTION → RUNTIME-ADAPTERS → ORCHESTRATION → MODEL-ROUTING → REVIEW-INTEGRATION → HOST-INTEGRATION
```

## ZIP and manifests — measured from the final archive

| Metric | Value | V1.3.2 displayed | Gate |
|---|---:|---:|---|
| `ZIP_ENTRY_COUNT` | {d['ZIP_ENTRY_COUNT']} | 182 (wrong) | matches archive — **PASS** |
| `FILE_COUNT` | {d['FILE_COUNT']} | 174 (wrong) | matches archive — **PASS** |
| `DIRECTORY_COUNT` | {d['DIRECTORY_COUNT']} | not displayed | matches archive — **PASS** |
| `PACKAGE_MANIFEST_ENTRY_COUNT` | {d['PACKAGE_MANIFEST_ENTRY_COUNT']} | 173 (wrong) | matches archive — **PASS** |
| `INSTALL_MANIFEST_ENTRY_COUNT` | {d['INSTALL_MANIFEST_ENTRY_COUNT']} | 172 (wrong) | matches archive — **PASS** |
{row('MANIFEST_HASH_FAILURES')}
{row('ZIP_SAFETY_VIOLATIONS')}
{row('DISPLAYED_DERIVED_COUNT_MISMATCHES')}

Every hash in both manifests was verified against the extracted archive, and the manifest membership was checked in both directions: the package manifest covers every file except itself, and the install manifest covers every file except itself and the package manifest.

## Scenarios and registry

| Metric | Value |
|---|---:|
| `SCENARIO_COUNT` | {d['SCENARIO_COUNT']} |
| `SOURCE_CLAIM_COUNT` | {d['SOURCE_CLAIM_COUNT']} |
| `POLICY_MATRIX_ROWS` | {d['POLICY_MATRIX_ROWS']} |
"""


def render_freeze(d, g, v, r, sha, ok):
    lines = []
    for k in ['CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE','SOURCE_MATRIX_STATUS_MISMATCHES',
              'STALE_A14_CURRENT_ASSERTIONS','CONTRADICTORY_VENDOR_STATUS_ASSERTIONS',
              'STALE_RESEARCH_UNAVAILABLE_ASSERTIONS','STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS',
              'UNDECLARED_DOCUMENT_AUTHORITY','OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS',
              'CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES','SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS',
              'POLICY_NEEDS_REVIEW_ROUTABLE_PATHS','VERIFIED_DISALLOWED_ROUTABLE_PATHS',
              'POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES','CURRENT_HOST_POSTURE_MISMATCHES',
              'CURRENT_HOST_EVENT_SOURCE_MISMATCHES','CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES']:
        lines.append(f"{k+':':<50}{v['results'][k]}")
    for k in ['BUILD_A2_COUNT_MISMATCH','BUILD_DAG_CYCLES','WAVE_ORDER_VIOLATIONS',
              'SCHEMA_INVALID','SCHEMA_OWNER_MISMATCH','VENDOR_ENUM_VIOLATIONS',
              'GRAPH_SCHEMA_FAILURES','PRECEDENCE_DAG_CYCLE_COUNT',
              'INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED','ADMISSION_FIXTURE_FAILURES',
              'FEATURE_ADMISSION_PROVIDER_OUTCOMES','FEATURE_ADMISSION_SAFETY_OUTCOMES',
              'CONTRACT_OWNER_COLLISIONS','PATH_OWNER_COLLISIONS',
              'MANAGER_OWNED_CONTRACT_SET_MISMATCHES','MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS',
              'SELF_OWNED_EXTERNAL_CONSUMPTIONS',
              'UNOWNED_REQUIRED_PATHS','AMBIGUOUS_REQUIRED_PATHS','REQUIRED_PATH_OWNER_MISMATCHES',
              'M0_SCHEMA_CLASSIFICATION_MISMATCHES','UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES',
              'HOST_CAPABILITY_INVALID_STATE_ACCEPTED','HOST_CAPABILITY_VALID_STATE_REJECTED',
              'PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED',
              'HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED',
              'HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED',
              'HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED',
              'HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES',
              'HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED',
              'HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED',
              'HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED',
              'STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED',
              'UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED',
              'HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED',
              'MODE_SELECTION_STATE_MISMATCHES','HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES',
              'EVENT_AUTHORITY_SCHEMA_INVALID','EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES',
              'EVENT_AUTHORITY_DUPLICATE_EVENTS','EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES',
              'NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES',
              'PARITY_DISPLAYED_COUNT_MISMATCHES','PARITY_CONFORMANCE_COVERAGE_MISSING',
              'INSTALL_MANIFEST_UNMAPPED_PATHS','INSTALLATION_MAP_AMBIGUOUS_PATHS',
              'INSTALLATION_MAP_PATH_MISMATCHES',
              'OPEN_QUESTION_STATUS_SOURCE_MISMATCHES',
              'OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES','TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH',
              'TRACEABILITY_STATUS_COUNT_MISMATCHES','TRACEABILITY_STALE_RESOLVED_QUESTION_REFS',
              'TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES',
              'CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS',
              'MANIFEST_HASH_FAILURES','ZIP_SAFETY_VIOLATIONS',
              'MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED','VALIDATION_DEPENDENCY_FAIL_OPEN',
              'FINAL_REPORT_PACKAGE_GATE_MISMATCHES','DISPLAYED_DERIVED_COUNT_MISMATCHES']:
        lines.append(f"{k+':':<50}{g[k]}")
    gates = '\n'.join(lines)
    yn = 'YES' if ok else 'NO'
    ready = 'PENDING_FINAL_INDEPENDENT_REVIEW' if ok else 'NO'
    return HEADER + final_gate_marker(g) + FINAL_OUTCOME + f"""
# FREEZE_READINESS_REPORT

Every value below was computed **from the final ZIP after it was written and reopened**. Nothing on this page was typed.

```
ARCHITECTURE_VERSION:                             V1.3.6 CANDIDATE
CORRECTION_TYPE:                                  HOSTCAPABILITYREPORT TRUST/COVERAGE/REASON PRECEDENCE / EVENT PROVENANCE / INSTALL MAP CLOSURE
PRODUCT_ARCHITECTURE_REDESIGNED:                  NO
NEW_BUILD_A2_CREATED:                             NO

CODEX_NATIVE_PLUGIN_PRIMARY:                      YES
CODEX_SUPERVISOR:                                 FALLBACK
A14_CURRENT_STATUS:                               RETIRED
CODEX_HOOK_TRUST_MODEL:                           SPECIFIED (definition/hash scoped)
PLUGIN_INSTALLED_IMPLIES_HOOKS_ACTIVE:            NO
ANTHROPIC_THIRD_PARTY_EXTERNAL_WORKER:            VERIFIED_DISALLOWED (not routable)
OPENAI_CONSUMER_THIRD_PARTY_EXTERNAL_WORKER:      POLICY_NEEDS_REVIEW (not routable)
OPENAI_USER_API_PROGRAMMATIC:                     VERIFIED_ALLOWED

SOURCE_VALIDATOR:                                 PASS
SOURCE_VALIDATOR_EXIT_STATUS_TEST:                PASS
SOURCE_VALIDATOR_REGRESSION_FIXTURES:             {r['total']}
SOURCE_VALIDATOR_REGRESSION_PASSED:               {r['passed']}
SOURCE_VALIDATOR_FALSE_NEGATIVE_FIXTURES:         {r['false_negatives']}
PACKAGE_VALIDATOR:                                PASS
REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE:       {d['REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE']}
SCHEMA_VALIDATION_EXECUTED:                        {d['SCHEMA_VALIDATION_EXECUTED']}

{gates}

BUILD_A2_COUNT:                                   {d['BUILD_A2_COUNT']}
BUILD_DAG_NODES:                                  {d['BUILD_DAG_NODES']}
BUILD_DAG_EDGES:                                  {d['BUILD_DAG_EDGES']}
SCHEMA_COUNT:                                     {d['SCHEMA_COUNT']}
INDIVIDUAL_CONTRACTS:                             {d['INDIVIDUAL_CONTRACTS']}
GRAPH_FIXTURE_COUNT:                              {d['GRAPH_FIXTURE_COUNT']}
GRAPH_NEGATIVE_FIXTURE_COUNT:                     {d['GRAPH_NEGATIVE_FIXTURE_COUNT']}
ADMISSION_FIXTURE_COUNT:                          {d['ADMISSION_FIXTURE_COUNT']}
SCENARIO_COUNT:                                   {d['SCENARIO_COUNT']}
SOURCE_CLAIM_COUNT:                               {d['SOURCE_CLAIM_COUNT']}
TRACE_ROWS:                                       {d['TRACE_ROWS']}
TRACE_SPECIFIED:                                  {d['TRACE_SPECIFIED']}
TRACE_PARTIAL:                                    {d['TRACE_PARTIAL']}
OPEN_QUESTION_TOTAL:                              {d['OPEN_QUESTION_TOTAL']}
OQ_RESOLVED:                                      {d['OQ_RESOLVED']}
CURRENT_NORMATIVE_DOCS:                           {d['CURRENT_NORMATIVE_DOCS']}
HISTORICAL_SNAPSHOT_DOCS:                         {d['HISTORICAL_SNAPSHOT_DOCS']}

REQUIRED_PATH_COUNT:                              {d['REQUIRED_PATH_COUNT']}
M0_SCHEMA_COUNT_ACTUAL:                           {d['M0_SCHEMA_COUNT_ACTUAL']}
M0_BEHAVIOURAL_COUNT_ACTUAL:                      {d['M0_BEHAVIOURAL_COUNT_ACTUAL']}
M0_SCHEMA_COUNT_MATCHES_ACTUAL:                   {d['M0_SCHEMA_COUNT_MATCHES_ACTUAL']}
M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL:     {d['M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL']}
HOST_CAPABILITY_FIXTURE_COUNT:                    {d['HOST_CAPABILITY_FIXTURE_COUNT']}
HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT:           {d['HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT']}
PARITY_CAPABILITY_COUNT:                          {d['PARITY_CAPABILITY_COUNT']}

ZIP_ENTRY_COUNT:                                  {d['ZIP_ENTRY_COUNT']}
FILE_COUNT:                                       {d['FILE_COUNT']}
DIRECTORY_COUNT:                                  {d['DIRECTORY_COUNT']}
PACKAGE_MANIFEST_ENTRY_COUNT:                     {d['PACKAGE_MANIFEST_ENTRY_COUNT']}
INSTALL_MANIFEST_ENTRY_COUNT:                     {d['INSTALL_MANIFEST_ENTRY_COUNT']}

FINAL_ZIP_ENTRY_COUNT_MATCH:                      {yn}
FINAL_ZIP_FILE_COUNT_MATCH:                       {yn}
FINAL_PACKAGE_MANIFEST_COUNT_MATCH:               {yn}
FINAL_INSTALL_MANIFEST_COUNT_MATCH:               {yn}
PACKAGE_MANIFEST_PASS:                            {yn}
INSTALL_MANIFEST_PASS:                            {yn}
ZIP_SAFETY_PASS:                                  {yn}
TRACEABILITY_CURRENT_ROW_COUNT_MATCH:             {yn}
TRACEABILITY_STATUS_COUNTS_MATCH:                 {yn}
DISPLAYED_CURRENT_NORMATIVE_DOC_COUNT:            {d['CURRENT_NORMATIVE_DOCS']}
VALIDATOR_CURRENT_NORMATIVE_DOC_COUNT:            {v['current_docs']}

IMPLEMENTATION:                                   NOT STARTED
BUILD_A2_INITIALIZATION:                          NOT AUTHORIZED
BUILD_A3_A4_INITIALIZATION:                       NOT AUTHORIZED
A1_RUNTIME:                                       NOT CREATED
BRANCHES_OR_WORKTREES_CREATED:                    NONE
COMMITTED_OR_PUSHED:                              NO
NEW_ARCHITECTURE_FREEZE_SHA:                      NOT ASSIGNED

FREEZE_READY:                                     {ready}
```

## What this pass fixed

Seven closure items, none of them architectural. `HostCapabilityReport` gained a host-neutral `hook_trust_required` field so `hooks_trusted = null` can no longer mean both "no trust model" and "explicit trust model, state unreported" — only the latter is now INVALID. A new `required_hook_coverage_satisfied` field separates "sufficient for what this orchestrator needs" from `hook_coverage_class`'s "coverage against the host's entire vendor surface," and gates `EMBEDDED` on the former. Eight precedence-ordered rules now require `inactive_reason` to match its own condition and never mask a higher-precedence one — closing the false-reporting case where `hooks_trusted = false` coexisted with `inactive_reason = HOOKS_DISABLED`. `HOST_PARITY_CONTRACT.md` and `CODEX_HOST_ADAPTER.md`'s last "Shallower; supervisor-mediated" framing of Codex's ordinary in-session path is corrected, and `NORMALIZED_HOST_EVENTS.md`'s event table is now validated row-by-row against a new canonical `evidence/HOST_EVENT_SOURCE_AUTHORITY.json` rather than trusted as free prose. `REPOSITORY_INSTALLATION_MAP.md` gained the seven rows it was missing for `evidence/**` and `fixtures/**`, checked against the manifest actually shipped. `V1_3_4_TO_V1_3_6_IMPACT_MATRIX.md` records each with its verifying gate.

## What did not change

ExecutionGraph · one graph core · Free/Pro as policies · FeatureAdmission/DispatchAdmission split · ActivationState · GraphEdge exclusivity · SQLite · seven BUILD-A2 topology · durable A1/A2 · fresh A3/A4 · Model Intelligence · independent A4 and repair · exact-SHA provenance · cross-host state · open core · no managed inference · provider policy classifications.

## What remains honestly open

`C-12` — consumer-subscription third-party external-worker policy — stays `POLICY_NEEDS_REVIEW` and **not routable by default** (`Q-V13-04`). `C-13-ANTHROPIC` stays `UNVERIFIED` (`Q-V13-09-ANTHROPIC`). Neither was promoted, and no other resolution in this pass was allowed to drag them along.

## Package identity

```
PACKAGE: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE.zip
SHA256:  computed at build time — printed by evidence/build_package.py
```

The archive digest is deliberately **not** printed inside the archive: a file cannot contain its own hash. `evidence/build_package.py` prints it as the last line of the run that produced the file, and `PACKAGE_MANIFEST.sha256` covers every file the archive contains.

## Next step

Final independent review. If accepted: install per `REPOSITORY_INSTALLATION_MAP.md` → commit → push → the resulting `main` SHA becomes `NEW_ARCHITECTURE_FREEZE_SHA`. **Not done in this pass.**
"""


# ------------------------------------------------------------------ sequence
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--root', default=os.path.dirname(HERE))
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    root, out = os.path.abspath(a.root), os.path.abspath(a.out)

    # 1. final payload file set — manifests included by construction
    files = [f for f in payload_files(root) if f not in MANIFESTS]
    final_files = sorted(files + list(MANIFESTS))
    print(f"[1] payload files (incl. manifests) = {len(final_files)}")

    # 2-3. derive counts from the final set
    stage = derive(root, display_check=False)
    # Manifests and generated-report markers are intentionally replaced below;
    # every other stage failure is a real package defect and stops the build.
    mutable_stage_gates = {'MANIFEST_HASH_FAILURES', 'FINAL_REPORT_PACKAGE_GATE_MISMATCHES'}
    stage_failures = [k for k, value in stage['gates'].items()
                      if value != 0 and k not in mutable_stage_gates]
    if stage_failures:
        print(f"BUILD FAILED BEFORE RENDER: required package gates failing: {stage_failures}")
        return 1
    d = dict(stage['derived'])
    dirs = set()
    for f in final_files:
        p = os.path.dirname(f)
        while p:
            dirs.add(p); p = os.path.dirname(p)
    d['FILE_COUNT'] = len(final_files)
    d['DIRECTORY_COUNT'] = len(dirs)
    d['ZIP_ENTRY_COUNT'] = len(final_files) + len(dirs)
    d['PACKAGE_MANIFEST_ENTRY_COUNT'] = len(final_files) - 1
    d['INSTALL_MANIFEST_ENTRY_COUNT'] = len(final_files) - 2
    print(f"[3] derived: files={d['FILE_COUNT']} dirs={d['DIRECTORY_COUNT']} "
          f"zip_entries={d['ZIP_ENTRY_COUNT']}")

    def run_validators():
        vjson, rjson = os.path.join(root, '.srcval.json'), os.path.join(root, '.regr.json')
        vr = sh([sys.executable, os.path.join(HERE, 'validate_sources.py')],
                env=dict(os.environ, SRCVAL_OUT=vjson), cwd=root)
        v = json.load(open(vjson)); v['returncode'] = vr.returncode
        rr = sh([sys.executable, os.path.join(HERE, 'run_regression.py')],
                env=dict(os.environ, REGRESSION_OUT=rjson), cwd=root)
        r = json.load(open(rjson))
        os.remove(vjson); os.remove(rjson)
        return v, r, vr.returncode, rr.returncode

    prev_sha = sha256(out) if os.path.exists(out) else '(first build — see build output)'
    # A successful final archive must have every package gate at zero. These
    # exact values are embedded in all three generated reports and checked
    # again after reopening the final ZIP.
    g = {key: 0 for key in stage['gates']}

    # 4-6. Render the reports, then re-validate the RENDERED payload and render
    # again. The reports are part of what the validators read, so a single pass
    # would display numbers measured over the previous build's report text.
    # Two passes reach a fixed point: pass 2 renders from a validator run whose
    # input already contains pass-1 report prose, which pass 2 reproduces
    # byte-for-byte because the renderer is deterministic. Step 12 proves the
    # fixed point held by re-running the validators from the finished ZIP.
    for pass_no in (1, 2):
        v, r, vcode, rcode = run_validators()
        if vcode != 0 or rcode != 0:
            print(f"BUILD FAILED BEFORE RENDER: validate_sources={vcode} regression={rcode}")
            return 1
        open(os.path.join(root, 'PACKAGE_INDEX.md'), 'w', encoding='utf-8').write(render_index(d, v, g))
        open(os.path.join(root, 'PACKAGE_VALIDATION_REPORT.md'), 'w', encoding='utf-8').write(
            render_validation(d, g, v, r))
        open(os.path.join(root, 'FREEZE_READINESS_REPORT.md'), 'w', encoding='utf-8').write(
            render_freeze(d, g, v, r, prev_sha, ok=True))
        print(f"[4-6] render pass {pass_no}: validate_sources exit={vcode} regression exit={rcode}")
    print("[4-6] reports rendered")

    # 7. INSTALL_MANIFEST — installable payload (excludes both manifests)
    inst = [f for f in final_files if f not in MANIFESTS]
    with open(os.path.join(root, 'INSTALL_MANIFEST.sha256'), 'w', encoding='utf-8') as f:
        for rel in inst:
            f.write(f"{sha256(os.path.join(root, rel))}  {rel}\n")
    # 8. PACKAGE_MANIFEST — full package payload (excludes only itself)
    pkg = [f for f in final_files if f != 'PACKAGE_MANIFEST.sha256']
    with open(os.path.join(root, 'PACKAGE_MANIFEST.sha256'), 'w', encoding='utf-8') as f:
        for rel in pkg:
            f.write(f"{sha256(os.path.join(root, rel))}  {rel}\n")
    print(f"[7-8] manifests: install={len(inst)} package={len(pkg)}")

    # 9. create the ZIP
    if os.path.exists(out):
        os.remove(out)
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
        for dpath in sorted(dirs):
            zi = zipfile.ZipInfo(dpath + '/')
            # directory entries need real mode bits, or extraction produces
            # unreadable directories on any tool that honours them
            zi.external_attr = (0o40755 << 16) | 0x10
            z.writestr(zi, b'')
        for rel in final_files:
            z.write(os.path.join(root, rel), rel)
    print(f"[9] wrote {out}")

    # 10. OPEN THE FINAL ZIP
    tmp = tempfile.mkdtemp()
    with zipfile.ZipFile(out) as z:
        bad = z.testzip()
        if bad is not None:
            print(f"!! CRC failure in {bad}", file=sys.stderr)
            return 1
        z.extractall(tmp)
    print("[10] final ZIP reopened and extracted")

    # 11-12. re-derive FROM THE ZIP and compare against every displayed count
    final = derive(tmp, zpath=out, display_check=True,
                   out=os.path.join(os.path.dirname(out), 'final_validation.json'))
    fv = sh([sys.executable, os.path.join(tmp, 'evidence', 'validate_sources.py')],
            env=dict(os.environ, SRCVAL_OUT=os.path.join(tmp, 'v.json')), cwd=tmp)
    fr = sh([sys.executable, os.path.join(tmp, 'evidence', 'run_regression.py')],
            env=dict(os.environ, REGRESSION_OUT=os.path.join(tmp, 'r.json')), cwd=tmp)
    print("[11] re-derived from the ZIP")

    fv_res = json.load(open(os.path.join(tmp, 'v.json')))
    fr_res = json.load(open(os.path.join(tmp, 'r.json')))
    # the validator numbers PRINTED in the reports must equal what the validator
    # reports when run from the shipped archive
    displayed_ok = (fv_res['results'] == v['results']
                    and fv_res['current_docs'] == v['current_docs']
                    and fr_res['total'] == r['total']
                    and fr_res['false_negatives'] == r['false_negatives'])
    print(f"[12] displayed validator values == from-ZIP validator values: "
          f"{'YES' if displayed_ok else 'NO'}")
    print(f"[12] DISPLAYED_CURRENT_NORMATIVE_DOC_COUNT={d['CURRENT_NORMATIVE_DOCS']} "
          f"VALIDATOR_CURRENT_NORMATIVE_DOC_COUNT={fv_res['current_docs']}")
    if d['CURRENT_NORMATIVE_DOCS'] != fv_res['current_docs']:
        displayed_ok = False

    outcome_text = FINAL_OUTCOME.strip()
    for rel in REPORTS:
        if outcome_text not in open(os.path.join(tmp, rel), encoding='utf-8').read():
            displayed_ok = False

    fd = final['derived']
    matches = {k: (d.get(k), fd.get(k)) for k in
               ('ZIP_ENTRY_COUNT', 'FILE_COUNT', 'DIRECTORY_COUNT',
                'PACKAGE_MANIFEST_ENTRY_COUNT', 'INSTALL_MANIFEST_ENTRY_COUNT')}
    ok = all(x == y for x, y in matches.values())
    for k, (x, y) in matches.items():
        print(f"[12] {k}: planned={x} zip={y} {'OK' if x == y else 'MISMATCH'}")

    failed = [k for k, val in final['gates'].items() if val != 0]
    print(f"[12] package gates failing: {failed or 'none'}")
    print(f"[12] validate_sources(from zip) exit={fv.returncode}  regression exit={fr.returncode}")
    print(f"[12] DISPLAYED_DERIVED_COUNT_MISMATCHES = {final['gates']['DISPLAYED_DERIVED_COUNT_MISMATCHES']}")
    for det in final['details'][:20]:
        print("     ", det)

    print(f"\nSHA256({os.path.basename(out)}) = {sha256(out)}")
    good = (ok and not failed and displayed_ok
            and final['returncode'] == 0 and fv.returncode == 0 and fr.returncode == 0)
    if good:
        try:
            import jsonschema
            jsonschema_version = getattr(jsonschema, '__version__', 'unknown')
        except Exception:
            jsonschema_version = 'unavailable'
        attestation = {
            'package_sha256': sha256(out),
            'validate_package_sha256': sha256(os.path.join(tmp, 'evidence', 'validate_package.py')),
            'validate_sources_sha256': sha256(os.path.join(tmp, 'evidence', 'validate_sources.py')),
            'run_regression_sha256': sha256(os.path.join(tmp, 'evidence', 'run_regression.py')),
            'build_package_sha256': sha256(os.path.join(tmp, 'evidence', 'build_package.py')),
            'validate_package_exit': final['returncode'],
            'validate_sources_exit': fv.returncode,
            'regression_exit': fr.returncode,
            'validated_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'validation_environment': {'python': sys.version, 'jsonschema': jsonschema_version},
            'scope': 'package self-consistency evidence; not cryptographic remote attestation',
        }
        with open(out + '.FINAL_VALIDATION_ATTESTATION.json', 'w', encoding='utf-8') as f:
            json.dump(attestation, f, indent=2, sort_keys=True)
    else:
        os.remove(out)
        sidecar = out + '.FINAL_VALIDATION_ATTESTATION.json'
        if os.path.exists(sidecar):
            os.remove(sidecar)
    shutil.rmtree(tmp, ignore_errors=True)
    print("BUILD OK" if good else "BUILD FAILED")
    return 0 if good else 1


if __name__ == '__main__':
    sys.exit(main())
