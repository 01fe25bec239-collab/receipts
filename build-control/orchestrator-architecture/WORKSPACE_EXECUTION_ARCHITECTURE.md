<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# WORKSPACE_EXECUTION_ARCHITECTURE

## Git topology (§63)

```
main
├── runtime-a2/auth
│    ├── runtime-a3/auth-001
│    └── runtime-a3/auth-002-r1
├── runtime-a2/payments
│    ├── runtime-a3/pay-001
│    └── runtime-a3/pay-002
└── runtime-a2/ui
     └── runtime-a3/ui-001
```

| Level | Branch | Lifetime | Created by |
|---|---|---|---|
| Integration baseline | `main` | permanent | user's repo |
| Workstream | `runtime-a2/<workstream>` | manager's tenure | orchestrator (A1 authority) |
| Task | `runtime-a3/<task-id>` | one attempt | orchestrator (A2 authority) |

Repair attempts get their own branch (`-r1`, `-r2`), so every rejected attempt stays independently inspectable.

## Worktree isolation ≠ security (§65, I-11)

> **Git worktrees provide workspace isolation, not security isolation.**

A worktree stops two agents editing the same checkout. It does **not** stop an agent reading elsewhere on the filesystem, running arbitrary commands, or reaching the network. Security comes from the host/runtime sandbox (A-07, A-12) and OS boundaries. No document, message, or status string may describe a worktree as a sandbox.

## Workspace lifecycle

```
provision   validate base SHA → create branch → create worktree → verify clean
execute     adapter runs the attempt inside the worktree, under sandbox policy
checkpoint  periodic: commit or stash-equivalent snapshot + state record
finalize    final commit = review anchor
recover     on crash: see WORKSPACE_RECOVERY.md
teardown    on accept/cancel: remove worktree; retain branch per policy
```

## Orchestrator owns worktree creation

A deliberate reversal of historical ADR-001, whose premise (an evidence layer has no business creating workspaces) no longer holds for a product whose core job *is* workspace lifecycle. On Claude Code this uses the `WorktreeCreate` hook, which per A-03 replaces default git logic and must return the created path; elsewhere the orchestrator creates worktrees directly. Full reasoning in `ADR_IMPACT_MATRIX.md`.

Because the hook sits on an interactive path, it must be correct and fast, and must fall back to a plain `git worktree` if the core is unreachable.

## Process execution

Explicit argv, never a shell string. Resolved executable, realpath-validated working directory inside the worktree, minimal allowlisted environment, orchestrator-owned timeout (terminate, then bounded kill), separate bounded stdout/stderr capture stored by reference with digests.

These rules are carried over from the historical runner design, which got them right; nothing about the new product makes shell-string execution safer.

## Write-scope verification

Post-hoc diff verification against `allowed_write_paths` at handoff. This is the reliable layer: per A-08, path permission rules do not constrain subprocesses, so detection at the boundary is what actually holds.

## Concurrency

Parallel worktrees per workstream, admitted only on disjoint write-sets (`CONCURRENCY_MODEL.md`).

## Cleanup

Accepted: worktree removed, branch retained until workstream integration. **Rejected: branch and commits retained permanently** — evidence of what the repair changed. Abandoned: pruned per `WORKSPACE_RECOVERY.md`.
