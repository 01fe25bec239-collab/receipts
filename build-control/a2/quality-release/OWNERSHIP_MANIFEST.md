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

# OWNERSHIP_MANIFEST — A2-QUALITY-RELEASE

**Authoritative for write authority.** `A2_OWNERSHIP_REMAP.md` is authoritative for manager identity and ownership across the program; this file is its per-manager projection. No two managers hold overlapping write ownership. Where a committed orchestration document names a superseded manager for one of these paths, this file and the remap win.

**Nothing in `FILES/DIRECTORIES OWNED` exists yet** unless it is listed as committed at `CONTRACT_FREEZE_SHA` (`2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`). Source paths are *planned* ownership, created only when the currently active A1 authorizes your implementation wave.

## FILES / DIRECTORIES OWNED

**Planned, created only on wave authorization:** `eval/**`, `README.md`, `docs/**` (ARCHITECTURE, TRUST_MODEL, ENFORCEMENT_SCOPE, HOOK_MAPPING, PROVIDERS, EVALUATION, installation guide, development guide, demo instructions, release checklist), collision-check and release evidence records, and `build-control/a2/quality-release/**`.

You own the benchmark **oracles** inside `eval/**`. No other manager may modify them, and you may not modify them after a campaign starts.

## FILES / DIRECTORIES CONSUMED

`contracts/CONTRACT_CLI_001.md`, `CONTRACT_EXPORT_001.md`, `CONTRACT_POLICY_002.md`, `CONTRACT_CORE_003.md`, `CONTRACT_RUNNER_002.md`, `CONTRACT_REVIEW_002.md`, `CONTRACT_OVERRIDE_001.md`, and A2-TRUST's enforcement-scope audit. Read and obey; never edit.

## FILES / DIRECTORIES REFERENCE-ONLY

Every remaining contract, every orchestration file, `schemas/SCHEMA_PLAN.md`, and the superseded 8-manager package if it is committed to the repository (historical only, never authority).

## FILES / DIRECTORIES FORBIDDEN TO MODIFY

Everything outside `eval/**`, `README.md`, `docs/**`, and `build-control/a2/quality-release/**`. Specifically: `Receipts_Final_Architecture.md`; `orchestration/**`; `architecture-decisions/**`; `contracts/**` (all 21 plus the index); `schemas/**`; `src/**`; `bin/**`; `.claude-plugin/**`; `hooks/**`; `skills/**`; `agents/**`; `A2_CONSOLIDATION_DECISION.md`; `A2_OWNERSHIP_REMAP.md`.

A product defect that blocks measurement is a `DEPENDENCY_REQUEST`, never a local fix.

## Shared-file protocol

There is no shared write ownership in this topology. If you believe a file needs two writers, that is a design smell and an A1 escalation, not a local arrangement. Resolve every cross-boundary need with a `DEPENDENCY_REQUEST`, a `CONTRACT_CHANGE_REQUEST`, or an `ARCHITECTURE_DEVIATION_REQUEST`.

Solving a dependency by editing another manager's files is a boundary violation and an automatic A4 `REJECT`, regardless of how correct the edit is.

## Branch and worktree model

Two kinds of worktree exist, and they have **different authorization semantics**. Confusing them is a governance failure, not a naming preference.

### A2 integration worktree — long-lived, A1-provisioned

- Branch: `a2/quality-release`
- Path: supplied as `manager_worktree_path` in your bootstrap handoff
- Created from: `A2_START_SHA`
- **Created or validated by the currently active A1**, then handed to you. You **verify** it against the handoff; you do not provision it, rename it, rebase it, or move it.
- Long-lived: it persists for your whole tenure and survives individual implementation waves.
- Its existence does **not** authorize implementation. You may hold a verified integration worktree and still have `a3_implementation_authorized: false`.

### A3 implementation worktree — short-lived, task-scoped

- Branch: `a3/quality-release/<task-id>`, one worktree per active implementation task
- Created only **after** your implementation wave is authorized, and only for a specific bounded A3 task
- Destroyed when the task is accepted or abandoned
- A4 reviews the immutable A3 commit and does **not** share the A3 working tree

### Rules that apply to both

Worktrees are **workspace isolation, not security isolation** (invariant 12). Never describe one as a sandbox.

Git worktrees used here are ordinary **development-process** mechanics performed by the operator, the active A1, or you. This is entirely separate from Receipts product runtime behavior: under ADR-001, the Receipts product installs no `WorktreeCreate` and no `WorktreeRemove` hook and does not own worktree creation. The two must never be conflated in any artifact you produce.

**State at the time this package was generated (A1-BOOTSTRAP, pre-installation):** no A2 branch, no A2 worktree, no A3 branch, no A3 worktree, no commit, no push existed. That is a historical snapshot, not a rule you inherit. By the time you read this at initialization, your **A2 integration** branch and worktree exist — the currently active A1 provisioned or validated them and handed them to you. The standing rule is: verify that workspace, never create or alter it; and create no **A3 implementation** branch or worktree, and commit no implementation work, until your implementation wave is authorized.
