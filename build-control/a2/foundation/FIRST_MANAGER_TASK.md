<!--
Receipts — A2 Component Manager Initialization (5-manager FINAL topology, V2)
Issued by: A1-BOOTSTRAP (temporary bootstrap A1: designs, freezes, and packages the
           Receipts multi-agent operating system; retires on authority transfer)
Issued: 2026-08-10
Repository: 01fe25bec239-collab/receipts   Remote: origin -> https://github.com/01fe25bec239-collab/receipts   Integration branch: main
CONTRACT_FREEZE_SHA: 2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221
AGENT_SYSTEM_FREEZE_SHA: <AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>
Authority at runtime: A1-RUNTIME (not yet initialized). Report upward to the
currently active A1 -- never to a specific model, vendor, or conversation.
Supersedes the 8-manager package for manager topology only. Product architecture and
frozen contract semantics are UNCHANGED.
-->

# FIRST_MANAGER_TASK — A2-FOUNDATION

**This task is SPECIFICATION, VALIDATION, OWNERSHIP CONFIRMATION, and BLOCKER RESOLUTION only.**

It is **not** source implementation. A3 implementation remains blocked until A1 explicitly authorizes the relevant implementation wave. If your output contains source code, the task has failed.

## Universal steps — every manager

1. **Verify the bootstrap handoff and the baseline.** Confirm your handoff has no unresolved placeholders in any required field, then run all eight initialization checks from your manager file: HEAD equals `a2_start_sha`; the working tree is clean; the remote matches; `CONTRACT_FREEZE_SHA` is an ancestor of HEAD; `AGENT_SYSTEM_FREEZE_SHA` is an ancestor of or equal to `A2_START_SHA`; and the branch and worktree path match the handoff. Record every result. **If any check fails, STOP and report to the currently active A1** — do not repair, reset, or proceed.
2. **Read your `CONTEXT_MANIFEST.md` first**, then ingest the `MANDATORY` class in full. Record what you read and the frozen version of each contract.
3. **Confirm ownership.** Walk `OWNERSHIP_MANIFEST.md` against the repository at the frozen SHA. Report any path that is claimed but does not exist, exists but is unclaimed, or appears claimed by more than one manager.
4. **Confirm contract classification.** For all 21 frozen contracts, confirm your `OWNED` / `CONSUMED` / `REFERENCE` / `EXCLUDED` classification is correct and complete. Report any contract you believe is misclassified — with a reason, not a preference.
5. **Semantic-conflict check.** Confirm that consolidation changed ownership only. If any committed orchestration statement conflicts *semantically* with the architecture or a frozen contract, **stop and raise it to the currently active A1**; do not reconcile it yourself.
6. **Confirm your worktree model.** State the integration branch and worktree path you verified, and confirm your `a3_implementation_authorized` value. If it is `false`, list what you may do (specification, analysis, proposals, `NOT_ISSUED` task packets) and what you may not (branches, worktrees, source files, A3 prompts).
7. **Report inherited blockers** — the open issues and gaps remapped to you, each with your proposed resolution path and what it is waiting on.

## Component-specific deliverables

1. **`OI-001` proposal for A1 approval.** Runtime baseline, package manager, build and test framework, SQLite driver. For each: alternatives considered, decision, reason, risk, and migration cost if wrong. Name versions. Include the effect on native-module portability for the fresh-install release gate (RG-10).
2. **`OI-002` proposal for A1 approval.** The exact canonical serialization algorithm: dialect, key ordering, string and unicode normalization, number representation, null and absent-field handling, timestamp normalization, and the precise bytes fed to the digest. At least six golden fixtures with expected digests, including a unicode case and a numeric-edge case, plus how an independent verifier reproduces them.
3. **Hash-chain specification** — genesis handling, previous-hash linkage, exactly what each hash covers, and what a verifier must recompute.
4. **Fingerprint specification pack** — exact git invocations with explicit argv, exact byte-level composition order, exact hashing inputs, tie-break and normalization rules. Specification, not code.
5. **Contract self-audit** across all ten owned contracts. Give specific attention to: `repoId` fallback for repositories without a root commit; the exact definition of "untracked-not-ignored"; whether `workingTreeDigest` covers file mode and symlink target; and the precise input set `admit()` may read.
6. **Purity contract for `admit()`** — permitted inputs, prohibited operations, and the mechanism you will require an A3 to use so purity is machine-checkable.
7. **SQLite schema draft** — tables, indices, pragmas, migration approach, transaction boundary per invocation.
8. **Tamper-evidence threat model**, written honestly, including the local-attacker limitation.
9. **Git fixture catalogue** — every M0 fixture with the property it proves, including hostile filenames and no-root-commit.
10. **GAP-001 consumer position** — can you implement the domain typed error model against `CONTRACT_CLI_001.md` alone, or does A1 need to elevate `CONTRACT-ERROR-001`? Recommend, with a reason.
11. **Proposed A3 task decomposition for M0 and M2** — at minimum `A3-FINGERPRINT`, `A3-LEDGER-SPINE`, `A3-PROJECTION-REBUILD`, `A3-CLAIMS`, `A3-STALENESS`, `A3-ADMISSION`, `A3-EXPORT`, each atomic, each with owned files, input/output contracts, and unmet preconditions. Mark every one `NOT_ISSUED`. **Do not collapse these into one task because one manager now owns both domains.**

## Output format

Return documents only:

- handoff-validation and six-check baseline-verification record
- ingestion record (files read at `A2_START_SHA`, contract versions)
- ownership confirmation, with discrepancies
- contract classification confirmation, with discrepancies
- semantic-conflict report (or an explicit "none found")
- inherited-blocker report with proposed resolutions
- your component-specific deliverables above
- your worktree-model confirmation
- your seven `build-control/a2/foundation/` status files, populated

**Return no code. Issue no A3 task.** Do not create an A3 implementation branch or worktree, do not modify product source, and do not commit implementation work. Do not create, replace, rebase, rename, or move your A2 integration worktree — it is provisioned or validated by the currently active A1, and you verify it rather than manage it. Report to the currently active A1.
