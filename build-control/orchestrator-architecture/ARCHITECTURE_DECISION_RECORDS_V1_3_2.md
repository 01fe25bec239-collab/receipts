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

# ARCHITECTURE_DECISION_RECORDS_V1_3_2

**Current normative ADR record.** The V1.3 ADR set is preserved as `ARCHITECTURE_DECISION_RECORDS_V1_3.md` with `DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT`.

## ADR-HOST-002 — Codex native plugin/hook integration is the current primary path

**Supersedes ADR-HOST-001.**

**Decision.** Codex host integration uses the native plugin and lifecycle-hook path as its **primary** posture, identical in kind to Claude Code. Supervised and hybrid are compatibility fallbacks selected by runtime capability discovery.

**Evidence.** Registry `C-01` and `C-02`, `VERIFIED_CURRENT_SELF_FETCHED` 2026-08-15 — fetched directly rather than accepted on report.

**[HISTORICAL] What ADR-HOST-001 decided, and why it was right then.** At V1.3 the claim that Codex had plugins was `USER_DECLARED`. ADR-HOST-001 rejected retiring A-14 on unverified evidence and made host posture discovery-driven instead. That was the correct decision on the evidence available: it avoided repeating the original error in the opposite direction, and it meant that when verification arrived, **no architecture document had to change** — discovery simply selected EMBEDDED.

**A-14 current status: RETIRED.** The 2026-08-13 observation is preserved in the registry as `C-03` and nowhere asserted as current.

**Consequences.** Discovery is retained and is now *more* load-bearing, not less: a host supporting hooks is not the same as our hooks being configured, trusted, enabled and permitted. `HostCapabilityReport` models those states explicitly.

## ADR-HOST-003 — Hooks are never the enforcement boundary

**Decision.** Entitlement and security authority live exclusively in the shared core. No host hook layer is treated as an enforcement boundary.

**Evidence.** OpenAI's own documentation states specialized tool paths may bypass hook coverage and that hooks are a guardrail rather than a complete enforcement boundary (`C-02c`). Independently: plugin hooks are untrusted until reviewed, with trust bound to the hook hash (`C-02a`), and hooks can be disabled entirely or excluded by managed-only policy (`C-02b`).

**Rejected — enforce entitlement in a `PreToolUse` hook.** Superficially attractive, since `PreToolUse` can deny or rewrite calls. Rejected because all three findings above mean the hook may simply not run.

**Consequences.** `HostCapabilityReport.inactive_reason` surfaces the exact condition. The product never silently waits for events that cannot arrive.

## ADR-EVIDENCE-001 — One machine-readable source registry, enforced by a hardened validator

**Decision.** `evidence/SOURCE_CLAIM_REGISTRY.json` is the sole authority for volatile vendor claims. The source matrix is generated from it. Every source-bearing document declares `DOCUMENT_AUTHORITY`. Blocks opt into historical status with an explicit `[HISTORICAL]` marker.

**[HISTORICAL] Why the marker is explicit rather than inferred.** V1.3.1's validator inferred historical status from nearby text, which let a document asserting retirement in one paragraph and the obsolete hook claim in another pass cleanly. Proximity is not evidence of intent. An explicit marker is a declaration.

**Consequences.** The validator exits non-zero on any failure, and six regression fixtures prove it catches the cases V1.3.1 missed — including that exact one.
