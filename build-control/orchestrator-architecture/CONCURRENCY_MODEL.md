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

# CONCURRENCY_MODEL

Parallel RUNTIME-A3 execution is first-class — and is the main reason a control plane is worth having.

```
RUNTIME-A2-AUTH
├── A3 #1 → provider P, frontier model   worktree wt-1
├── A3 #2 → provider Q, frontier model   worktree wt-2
└── A3 #3 → provider R, frontier model   worktree wt-3
```

Different providers concurrently is normal, not exceptional: it is how the system converts three quotas into three times the throughput.

## Admission conditions

Concurrency is permitted only when dependencies allow, **write ownership does not collide**, global budget permits, provider limits permit, and workspace isolation is valid.

## Write-collision detection

Every capsule declares `allowed_write_paths`. Before dispatch the scheduler intersects the candidate's write-set with every running task's write-set.

```
overlap(A, B) = ∃ path p : p matches A.allowed_write_paths ∧ p matches B.allowed_write_paths
```

On overlap, exactly one resolution — never "both, carefully":

1. **Serialize** — add a dependency edge.
2. **Defer assembly** — move the shared write into a later single-writer task.
3. **Assign a canonical writer** — one task owns the file; the other consumes it.
4. **Re-cut the boundary** — split so write-sets are disjoint.

Two agents editing one file in two worktrees produces a merge conflict a third agent must resolve without context. Preventing it is cheaper than resolving it.

## Limits

| Limit | Default | Why |
|---|---|---|
| Global concurrent A3 | 4 | Beyond this, review and integration become the bottleneck |
| Per-workstream | 3 | Keeps one workstream from starving others |
| Per-provider | from adapter/quota state | Respect observed provider limits |
| Concurrent A4 | 4 | Audits are cheaper; they should not queue behind implementations |
| Per-goal cost/hour | configurable | Prevents runaway spend |

Defaults are conservative on purpose. More agents is not the objective; highest-quality accepted result for justified compute is (§84).

## Cancellation

Triggers: dependency invalidated; budget exhausted; user cancel; goal blocked; superseded by a repair; provider terminally failed.

Cancellation always: signals the runtime, waits for a bounded grace period, checkpoints the workspace, records the partial state as `CANCELLED` (never `ACCEPTED`), and preserves the branch for inspection. Partial work is never silently promoted.

## Timeouts

Per task from the capsule; per attempt at the adapter; a global goal budget above both. A timeout is a classified failure with preserved evidence, not a disappearance.

## Backpressure

When a provider degrades or rate-limits, the scheduler reduces that provider's concurrency rather than failing tasks. Sustained degradation across all eligible providers surfaces as `BLOCKED` with a reason — the honest answer when there is genuinely nowhere to run work.
