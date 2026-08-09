# Receipts Contract Index

**Contract-freeze date:** 2026-08-09  
**Architecture authority:** `Receipts_Final_Architecture.md`, sections A–Z  
**A1 authority:** A1-RECEIPTS  
**Contract freeze status:** **READY / FROZEN**  
**Architecture correction of record:** ADR-001 — **APPROVED** 2026-08-09  
**Last updated:** 2026-08-09 (ADR-001 reconciliation)

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Contract status

| Contract | Version | Owner | Primary consumers | Status | First milestone |
|---|---:|---|---|---|---|
| CONTRACT-CORE-001 | 1.0.0 | A2-CORE | Ledger, Runner, Review, Plugin, Integrity, Evaluation | FROZEN | M0 |
| CONTRACT-CORE-002 | 1.0.0 | A2-CORE | Ledger, Plugin, Review, Evaluation | FROZEN | M2 |
| CONTRACT-CORE-003 | 1.0.0 | A2-CORE | Ledger, Runner, Review, Plugin, Evaluation | FROZEN | M2 |
| CONTRACT-EVIDENCE-001 | 1.0.0 | A2-CORE | Ledger, Runner, Review, Integrity, Evaluation | FROZEN | M1 |
| CONTRACT-RUNNER-001 | 1.0.0 | A2-RUNNER | Core, Ledger, Plugin, Integrity, Evaluation | FROZEN | M1 |
| CONTRACT-RUNNER-002 | 1.0.0 | A2-RUNNER | Core, Ledger, Evaluation | FROZEN | M1 |
| CONTRACT-POLICY-001 | 1.0.0 | A2-CORE | Plugin, Review, Integrity, Evaluation | FROZEN | M2 |
| CONTRACT-POLICY-002 | 1.0.0 | A2-CORE | Ledger, Plugin, Review, Integrity, Evaluation | FROZEN | M2 |
| CONTRACT-OVERRIDE-001 | 1.0.0 | A2-INTEGRITY-SECURITY | Core, Ledger, Plugin, Evaluation | FROZEN | M2/M5 |
| CONTRACT-REVIEW-001 | 1.0.0 | A2-REVIEW | Review providers, Plugin, Evaluation | FROZEN | M4 |
| CONTRACT-REVIEW-002 | 1.0.0 | A2-REVIEW | Core, Ledger, Plugin, Evaluation | FROZEN | M4 |
| CONTRACT-REVIEW-003 | 1.0.0 | A2-REVIEW | Core, Plugin, Integrity | FROZEN | M4 |
| CONTRACT-LEDGER-001 | 1.0.0 | A2-LEDGER | All event producers/consumers | FROZEN | M0 |
| CONTRACT-LEDGER-002 | 1.0.0 | A2-LEDGER | Core, Runner, Review, Integrity, Evaluation | FROZEN | M0 |
| CONTRACT-PLUGIN-001 | 1.0.0 | A2-CLAUDE-INTEGRATION | Core, Integrity, Ledger | FROZEN | M3 |
| CONTRACT-PLUGIN-002 | 1.0.0 | A2-CLAUDE-INTEGRATION | Core, Integrity | FROZEN | M3 |
| CONTRACT-CLI-001 | 1.0.0 | A2-CLAUDE-INTEGRATION | All A2s, skills, hooks, humans | FROZEN | M0 |
| CONTRACT-CONFIG-001 | 1.0.0 | A2-RUNNER | Core, Runner, Integrity | FROZEN | M1 |
| CONTRACT-CONFIG-002 | 1.0.0 | A2-CORE | Core, Plugin, Review, Integrity | FROZEN | M2 |
| CONTRACT-CONFIG-003 | 1.0.0 | A2-REVIEW | Review, Core, Plugin | FROZEN | M4 |
| CONTRACT-EXPORT-001 | 1.0.0 | A2-LEDGER | Evaluation, Docs/Release, future CI | FROZEN | M5 |

## Frozen dependency order

```text
CORE-001 + LEDGER-001 + LEDGER-002
                 |
                 v
RUNNER-001 + RUNNER-002 + EVIDENCE-001 + CONFIG-001
                 |
                 v
CORE-002 + CORE-003 + POLICY-001 + POLICY-002 + OVERRIDE-001 + CONFIG-002
                 |
                 v
PLUGIN-001 + PLUGIN-002 + CLI-001
                 |
                 v
REVIEW-001 + REVIEW-002 + REVIEW-003 + CONFIG-003
                 |
                 v
EXPORT-001
```

## Architecture-deviation gate — CLEARED

`ADR-001` was raised during contract freeze because current Claude Code documentation states that configuring `WorktreeCreate` replaces Claude Code's default Git worktree creation and requires the hook to create and return the worktree path. The frozen architecture instead treated `WorktreeCreate` as an observational identity-binding hook whose handler must be trivial and always exit 0. A1 would not silently convert Receipts into a worktree creator.

`ADR-001` was **APPROVED** by the architecture authority on 2026-08-09 and the preferred minimal correction was adopted:

- Receipts does **not** install a `WorktreeCreate` hook in MVP and does **not** own worktree creation.
- Receipts does **not** replace Claude Code's default Git worktree behavior and implements no custom worktree creation.
- Workspace identity is bound observationally from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations from the active working directory.
- `WorktreeRemove` was re-verified against current official documentation and is **also omitted** from the MVP installed hook set; it is not retained for symmetry, and workspace cleanup remains Claude Code's / Git's responsibility.
- No other architecture semantics changed. `CLAIM → EVIDENCE → POLICY → ADMISSION`, `CodeStateFingerprint`, evidence authority, staleness, `VerificationRecipe`, `ExecutionReceipt`, `ReviewProvider`, the L1 `TaskCompleted` gate, the L2 `PreToolUse` gate, the CLI → SQLite broker topology, the security/trust model, and the evaluation architecture are unchanged.

`CONTRACT-PLUGIN-001` and `CONTRACT-PLUGIN-002` are consequently at **1.0.0 FROZEN**. The full record, including the `WorktreeRemove` verification and the one unresolved third-party conflict it leaves open, is in `ARCHITECTURE_DEVIATION_REQUEST_001.md`. `OI-009` tracks post-MVP re-verification by local smoke test.

**No architecture-blocking issue remains.** Remaining open issues (runtime/library selection, canonical serialization, Claude fallback invocation, permission fixtures, recipe-approval UX, and similar) are implementation decisions that block specific future A3 tasks. They are not contract-freeze blockers and MUST NOT be represented as architecture blockers.

## Contract-change request minimum contents

Contract ID/current version; exact clause; reason/evidence; producer/consumer impact; compatibility impact; security impact; migration; proposed version; A1 decision.
