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

# ASSUMPTION_REGISTER

> **This document no longer carries independent evidence status.**
> `evidence/SOURCE_CLAIM_REGISTRY.json` is the single authority, and `SOURCE_VERIFICATION_MATRIX_V1_3_6.md` is generated from it. This register records *architectural assumptions and design decisions*, and points at the registry for every vendor claim.
>
> V1.3 carried per-assumption `VERIFIED_CURRENT` labels that contradicted its own source matrix. That duplication was the defect; removing the duplicate authority is the fix.

## Vendor claims — see the registry

| Former assumption | Registry claim | Current status |
|---|---|---|
| A-01 Claude plugin system | `C-04` | Reviewer-supplied current primary source |
| A-02 Claude hook events | `C-05` | Reviewer-supplied current primary source |
| A-03 `WorktreeCreate` replaces default behaviour and returns a path | `C-05` | Reviewer-supplied current primary source |
| A-04 `claude -p` headless with structured output and resume | `C-07` | Reviewer-supplied current primary source |
| A-05, A-06, A-09 exact Claude flag spellings, output caps, subagent frontmatter | — | Still `UNVERIFIED`. Probed at install; nothing depends on them |
| A-10…A-13, A-15 `codex exec` flag surface, sandbox, resume | `C-06` | `VERIFIED_HISTORICAL` — not re-verified this pass; probed at install |
| **A-14** — see `C-03` | **`C-03`** | **[HISTORICAL] RETIRED as a current assumption; currently false per `C-02`.** The 2026-08-13 wording is preserved in the registry, not restated here |
| A-20 Anthropic third-party credential restriction | `C-10` | Reviewer-supplied current primary source; `VERIFIED_DISALLOWED` for the external-worker context |
| A-21 Codex ChatGPT sign-in | `C-11` | Reviewer-supplied. Technical support and policy eligibility are recorded separately |
| A-22, A-23 rate-limit headers and quota visibility | — | `VERIFIED_HISTORICAL`; quota remains `UNKNOWN`-tolerant by design |
| A-25…A-31 other runtimes | — | Unchanged; all deferred adapters |
| A-32 no normalised provider safety state | — | Still `ASSUMPTION`. `UNKNOWN` remains the safe default |
| A-50…A-54 repository reconciliation facts | — | Established from the supplied snapshot; unchanged |

## Design decisions — ours, not vendor facts

| ID | Decision | Registry |
|---|---|---|
| A-40 | SQLite as the MVP state store, now including graph tables | `C-20` |
| A-41 | RUNTIME-A3 and A4 are always fresh sessions | — |
| A-42 | Default manager failover is `FRONTIER_FAILOVER` | — |
| A-43 | Default routing mode is `ASK_ON_UNCERTAINTY` | — |
| A-44 | Default repair bound is 3 attempts | — |
| A-45 | Default assurance profile is `STANDARD` | — |
| A-46 | Default remote branch policy is `PUSH_A2_BRANCHES` | — |
| A-47 | Seven BUILD-A2 managers | — |
| A-48 | The orchestrator owns worktree creation | `C-05` |
| A-49 | Worker checks accepted as evidence under LIGHT/STANDARD | — |
| A-60 | One graph core with two execution policies | `C-21` |
| A-61 | Open core with proprietary Pro modules | `C-22` |
| A-62 | Host capability discovery retained, native path primary where verified | `C-23` |
| A-63 | On-demand local core invocation rather than a daemon | — |

## Rule

A claim's evidence status is changed **in the registry**, never in prose. `evidence/validate_sources.py` enforces agreement; a document asserting a status the registry does not carry is a validation failure.
