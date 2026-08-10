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

# FIRST_MANAGER_TASK — A2-QUALITY-RELEASE

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

1. **Internal firewall declaration.** Write down how you will keep measurement and publication separate inside one manager: the task-naming convention, the A4 assignment rule, the provenance record format, and the check that catches a violation. Submit it to A1 as a compensating control for the loss of `D-010`. Do this **first** — it governs everything else you produce.
2. **Benchmark task catalogue** — all 12 tasks, 6 defective and 6 clean. For each: objective, defect class where applicable, the exact oracle, the expected outcome per arm, and why the task is not trivially gameable.
3. **Arm definitions A–E** (F optional) — exactly what each enables and disables, and precisely what state must be isolated between them.
4. **Reset fixture specification** — what is reset (repository, ledger, recipe approvals, provider session state, caches) and how reproducibility is verified.
5. **Metric definitions** — defect escape rate, false-block rate, review false-positive rate, cache-hit rate, wall-clock overhead, token/cost overhead, human intervention, override frequency. Define every **denominator** explicitly; most measurement dishonesty lives in the denominator.
6. **No-significance guard specification** — the exact language the harness refuses to emit and the mechanism enforcing the refusal.
7. **Result-integrity plan** — provenance fields, raw-output retention, and how a third party reproduces a published number from retained artifacts alone.
8. **Honest scope sentence — draft.** The exact wording for the README stating that Receipts governs Claude-Code-mediated actions only. Draft it now, before any pressure exists to soften it.
9. **L1–L4 enforcement table — draft**, with L4 explicitly marked deferred.
10. **Proof / non-proof statements — draft**, per evidence family, for review by A2-TRUST, A2-VERIFICATION, and A2-FOUNDATION.
11. **Truthfulness policy** — the rules you enforce on yourself and on any A3 writing task, plus the review step that catches violations.
12. **`OI-006` collision check** — run it now and record results across GitHub, npm, PyPI, crates.io, and the web, with date and sources.
13. **`OI-007` proposal** — demo language ecosystem and fixture approach, with reproducibility as the deciding criterion.
14. **Documentation architecture** — the full document set, each with purpose, audience, source of truth, and the manager who must sign off on its accuracy.
15. **Release checklist v0** — mapped to RG-1 through RG-10, with the evidence each gate requires and the manager supplying it.
16. **Proposed A3 task decomposition for M6 and M7**, each `NOT_ISSUED`, with the M5-complete gate stated. **No task may both measure and publish.**
17. **Explicit statement of measurement limits** — the configuration measured, and the conclusions it cannot support.

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
- your seven `build-control/a2/quality-release/` status files, populated

**Return no code. Issue no A3 task.** Do not create an A3 implementation branch or worktree, do not modify product source, and do not commit implementation work. Do not create, replace, rebase, rename, or move your A2 integration worktree — it is provisioned or validated by the currently active A1, and you verify it rather than manage it. Report to the currently active A1.
