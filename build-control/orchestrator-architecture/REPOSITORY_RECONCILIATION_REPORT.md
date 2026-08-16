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

# REPOSITORY_RECONCILIATION_REPORT

Final architecture artifact summarising the accepted reconciliation. Supersedes `RECONCILIATION_REPORT_V1_1.md` as the canonical statement; the V1.1 report is retained in this package as the working record.

## Evidence and its limits

| Artifact | Establishes |
|---|---|
| `receipts-snapshot.zip` | `git archive HEAD` — exact tracked contents, 74 files |
| `git-state.txt` | Root, branch, remote, clean tree, HEAD, origin/main, ancestry, commits, inventory |

**No direct GitHub or local-machine access was used, and none is claimed.** Earlier attempts (`git ls-remote`, GitHub API) failed against the private repository. Everything below derives from the two supplied artifacts only.

## Git state — VERIFIED

| Fact | Value |
|---|---|
| Branch | `main` |
| HEAD | `3c70f4d8bac1732058de50b383f0485ab4632de9` |
| origin/main | identical to HEAD |
| Working tree | clean |
| `CONTRACT_FREEZE_SHA` `2d2dbc2c…` ancestor of HEAD | yes |
| `A2_DEFINITION_SHA` | **equals HEAD** — no commit followed the A2 freeze |
| Tracked files | 74 |

## Preserved findings

**R-01 — Two historical invariant lists exist and must not be confused.** Architecture §C: 10. Orchestration 01: 17. Numbers 9/10/11 differ in meaning between them; 12–17 exist only in the second. Historical citations follow the 17-item list.

| Concept | §C | Orchestration 01 |
|---|---|---|
| Broker-only ledger writes | 6 | 6 |
| Approved recipes only | 7 | 7 |
| Human override recorded | 8 | 8 |
| `ADMITTED_WITH_OVERRIDE` never proof | in 8 | **9** |
| Enforcement scope honest | **9** | **10** |
| Model/provider is configuration | **10** | **11** |
| Worktree ≠ security | §S | **12** |
| CLI not daemon | §Q.1 | **13** |
| Not a generic orchestrator | — | **14** |
| `ReviewProvider` stays small | — | **15** |
| MCP not introduced | — | **16** |
| Deviation required for external change | — | **17** |

**R-02 — §O/§T stale worktree guidance.** The historical architecture still instructs an always-exit-0 `WorktreeCreate` handler, conflicting with APPROVED ADR-001 and incompatible with the new orchestrator. Recorded in `HISTORICAL_BASELINE_ERRATA.md` E-01. **Historical files are not modified.**

**R-03 — All other V1 citations reconciled.** Section letters, 22 MVP `ExecutionReceipt` fields, four MVP claim types, 21 contracts all `1.0.0 FROZEN`, `D-001`…`D-013`, §I.3 proof/non-proof language, and the Codex/Claude fallback invocations all verified against the snapshot.

**R-04 — Zero drift in the historical BUILD-A2 package.** All 28 files under `build-control/a2/` are byte-identical to the package produced in the originating session. `INSTALL_MANIFEST.sha256` installed; `PACKAGE_MANIFEST.sha256` correctly not installed.

**R-05 — Filename drift.** `Receipts_Final_Architecture(1).md` referenced vs `Receipts_Final_Architecture.md` committed. Cosmetic.

**R-06 — Historical invariants 16 and 17 now dispositioned.** Invariant 16 (no MCP without a concrete unmet requirement) is satisfied by `MCP_POSITION.md`. Invariant 17 (external-capability change requires a deviation request) is the process `ARCHITECTURE_REOPEN_001` followed.

## Implementation-readiness inputs still unavailable

These are **not architecture-freeze blockers** — the architecture defines safe abstract interfaces without them, and every adapter probes its runtime at install time rather than assuming.

| Input | Blocks | Classification |
|---|---|---|
| Installed `claude` version and `--help` | First Claude adapter task (A-05) | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` |
| Installed `codex` version and `--help` | First Codex adapter task (A-15) | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` |
| Authenticated providers and plans | `PERSONAL_LOCAL_MODE` validation | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` |
| OS / Node / machine details | Sandbox availability, concurrency defaults | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` |
| First target `SPEC.md` | North-star demo realism | `NONBLOCKING` |

Named precisely rather than guessed.

## Result

```
REPOSITORY_RECONCILIATION = PASS
ARCHITECTURE_CHANGES_REQUIRED_BY_RECONCILIATION = 0
```
