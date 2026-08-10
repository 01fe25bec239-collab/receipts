<!--
Receipts — A2 Ownership Remap
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
AUTHORITATIVE for current manager identity and ownership.
-->

# A2_OWNERSHIP_REMAP

**Issuing authority:** A1-BOOTSTRAP
**Date:** 2026-08-10 — package V2
**Semantic baseline:** `CONTRACT_FREEZE_SHA` = `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`
**System baseline:** `AGENT_SYSTEM_FREEZE_SHA` = `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>`
**Install path:** `build-control/a2/A2_OWNERSHIP_REMAP.md`
**Status:** **AUTHORITATIVE for current manager identity, manager count, contract ownership, and file ownership.**

This document is authority because it is a committed repository artifact, not because any particular agent issued it. It survives A1 succession unchanged and binds `A1-RUNTIME` exactly as it binds `A1-BOOTSTRAP`.

## Precedence

1. `Receipts_Final_Architecture.md` and the 21 frozen contracts — **product semantics**. This document may not alter them and does not.
2. **This document** — manager identity, manager count, contract ownership, file ownership.
3. The committed orchestration package — everything else.

Where a committed orchestration document names a superseded manager for a contract or a path, **this document wins**. Where a conflict is **semantic** rather than about ownership, **stop and raise it to A1**.

## Manager register

| Manager | Slug | Integration branch | Definition folder | Responsibility | Activation phase |
|---|---|---|---|---|---|
| **A2-FOUNDATION** | `foundation` | `a2/foundation` | `build-control/a2/foundation/` | Core domain + ledger domain | 1 |
| **A2-VERIFICATION** | `verification` | `a2/verification` | `build-control/a2/verification/` | Deterministic verification execution | 2 |
| **A2-CLAUDE-INTEGRATION** | `claude-integration` | `a2/claude-integration` | `build-control/a2/claude-integration/` | Claude Code product integration | 3 |
| **A2-TRUST** | `trust` | `a2/trust` | `build-control/a2/trust/` | Probabilistic review + integrity + security + break-glass | 4 |
| **A2-QUALITY-RELEASE** | `quality-release` | `a2/quality-release` | `build-control/a2/quality-release/` | Evaluation + documentation + release evidence | 5 |

Manager roles are **logical**. Any capable runtime may execute any of them; nothing here requires a particular model. `A2-CLAUDE-INTEGRATION` builds Claude Code functionality and may itself run on a non-Claude runtime.

**Exactly five.** No sixth manager without a genuine architecture-blocking reason and A1 approval.

## Contract ownership — all 21 frozen contracts

| Contract | Subject | Version | Owner |
|---|---|---|---|
| `CONTRACT_CORE_001.md` | CodeStateFingerprint | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_CORE_002.md` | Task + TaskState | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_CORE_003.md` | Claim + ClaimStatus | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_EVIDENCE_001.md` | Evidence families + Evidence | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_POLICY_001.md` | VerificationPolicy | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_POLICY_002.md` | Admission + AdmissionDecision | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_CONFIG_002.md` | `.receipts/policy.yaml` | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_LEDGER_001.md` | LedgerEvent | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_LEDGER_002.md` | Append-only / projection invariants | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_EXPORT_001.md` | Portable ledger export | 1.0.0 FROZEN | A2-FOUNDATION |
| `CONTRACT_RUNNER_001.md` | VerificationRecipe | 1.0.0 FROZEN | A2-VERIFICATION |
| `CONTRACT_RUNNER_002.md` | ExecutionReceipt | 1.0.0 FROZEN | A2-VERIFICATION |
| `CONTRACT_CONFIG_001.md` | `.receipts/recipes.yaml` | 1.0.0 FROZEN | A2-VERIFICATION |
| `CONTRACT_PLUGIN_001.md` | Normalized hook → broker request | 1.0.0 FROZEN | A2-CLAUDE-INTEGRATION |
| `CONTRACT_PLUGIN_002.md` | Broker → hook decision / error response | 1.0.0 FROZEN | A2-CLAUDE-INTEGRATION |
| `CONTRACT_CLI_001.md` | `receipts` CLI semantics + exit codes | 1.0.0 FROZEN | A2-CLAUDE-INTEGRATION |
| `CONTRACT_REVIEW_001.md` | ReviewRequest | 1.0.0 FROZEN | A2-TRUST |
| `CONTRACT_REVIEW_002.md` | ReviewResult + ReviewFinding | 1.0.0 FROZEN | A2-TRUST |
| `CONTRACT_REVIEW_003.md` | ReviewProvider | 1.0.0 FROZEN | A2-TRUST |
| `CONTRACT_CONFIG_003.md` | `.receipts/providers.yaml` | 1.0.0 FROZEN | A2-TRUST |
| `CONTRACT_OVERRIDE_001.md` | Override / Waiver semantics | 1.0.0 FROZEN | A2-TRUST |

**A2-QUALITY-RELEASE owns no product runtime contract.** This is structural: an evaluator or publisher that owns product contracts can reshape the product to suit its own output.

### Ownership integrity check

| Check | Result |
|---|---|
| Contracts in freeze package | 21 |
| Contracts with exactly one owner | 21 |
| Contracts ownerless | **0** |
| Contracts multiply owned | **0** |
| Owner totals | FOUNDATION 10, VERIFICATION 3, CLAUDE-INTEGRATION 3, TRUST 5, QUALITY-RELEASE 0 = **21** |

Ownership means the manager answers for the contract and originates any change request. **It does not mean the manager may edit it.** All 21 remain frozen; changes go through `CONTRACT_CHANGE_REQUEST` to A1.

## Contract ownership — old → new

| Contract | Previous owner | Current owner | Change |
|---|---|---|---|
| CORE-001/002/003, EVIDENCE-001, POLICY-001/002, CONFIG-002 | A2-CORE | A2-FOUNDATION | manager merged |
| LEDGER-001/002, EXPORT-001 | A2-LEDGER | A2-FOUNDATION | manager merged |
| RUNNER-001/002, CONFIG-001 | A2-RUNNER | A2-VERIFICATION | manager renamed |
| PLUGIN-001/002, CLI-001 | A2-CLAUDE-INTEGRATION | A2-CLAUDE-INTEGRATION | unchanged |
| REVIEW-001/002/003, CONFIG-003 | A2-REVIEW | A2-TRUST | manager merged |
| OVERRIDE-001 | A2-INTEGRITY-SECURITY | A2-TRUST | manager merged |

**No contract semantics changed.** Ownership moved; text did not.

## Repository write ownership

Only `contracts/**` exists as owned content at `CONTRACT_FREEZE_SHA`. All source paths are **planned** and are created only when A1 authorizes the owning manager's implementation wave.

| Path | Owner |
|---|---|
| `contracts/CONTRACT_{CORE_001,CORE_002,CORE_003,EVIDENCE_001,POLICY_001,POLICY_002,CONFIG_002,LEDGER_001,LEDGER_002,EXPORT_001}.md` | A2-FOUNDATION |
| `src/core/fingerprint/**`, `src/core/claims/**`, `src/core/policy/**`, `src/core/ledger/**`, `src/adapters/git/**` | A2-FOUNDATION |
| `schemas/{fingerprint,task,claim,evidence,policy,admission,ledger-event,export}.schema.json` | A2-FOUNDATION |
| `contracts/CONTRACT_{RUNNER_001,RUNNER_002,CONFIG_001}.md` | A2-VERIFICATION |
| `src/adapters/runner/**`, `schemas/{recipe,receipt}.schema.json` | A2-VERIFICATION |
| `contracts/CONTRACT_{PLUGIN_001,PLUGIN_002,CLI_001}.md` | A2-CLAUDE-INTEGRATION |
| `.claude-plugin/**`, `hooks/**`, `skills/**`, `agents/**`, `src/entry/**`, `bin/**` | A2-CLAUDE-INTEGRATION |
| `schemas/{hook-request,hook-decision,cli-envelope}.schema.json` | A2-CLAUDE-INTEGRATION |
| `contracts/CONTRACT_{REVIEW_001,REVIEW_002,REVIEW_003,CONFIG_003,OVERRIDE_001}.md` | A2-TRUST |
| `src/adapters/providers/**`, `src/core/integrity/**` | A2-TRUST |
| `schemas/{finding,review-request,review-result,override}.schema.json` | A2-TRUST |
| security test suite; enforcement-scope audit | A2-TRUST |
| `eval/**`, `README.md`, `docs/**` | A2-QUALITY-RELEASE |
| `build-control/a2/<slug>/**` | the manager with that slug |
| `Receipts_Final_Architecture.md`, `orchestration/**`, `architecture-decisions/**`, `contracts/CONTRACT_INDEX.md`, `schemas/SCHEMA_PLAN.md` | **the currently active A1** |
| `build-control/a2/*.md` (the **seven** program-level files), `build-control/a2/INSTALL_MANIFEST.sha256` | **the currently active A1** |
| `build-control/a2/<slug>/{A2_*_MANAGER,CONTEXT_MANIFEST,OWNERSHIP_MANIFEST,FIRST_MANAGER_TASK}.md` | **the currently active A1** — a manager reads its own charter and never edits it |

**No two managers hold overlapping write ownership.** There is no shared-file protocol because there are no shared files. If one appears necessary, that is an A1 escalation, not a local arrangement.

### Ownership of a file is not ownership of the rule inside it

| File | Written by | Rule owned by |
|---|---|---|
| `agents/receipts-reviewer.md` | A2-CLAUDE-INTEGRATION | read-only tool list: **A2-TRUST** |
| `hooks/hooks.json`, plugin `settings.json` | A2-CLAUDE-INTEGRATION | deny rules and fail direction: **A2-TRUST** |
| `src/adapters/runner/**` | A2-VERIFICATION | approval-authority and command-safety requirements: **A2-TRUST** |

The writing manager must pass the requirement owner's acceptance tests. The requirement owner may block acceptance and may never implement around the writing manager.

## Open issue remap

| Issue | Previous owner | Current owner | Blocking level |
|---|---|---|---|
| `OI-001` runtime / library baseline | A2-LEDGER | **A2-FOUNDATION** | Blocks all A3; A1 approves |
| `OI-002` canonical serialization | A2-LEDGER | **A2-FOUNDATION** | Blocks all A3; A1 freezes |
| `OI-003` Claude fallback invocation | A2-REVIEW (+ security sign-off) | **A2-TRUST** with A2-CLAUDE-INTEGRATION; **A1 signs off** | Blocks Claude-fallback A3 |
| `OI-004` permission deny rules | A2-CLAUDE-INTEGRATION + A2-INTEGRITY-SECURITY | **A2-CLAUDE-INTEGRATION + A2-TRUST** | Blocks M3 permission A3 |
| `OI-005` recipe-approval UX | A2-RUNNER + A2-INTEGRITY-SECURITY | **A2-VERIFICATION + A2-TRUST** | Blocks approval-path A3 |
| `OI-006` name collision check | A2-DOCS-RELEASE | **A2-QUALITY-RELEASE** | Release only (RG-9) |
| `OI-007` demo ecosystem / fixtures | A2-EVALUATION | **A2-QUALITY-RELEASE** | M6 only |
| `OI-008` Gemini provider | A2-REVIEW | **A2-TRUST** | Deferred / optional |
| `OI-009` worktree-hook re-verification | A2-CLAUDE-INTEGRATION | **A2-CLAUDE-INTEGRATION** | Post-MVP; both hooks stay uninstalled |

## Gap remap — preserved, not closed

| Gap | Detail | Current owner | A1 role |
|---|---|---|---|
| **GAP-001** | `CONTRACT-ERROR-001` referenced but no file exists; the typed error model lives inside `CONTRACT_CLI_001.md` | **A2-CLAUDE-INTEGRATION** proposes; **A2-FOUNDATION** returns a consumer position | **A1 decides** elevation or explicit relocation |
| **GAP-002** | `CONTRACT-PROCESS-001` referenced **by name inside frozen `CONTRACT_PLUGIN_001.md`** but no file exists; rules distributed across RUNNER-001/002, REVIEW-003, CLI-001 | **A2-VERIFICATION** escalates with a proposed specification | **A1 decides** elevation or explicit relocation |

Neither gap was silently closed and neither may be. **A1 remains the final authority on whether a new frozen contract is genuinely required, and the default answer is no.** Do not convert every implementation question into a contract.

## Standing decisions unchanged by this remap

- `D-002` — MVP broker is a short-lived CLI → SQLite; no daemon.
- `D-003` — **MCP NOT REQUIRED FOR MVP.** Adding it requires a proven concrete capability gap that hooks, skills, and the `receipts` CLI cannot satisfy — not appearance or portfolio complexity.
- `D-004` — Codex reviewer is read-only and never uses `--full-auto`.
- `D-006` — plugin-subagent `permissionMode` is not an authority path for reviewer read-only behavior.
- `D-008` — whole-tree evidence invalidation in MVP.
- `D-009` — stored admission is never source of truth.
- `D-010` — **superseded in form, preserved in substance** by the A2-QUALITY-RELEASE internal firewall.
- `D-011` — branches and worktrees are workspace mechanics, never a security boundary.
- **ADR-001** — APPROVED and binding: no `WorktreeCreate` hook, no `WorktreeRemove` hook, no Receipts-owned worktree creation, observational workspace identity only.

## Baseline model

| Baseline | Role | Value |
|---|---|---|
| `CONTRACT_FREEZE_SHA` | Immutable semantic baseline; permanent historical authority | `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221` |
| `AGENT_SYSTEM_FREEZE_SHA` | `main` commit holding the complete frozen agent operating system | `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>` |
| `A2_START_SHA` | Per-manager start commit, supplied in the bootstrap handoff | assigned at initialization |

A manager verifies HEAD against its supplied `A2_START_SHA`, and verifies `CONTRACT_FREEZE_SHA` as an **ancestor** of HEAD. **Manager initialization is never coupled to one historical SHA**, so a manager initialized or re-initialized later may legitimately start from a newer accepted `main` commit. No agent may fabricate a SHA.

## A1 lifecycle

| Role | State now | Authority |
|---|---|---|
| `A1-BOOTSTRAP` | **ACTIVE** | Designs, freezes, and packages the agent operating system. Issues no implementation wave. |
| `A1-RUNTIME` | **NOT YET INITIALIZED** | On formal transfer: validates A2 integration worktrees, initializes managers, authorizes implementation waves, runs integration gates. |

Never two authoritative A1 agents. On transfer, `A1-BOOTSTRAP` becomes RETIRED and issues nothing further.

## Current program state

| Item | State |
|---|---|
| Local + remote repository | CREATED, clean at `CONTRACT_FREEZE_SHA` |
| Active A1 | `A1-BOOTSTRAP` |
| `A1-RUNTIME` | **NOT INITIALIZED**; authority transfer **NOT PERFORMED** |
| `AGENT_SYSTEM_FREEZE_SHA` | **NOT YET ASSIGNED** |
| A2 integration branches | **NOT CREATED** |
| A2 integration worktrees | **NOT CREATED** |
| A2 managers initialized | **NO** |
| A3 implementation | **BLOCKED** — no wave authorized |
| A4 code review | **NOT STARTED** |
| Contract freeze | READY / FROZEN — 21 contracts at 1.0.0 |
| Architecture deviations | ADR-001 APPROVED and reconciled; none pending |
