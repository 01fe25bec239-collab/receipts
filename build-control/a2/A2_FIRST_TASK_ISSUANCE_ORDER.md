<!--
Receipts — A2 First Task Issuance Order
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
-->

# A2_FIRST_TASK_ISSUANCE_ORDER

**Issuing authority:** A1-BOOTSTRAP
**Date:** 2026-08-10 — package V2
**Semantic baseline:** `CONTRACT_FREEZE_SHA` = `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`
**System baseline:** `AGENT_SYSTEM_FREEZE_SHA` = `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>`

This document governs the order in which each manager may issue its **first A3 implementation task**, and the exact gates that must clear first. **It authorizes nothing.** A3 remains BLOCKED across the whole program until the listed gates clear and the currently active A1 explicitly authorizes the relevant implementation wave.

Implementation waves will be authorized by `A1-RUNTIME`, which is **not yet initialized**. `A1-BOOTSTRAP` does not and will not authorize any implementation wave.

## Universal preconditions — every first A3 task, without exception

1. **The currently active A1 has explicitly authorized the manager's implementation wave**, and the manager's bootstrap handoff carries `a3_implementation_authorized: true`. Initialization is not authorization, and a validated A2 integration worktree is not authorization.
2. **`OI-001` approved by A1** — runtime baseline, package manager, build/test framework, SQLite driver. Nothing can be built before the toolchain exists.
3. **`OI-002` frozen by A1** as a `CONTRACT_LEDGER_001` serialization appendix. Any code that hashes anything before this is code that will be rewritten.
4. **GAP-001 and GAP-002 have an A1 decision** — elevation to a frozen contract, or an explicit written statement of where the error model and process-safety rules are normatively located. `GAP-002` is the more urgent of the two: a frozen contract currently cites a contract ID that does not exist.
5. The issuing manager has completed its `FIRST_MANAGER_TASK.md` and A1 has accepted it.
6. The task packet satisfies the six A3 delegation conditions and uses the frozen task template.
7. All eight initialization checks passed against the supplied `A2_START_SHA`; the A2 **integration** branch and worktree validated by the active A1; the A3 **implementation** branch and worktree created per bounded task at wave time.

## Issuance sequence

| Seq | Manager | First A3 task area | Milestone | Additional gates |
|---:|---|---|---|---|
| 1 | **A2-FOUNDATION** | `A3-FINGERPRINT` — repository identity, fingerprint engine, git adapter | M0 | Fingerprint specification accepted; git fixture catalogue complete. |
| 1 | **A2-FOUNDATION** | `A3-LEDGER-SPINE` — event spine, SQLite schema, hash chain | M0 | Runs in parallel with `A3-FINGERPRINT` as a **separate** task with its own A4. |
| 2 | **A2-FOUNDATION** | `A3-PROJECTION-REBUILD` | M0 | Ledger spine accepted. |
| 3 | **A2-VERIFICATION** | Recipe schema, `recipeDigest`, execution core, receipts | M1 | M0 integrated at IG-6; `DR-003-R` satisfied; `GAP-002` decided. **The approval-path task is separately gated on `OI-005` and may not be bundled.** |
| 4 | **A2-FOUNDATION** | `A3-CLAIMS`, `A3-STALENESS`, `A3-ADMISSION` | M2 | M0 and M1 integrated; A2-TRUST's override semantics agreed. Three separate tasks. |
| 5 | **A2-TRUST** | Override / waiver semantics and their negative tests | M2 | Issued alongside seq 4 so `ADMIT_WITH_OVERRIDE` is never implemented without its guard tests. |
| 6 | **A2-CLAUDE-INTEGRATION** | CLI entry surface and exit contract | M0/M2 | May be issued earlier as a thin facade once CLI-001 consumers exist; must encode no admission logic. |
| 7 | **A2-CLAUDE-INTEGRATION** | Plugin manifest, hooks packaging, input normalization, decision encoding, L1 and L2 gates, skills | M3 | M2 integrated; `DR-004-R` and `DR-005-R` satisfied; current-doc re-verification accepted. **Separate tasks; permission configuration is not one of them.** |
| 8 | **A2-CLAUDE-INTEGRATION** | Permission configuration and deny rules | M3 | **`OI-004` frozen** with A2-TRUST fixtures. Separate from seq 7. |
| 9 | **A2-TRUST** | Codex provider, findings parsing, provider resolution and downgrade | M4 | M3 integrated; `DR-006-R` satisfied; CONFIG-003 handling specified. |
| 10 | **A2-TRUST** | Claude-session fallback | M4 | **`OI-003` frozen with A1 sign-off** and a security A4 distinct from the specification session; `DR-007-R` satisfied. Must not be bundled with seq 9. |
| 11 | **A2-TRUST** | Integrity signals, test-change detection, security suite | M5 | M4 integrated; `DR-008-R` and `DR-009-R` satisfied. |
| 12 | **A2-FOUNDATION** | `A3-EXPORT` — portable export and independent verifier | M5 | Issued alongside seq 11; export must preserve overrides and downgrades exactly. |
| 13 | **A2-QUALITY-RELEASE** | Harness, tasks, oracles, arms, metric collector | M6 | **M5 complete and integrated**; `DR-010-R` satisfied; `OI-007` accepted; oracles frozen before any run. |
| 14 | **A2-QUALITY-RELEASE** | README, docs, install, demo, release package | M7 | M6 complete; provenance records received from the evaluation side through the internal firewall; `OI-006` recorded; A2-TRUST enforcement audit final. |

## Tasks that must never be bundled

Separating these is not bureaucracy. Each pairing is a known route to a failure that review misses, and consolidation makes each one *easier* to commit because the same manager now owns both sides.

| Must stay separate | Reason | Now inside one manager? |
|---|---|---|
| Fingerprint ↔ ledger spine | Two distinct trust anchors with distinct failure modes | **Yes — A2-FOUNDATION** |
| Claims ↔ staleness ↔ admission | Three separable semantics; bundling hides derivation errors | **Yes — A2-FOUNDATION** |
| Runner execution ↔ recipe approval path | Approval is the authority boundary; bundling hides it in a larger diff | No |
| Codex provider ↔ Claude-session fallback | The fallback carries the hook-recursion and authentication risk | **Yes — A2-TRUST** |
| Review providers ↔ integrity signals | Different evidence families; must not share code paths that let one imply the other | **Yes — A2-TRUST** |
| Admission logic ↔ override semantics | Override must ship with its guard tests, not after them | No (Foundation / Trust) |
| Hook packaging ↔ permission rules | Permission denies are A2-TRUST requirements with their own negative fixtures | No |
| Measurement ↔ publication | A number must not be produced and published by the same task | **Yes — A2-QUALITY-RELEASE** |
| Any product change ↔ oracle change | An oracle changed alongside product code invalidates comparability | No |

Five of the nine pairings now sit inside a single manager. That is the specific risk consolidation introduces, and it is why every manager file repeats: **merging managers must not merge atomic implementation tasks.** A1 checks this at IG-6 by inspecting task granularity, not just test results.

## Standing rule

```
A3_START_ALLOWED =
      wave_authorized_by_A1
  AND contracts_frozen
  AND prerequisites_accepted
  AND blocking_open_issues_cleared
  AND workspace_assigned
```

If the expression is false, the manager returns a `DEPENDENCY_REQUEST`, a `CONTRACT_CHANGE_REQUEST`, an `ARCHITECTURE_DEVIATION_REQUEST`, or an open-issue proposal to A1 — **never an implementation prompt**.

Issuing an A3 task against an unfrozen contract remains the single most damaging action available to a manager, because the resulting code looks finished and is not.
