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

# V1_3_1_TO_V1_3_2_IMPACT_MATRIX

Source-of-truth regeneration and validator hardening. **No architecture redesign.**

## Supersession chain

```
V1.2.3 → V1.3 → V1.3.1 → V1.3.2 (current)
   │        │        │         └── source registry propagated; validator hardened
   │        │        └── §7 research closed; Codex verified; admission axes split
   │        └── graph promotion; Free/Pro; entitlement
   └── seven BUILD-A2 topology; acyclic build DAG
```

| Item | Status | Detail |
|---|---|---|
| **`evidence/validate_sources.py`** | **REWRITTEN** | V1.3.1's validator was **unsound**. Three defects reproduced and fixed: proximity exemption (A), whole-document exemption (B), unconditional `sys.exit(0)` (C) |
| **Validator regression suite** | **NEW** | `evidence/run_regression.py`, 6 fixtures, 0 false negatives. F1 is the exact case V1.3.1 passed |
| **`DOCUMENT_AUTHORITY`** | **NEW** | Every document declares `CURRENT_NORMATIVE` or `HISTORICAL_SNAPSHOT`. Blocks opt into historical status with an explicit `[HISTORICAL]` marker |
| `HOST_ARCHITECTURE.md` | **REVISED** | **[HISTORICAL]** "No hook system exists (A-14)" removed. Codex EMBEDDED is primary; supervised/hybrid is fallback |
| `CODEX_HOST_ADAPTER.md` | **REVISED** | **[HISTORICAL]** Supervisor-primary framing, "none of these deliver lifecycle events", "re-verification not possible", and "undocumented endpoint" all removed |
| `CONTEXT_REHYDRATION_ARCHITECTURE.md` | **REVISED** | Compaction confidence now depends on `HostCapabilityReport`, not vendor name |
| `CONFLICT_RESOLUTION_LOG.md` | **REVISED** | Current resolution is native-plugin for both hosts; old resolution retained as tagged history |
| `ARCHITECTURE_DECISION_RECORDS_V1_3.md` | **HISTORICAL_SNAPSHOT** | Option A taken. Current ADRs in `ARCHITECTURE_DECISION_RECORDS_V1_3_2.md` |
| `ARCHITECTURE_REOPEN_002`, `V1_2_3_TO_V1_3_IMPACT_MATRIX`, `ARCHITECTURE_REOPEN_001`, `RECONCILIATION_REPORT_V1_1` | **HISTORICAL_SNAPSHOT** | Marked and indexed as historical |
| `CLAUDE_PLUGIN_PACKAGING.md` | **REVISED** | Evidence status taken from the registry; probing retained |
| `PROVIDER_CREDENTIAL` / Codex auth language | **REVISED** | **[HISTORICAL]** "Undocumented endpoint" removed. The unresolved issue is commercial, not technical |
| `HostCapabilityReport` | **NEW SCHEMA** | Models `PLUGIN_INSTALLED`, `HOOKS_CONFIGURED`, `HOOKS_TRUSTED`, `HOOKS_ENABLED`, `HOOKS_ALLOWED_BY_ADMIN_POLICY`, `HOOK_COVERAGE_CLASS`, `selected_mode`, `inactive_reason` |
| `ProviderPolicyEligibility.evidence_label` | **REVISED** | Option A: vocabulary generated from the registry; `source_claim_id` added. Provenance never collapsed |
| Seven BUILD-A2 manager charters | **REVISED** | One normative owned-contract list each, generated from the canonical map; V1.2 list demoted to a tagged non-normative snapshot |
| `PACKAGE_INDEX.md` | **REGENERATED** | All counts derived. V1.3.1 claimed 23 schemas and 35 contracts against actual 36 and 51 |
| `REQUIREMENTS_TRACEABILITY_MATRIX.md` | **REVISED** | **[HISTORICAL]** "research unavailable" row replaced; residual questions identified by exact ID |
| ExecutionGraph · Free/Pro · admission split · ActivationState · GraphEdge exclusivity · SQLite · seven managers · provider policy classifications | **UNCHANGED** | §§24–25 preserved in full |

## The validator finding, stated plainly

Independent review ran V1.3.1's validator and reproduced all-zero output while stale assertions were physically present. **[HISTORICAL] I reproduced that too**: a probe document asserting retirement in one paragraph and the obsolete hook claim in another passed cleanly, and the script ended in `sys.exit(0)` regardless of findings.

A validator that reports success while defects are present is worse than no validator, because it converts an open question into a false assurance. That is why this pass rewrote the mechanism rather than patching the six stale sentences it should have caught.

On first run the rewritten validator found **6 stale A-14 assertions, 3 stale research-unavailable claims, 1 stale USER_DECLARED, and 2 stale matrix references** — then exited 1.
