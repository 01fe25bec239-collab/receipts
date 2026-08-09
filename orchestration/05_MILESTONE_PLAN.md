# 05 — Milestone Plan

| Milestone | Scope | Owner | Contributors | Contracts before A3 | Input dependencies | Artifacts | Acceptance evidence |
|---|---|---|---|---|---|---|---|
| M0 | Fingerprint + ledger spine | A2-CORE | A2-LEDGER, A2-INTEGRITY-SECURITY | CORE-001; LEDGER-001; STORAGE-001; ERROR-001 | None; architecture authority only | Fingerprint engine; repository identity; event spine; SQLite projections; verify-ledger | Fingerprint changes on tracked edit, restores on revert, ignored files excluded; hash mutation detected; projections rebuild identically. |
| M1 | Recipes + runner + receipts | A2-RUNNER | A2-CORE, A2-LEDGER, A2-INTEGRITY-SECURITY | CORE-001; RUNNER-001; RUNNER-002; EVIDENCE-001; PROCESS-001; LEDGER-001 | M0 | Recipe schema/approval; recipeDigest; runner; receipts; gz logs; locks; flaky warning basis | Approved test recipe yields exact argv/cwd/exit/digests; unapproved agent proposal cannot execute; recipe change invalidates evidence. |
| M2 | Claims + admit() | A2-CORE | A2-LEDGER, A2-RUNNER, A2-INTEGRITY-SECURITY | CORE-002; CORE-003; EVIDENCE-001; POLICY-001; ADMISSION-001; OVERRIDE-001 | M0 + M1 | Claim derivation; staleness; LIGHT/STANDARD; pure admit(); causedByPaths | Unit/property tests for PROVED/REJECTED/STALE/WAIVED; pure no-I/O admission; blocked decisions name changed paths. |
| M3 | Claude Code integration L1/L2 | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY, A2-LEDGER | HOOKS-001; HOOKS-002; ADMISSION-001; CORE-002; ERROR-001 | M2 | Plugin; hooks; permissions; status/verify skills; L1 TaskCompleted and L2 merge/push gates | TaskCompleted blocks unmet task; merge/push denied while blocked; protected config edits denied; shipped `hooks/hooks.json` declares no `WorktreeCreate` and no `WorktreeRemove` entry (ADR-001); workspace identity binds observationally from SessionStart cwd + repository identity + read-only Git worktree metadata; output <10k; async observer does not block. |
| M4 | Review providers | A2-REVIEW | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY | REVIEW-001; REVIEW-002; ADMISSION-001; PROCESS-001; LEDGER-001 | M3 | Codex provider; Claude-session fallback; provider config; finding schema; review skill integration | Malformed/timeout remains UNPROVEN; model recorded as reported; different vendor preferred when healthy; fallback downgrade recorded; reviewer cannot write. |
| M5 | Integrity signals + override | A2-INTEGRITY-SECURITY | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-RUNNER, A2-REVIEW | OVERRIDE-001; EXPORT-001; ADMISSION-001; EVIDENCE-001; HOOKS-002 | M4 | Test diff signals; test-count delta; deletion policy; human override; export | Test deletion exposed/forces review diff; agent override impossible; overridden task never rendered verified; export independently hash-verifiable. |
| M6 | Evaluation harness | A2-EVALUATION | All product A2s | All product contracts frozen | M5 | 12 tasks; oracles; arms A–E; repeated runs; metric collector; raw results | Clean checkout reproducibility; >=3 runs/task/arm; defective vs clean separated; no significance claims. |
| M7 | Documentation + release evidence | A2-DOCS-RELEASE | All A2s, especially A2-EVALUATION | ARCH authority; EXPORT-001; all public schemas/contracts | M6 | README + docs + demo + install + release package + collision evidence | README states exact proof/non-proof and L1-L4 scope; no unmeasured evaluation number; name collision checked before adoption. |

## Milestone closure rule

A milestone closes only when:
1. its A3 task outputs have independent A4 review;
2. owner A2 has accepted all required evidence;
3. contract tests between contributors pass;
4. A1 integration gate passes;
5. no architecture invariant is weakened.

Measured evaluation outcomes are not an acceptance prerequisite until M6; before M6 all numeric thresholds remain targets only.
