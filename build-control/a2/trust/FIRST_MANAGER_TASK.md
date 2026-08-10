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

# FIRST_MANAGER_TASK — A2-TRUST

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

1. **Threat model** — assets (ledger, recipes, policy, approvals, admissions, provider credentials), actors (human user, Claude Code session, worker subagent, review provider, local process, repository content), trust boundaries, and the attacks explicitly **out of scope** for MVP. The out-of-scope list is the most valuable part; write it honestly.
2. **Enforcement-scope audit v0** — every enforcement claim the architecture makes, with the surface it applies to, the mechanism, the test that will prove it, and whether it is L1, L2, L3, or explicitly deferred L4.
3. **Deny / fail-direction requirements package for A2-CLAUDE-INTEGRATION** (former `DR-005`) — exact deny, fail-open, and fail-closed requirements plus the negative fixtures they must pass. Precise enough to test mechanically. This is your highest-leverage early deliverable and it is needed long before your own implementation wave.
4. **`OI-003` proposal** — the exact frozen `claude -p` fallback invocation: full argv, session isolation, structured output mechanism, read-only tool constraints, hook-recursion prevention, and preservation of the intended local authentication path. Include the threat model and at least two rejected alternatives. **Nominate the separate A4 security-review session and note that A1 signs off, not you.**
5. **`OI-004` joint test plan** with A2-CLAUDE-INTEGRATION for permission deny-rule verification against the current Claude version.
6. **`OI-005` joint proposal** with A2-VERIFICATION, focused on why an agent cannot manufacture approval.
7. **Current-documentation re-verification** for Codex non-interactive mode and CLI reference, and for `claude -p` headless, JSON output, and structured output. Record URL, access date, exact flags. Confirm or refute `codex exec`, `--sandbox read-only`, `--json`, `--output-schema`, `-o/--output-last-message`, `--ignore-user-config`, `--ignore-rules`, `--skip-git-repo-check`, and the deprecated status of `--full-auto`. Report divergence to A1.
8. **`ReviewProvider` minimality statement** — the four operations and an explicit list of extensions you will refuse, each mapped to the invariant it would violate.
9. **Override specification pack** — record fields, the human-only mechanism, fingerprint scoping, break-glass behavior, the rendering rule, and the exact prohibited renderings, with a negative test for each.
10. **Integrity signal specification** — test-change detection, test-count delta, deletion policy, include-test-diff behavior, and precisely what each signal does and does not imply.
11. **Prompt-injection-safe context rules** — exact rules for any Receipts-produced text reaching a model, with compliant and non-compliant examples.
12. **Family-separation test plan** — how you will prove, in both directions, that deterministic and review evidence cannot substitute for each other.
13. **Proposed A3 task decomposition for M4 and M5** — Codex provider, findings parsing, provider resolution and downgrade, Claude fallback, integrity signals, override, security suite as **separate atomic tasks**, each `NOT_ISSUED` with unmet preconditions. The Claude fallback must be separate from the Codex provider; override must ship with its guard tests.

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
- your seven `build-control/a2/trust/` status files, populated

**Return no code. Issue no A3 task.** Do not create an A3 implementation branch or worktree, do not modify product source, and do not commit implementation work. Do not create, replace, rebase, rename, or move your A2 integration worktree — it is provisioned or validated by the currently active A1, and you verify it rather than manage it. Report to the currently active A1.
