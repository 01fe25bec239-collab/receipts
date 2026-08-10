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

# FIRST_MANAGER_TASK — A2-VERIFICATION

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

1. **`GAP-002` escalation** — a precise note to A1: `CONTRACT-PROCESS-001` is cited by name inside a frozen contract but has no file. List where each process-safety rule currently lives across RUNNER-001/002, REVIEW-003, and CLI-001, and recommend elevation or explicit relocation. This is your highest-priority deliverable because a frozen contract currently points at nothing.
2. **Process-safety specification** — the complete rule set: argv construction, executable resolution, cwd realpath validation, environment allowlist, timeout and cancellation, output capture and bounding. Written so an A4 can check compliance mechanically.
3. **`OI-005` joint proposal** with A2-TRUST for A1 to freeze: the interactive human approval UX and its persistence representation. Include the threat model for agent-manufactured approval, the mechanism that prevents it, how a tampered approval record is detected, and at least two rejected alternatives with why they fail.
4. **`recipeDigest` specification** — exactly which fields are covered, the canonicalization used, and invalidation semantics, with worked examples. Coordinate the canonicalization with A2-FOUNDATION's `OI-002` so there is one algorithm in the product, not two.
5. **Receipt field audit** — confirm every MVP `ExecutionReceipt` field against `CONTRACT_RUNNER_002.md` and architecture §I.2, field by field. Report discrepancies; do not reconcile them silently.
6. **Duplicate-run suppression specification** — the exact conditions under which a run is skipped and an existing receipt reused, and the proof that suppression can never fabricate evidence.
7. **Fixture catalogue** — fake executables and the real demo-ecosystem fixture, each with the property it proves.
8. **Proposed A3 task decomposition for M1** — at minimum recipe schema and validation, approval state, `recipeDigest`, the execution core, receipt production, raw-log handling, and locking/suppression as **separate atomic tasks**. Mark every one `NOT_ISSUED` with unmet preconditions. The approval-path task must be separate from the execution task and separately gated on `OI-005`.

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
- your seven `build-control/a2/verification/` status files, populated

**Return no code. Issue no A3 task.** Do not create an A3 implementation branch or worktree, do not modify product source, and do not commit implementation work. Do not create, replace, rebase, rename, or move your A2 integration worktree — it is provisioned or validated by the currently active A1, and you verify it rather than manage it. Report to the currently active A1.
