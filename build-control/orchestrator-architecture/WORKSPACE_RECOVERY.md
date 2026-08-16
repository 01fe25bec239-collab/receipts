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

# WORKSPACE_RECOVERY

## The scenario (§55)

A RUNTIME-A3 dies mid-task with uncommitted work: process crash, provider outage, session exhaustion, host closed, machine restart.

## Governing rule

> **Do not automatically treat partial work as valid.**

Uncommitted work has passed no check and no audit. It may be half a refactor. Auto-resuming it would mean building on a state nobody verified — and the resulting commit would carry provenance implying otherwise.

## Captured state

```
WorkspaceRecoveryRecord {
  task_id, attempt_id, worktree_path, branch,
  head_sha, start_sha,
  dirty_diff_ref,          # captured, not applied
  modified_files[], untracked_files[],
  last_checkpoint_id?,
  executed_checks[],       # what actually ran before the crash
  crash_classification,    # from classifyFailure
  captured_at
}
```

Capture happens **before** anything else touches the worktree. A recovery that first cleans up destroys the evidence it was meant to preserve.

## Checkpoints

Periodic snapshots during execution: commit-or-snapshot, files changed, checks executed so far, a short progress note. They convert "unknown partial state" into "known state as of checkpoint N" — the difference between salvage and guesswork.

## Replacement A3 chooses explicitly

The replacement receives the recovery record and must **choose**, recording the choice and its reason:

| Option | When | Risk |
|---|---|---|
| `RESET_TO_LAST_ACCEPTED` | Partial work is incoherent, unclassifiable, or small | Rework — cheap and safe |
| `CONTINUE_FROM_CHECKPOINT` | A checkpoint exists and is coherent | Moderate; checkpoint state is known but unaudited |
| `INSPECT_AND_SALVAGE` | Substantial coherent work exists | Highest; requires explicit justification |

**MVP default: `RESET_TO_LAST_ACCEPTED`.** Rework costs one attempt; silently building on broken partial state costs a wrong result that passes review because the reviewer only sees the final diff. The safest default is the one that fails visibly.

Whatever is chosen, the resulting work is audited normally — no recovery path shortcuts A4.

## Orphan detection

A task `IN_PROGRESS` past its expected window with no handoff and an expired lease is orphaned. Recovery: capture state, classify, then apply the matrix. If commits exist but no handoff, the work is **unattested** — no agent has stated what it did or tested — so it may not go to A4. It is either reset or salvaged under a fresh attempt.

## Never

Never auto-commit dirty work as if complete. Never mark a crashed attempt `ACCEPTED`. Never delete a worktree before capture. Never present recovered work as reviewed.
