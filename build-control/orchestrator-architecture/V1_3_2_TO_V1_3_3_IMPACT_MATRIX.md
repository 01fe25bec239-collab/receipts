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

# V1_3_2_TO_V1_3_3_IMPACT_MATRIX

**Correction type: control-document closure. No product architecture changed.**

Independent review accepted the V1.3.2 product architecture. This pass touched only the documents that *describe* the package and the tooling that *checks* them. Every architectural decision listed in §10 of the correction instruction is carried forward byte-identical in substance: ExecutionGraph, one graph core, FREE/PRO policies, the capability catalog, `ProductEntitlement`, `ActivationState`, `FeatureAdmissionDecision`, `DispatchAdmissionDecision`, `ProviderPolicyEligibility`, `HostCapabilityReport`, SQLite, the open-core boundary, Runtime-A1/A2/A3/A4 semantics, A3→A4→repair, Model Intelligence, exact-SHA provenance, cross-host state, and the seven BUILD-A2 topology. **No new BUILD-A2.**

## What changed

| # | Defect at V1.3.2 | Change | Verified by |
|---|---|---|---|
| 1 | Archive counts wrong by exactly two in every dimension, while the reports asserted `DISPLAYED_DERIVED_COUNT_MISMATCHES = 0` "by construction" | The package-generation sequence now measures, renders, packs, **reopens the finished ZIP**, re-derives from it, and compares against every displayed number. `evidence/build_package.py` + `evidence/validate_package.py` | Both validators re-run from the extracted final ZIP |
| 2 | `CURRENT_NORMATIVE` document count displayed 104 while the package's own validator reported 106 | Both numbers now come from one measurement pass over the final archive. Nothing is hard-coded | `DISPLAYED_CURRENT_NORMATIVE_DOC_COUNT == VALIDATOR_CURRENT_NORMATIVE_DOC_COUNT` |
| 3 | **DEFECT D** — a metadata comment not followed by a blank line hid the live prose after `-->` from every source check | Parser rewritten: comments are removed as comments, then **all** remaining prose is scanned. Nothing is exempt for starting with `<!--` | Fixtures `F7` (must fail) and `F8` (must pass) + an in-process self-probe on every run |
| 4 | `OPEN_QUESTIONS.md` stale against the source registry; `VERIFIED_DISALLOWED` carried as unresolved | Eight questions resolved, two generic questions split per provider, summary derived from rows | `OPEN_QUESTION_STATUS_SOURCE_MISMATCHES = 0`, `OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES = 0` |
| 5 | Traceability matrix carried a V1.2 coverage summary (64 rows) beside a package-level count of 98 | One current matrix in three waves, one derived summary; the V1.2 figures moved to a `[HISTORICAL]` snapshot | `TRACEABILITY_*` gates |
| 6 | **[HISTORICAL]** Four documents strengthened `C-02a` into "every plugin update re-triggers review" | Reworded to what the source supports: new or changed hook definitions/hashes are marked for review and skipped until trusted | `OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS = 0` |
| 7 | `CONTRACT_CONSUMPTION_GRAPH` stated a generic producer-ownership rule that its own `ModelObservation` row contradicts | Generic rule replaced by the real invariant; producer/acceptor shorthands demoted to named patterns | `CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS = 0` |
| 8 | **[HISTORICAL]** Current generated source matrix still named `..._V1_3_1.md` inside a later candidate | Renamed to `SOURCE_VERIFICATION_MATRIX_V1_3_3.md` at that time; every reference updated in that pass. Superseded by a further rename to `SOURCE_VERIFICATION_MATRIX_V1_3_4.md` at V1.3.4 | `CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES = 0` |
| 9 | **DEFECT E**, found while verifying the DEFECT D fix: `blocks()` grouped a whole markdown table into one unit, so a `[HISTORICAL]` marker in any one row exempted every other row of that table | Table rows and list items are now their own units. A row is exempt only if that row declares itself historical | Fixture `F9` + `OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS = 0` |

## Contracts, schemas, topology — unchanged

No schema was added, removed or altered. No contract changed owner. No BUILD-A2 boundary moved. The BUILD DAG is numerically identical: 7 nodes, 10 edges, 0 cycles, 0 wave-order violations.

The only ownership *text* that changed is the rule sentence in `CONTRACT_CONSUMPTION_GRAPH.md`. `ModelObservation` remains owned by `BUILD-A2-MODEL-ROUTING` and `NormalizedHostEvent` remains owned by `BUILD-A2-HOST-INTEGRATION` — the correction was to the stated rule, never to the concrete owners. Changing an owner to rescue a bad sentence would have been the wrong repair.

## Provider facts carried forward unchanged

Codex native plugin/hooks remains the current primary host path. Plugin install does not imply hook trust. Hooks can be disabled; `allow_managed_hooks_only` may exclude plugin hooks; specialized tool paths may bypass ordinary hook coverage; the shared core remains the entitlement and security authority. The Anthropic Free/Pro/Max third-party external-worker path remains `VERIFIED_DISALLOWED`. The OpenAI ChatGPT consumer third-party paid external-worker path remains `POLICY_NEEDS_REVIEW` and not routable. The OpenAI `USER_API` programmatic path remains the supported programmatic direction. No safety or provider-policy bypass was introduced.

## New files

| File | Why |
|---|---|
| `evidence/build_package.py` | The generation sequence itself — measure, render, manifest, pack, reopen, re-derive, compare |
| `evidence/validate_package.py` | Structural and cross-document measurement pass; owns `DISPLAYED_DERIVED_COUNT_MISMATCHES` |
| `evidence/regression/F7_header_comment_without_blank_line.md` | Reproduces DEFECT D; must FAIL |
| `evidence/regression/F8_header_comment_valid_current.md` | Its counterpart; accurate prose after a comment must still PASS |
| `evidence/regression/F9_table_row_scope_leak.md` | Reproduces DEFECT E — one table row marked historical must not exempt its siblings; must FAIL |
| `V1_3_2_TO_V1_3_3_IMPACT_MATRIX.md` | This document |

**[HISTORICAL]** The V1.3.2-era file `SOURCE_VERIFICATION_MATRIX_V1_3_1.md` was renamed to the current `SOURCE_VERIFICATION_MATRIX_V1_3_3.md`. Net file change is therefore six additions and zero deletions.

## The finding that mattered most

DEFECT D is the same class of failure as V1.3.1's DEFECT A: a parser that decides what *not* to read, using a heuristic that looks reasonable in isolation. V1.3.2 replaced proximity inference with explicit declaration and was right to. But it kept one silent skip — "a paragraph beginning `<!--` is a header" — and that skip swallowed real prose whenever an author omitted one blank line.

**[HISTORICAL]** Reproduced before repairing, as at V1.3.2: fixture `F7` was run against the **V1.3.2** validator, which reported `STALE_A14_CURRENT_ASSERTIONS = 0` and exited 0 on a document that plainly restates the retired A-14 claim — the exact sentence `C-03` records as obsolete. The V1.3.3 parser catches it, and `F8` proves the fix did not degenerate into flagging everything that follows a comment.

DEFECT E was found by running the corrected validator over the package and reading what it *did not* flag — a line quoting the very overstatement §6 exists to remove passed cleanly, because a neighbouring row in the same table mentioned `[HISTORICAL]`. Two exemption bypasses in one revision, both of the same family, is the strongest available argument that exemption must be declared at the smallest scope that can carry a statement.

The general lesson, now written into both validators: **a checker must never decide that text is uninteresting because of where it sits.** Authority is declared, never inferred from position, punctuation, or whitespace.
