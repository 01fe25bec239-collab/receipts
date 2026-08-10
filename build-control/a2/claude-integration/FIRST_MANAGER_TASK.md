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

# FIRST_MANAGER_TASK — A2-CLAUDE-INTEGRATION

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

1. **Current-documentation re-verification.** Independently re-check current official Claude Code documentation for: plugin layout and manifest; plugins reference; the hooks reference for every installed event **and both worktree events**; permissions; skills; sub-agents; headless/programmatic mode; CLI usage. Record URL, access date, and the exact field, flag, or event relied on. **Report any divergence from the frozen contracts to A1 as a finding — do not adapt silently.** This is the check that produced ADR-001 and it is your highest-value deliverable.
2. **Hook set declaration** — each installed event with purpose, gate-or-observer classification, decision encoding, timeout budget, and fail direction. Include an explicit statement that `WorktreeCreate` and `WorktreeRemove` are not installed, and why.
3. **Workspace-observation specification** — how `cwd`, repository identity, read-only Git metadata, and normal broker invocation context combine into a workspace binding, plus the exact lazy-invalidation rule. Coordinate the read-only Git queries with A2-FOUNDATION so there is one git adapter, not two.
4. **`OI-004` test plan** with A2-TRUST — how you will verify deny-rule representation for `.receipts/policy.yaml`, `.receipts/recipes.yaml`, and the `CLAUDE_PLUGIN_DATA`-rooted ledger path against the current Claude version, and which fixtures you will freeze.
5. **GAP-001 proposal** — recommend whether the typed error model should be elevated out of `CONTRACT_CLI_001.md` into its own frozen contract. Give the consumer list, the coupling cost of leaving it, and a recommendation. A1 decides.
6. **Hook-recursion and launch-environment constraints for A2-TRUST** — what the Claude-session fallback must respect so it cannot recursively load Receipts hooks.
7. **Skill inventory** — the five MVP skills, each with purpose, invocation surface, and the exact honest wording constraints on its output.
8. **Hook latency budget** — a per-event budget with the measurement method, defined before implementation rather than discovered after.
9. **Proposed A3 task decomposition for the M0 CLI surface and M3 integration** — plugin manifest, hooks.json packaging, input normalization, decision encoding, L1 gate, L2 gate, skills, permission configuration as **separate atomic tasks**, each `NOT_ISSUED` with unmet preconditions. Permission configuration must be separate from hook packaging.

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
- your seven `build-control/a2/claude-integration/` status files, populated

**Return no code. Issue no A3 task.** Do not create an A3 implementation branch or worktree, do not modify product source, and do not commit implementation work. Do not create, replace, rebase, rename, or move your A2 integration worktree — it is provisioned or validated by the currently active A1, and you verify it rather than manage it. Report to the currently active A1.
