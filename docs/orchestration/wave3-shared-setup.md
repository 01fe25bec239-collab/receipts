# Shared Wave 3 library interfaces

State migration 24 adds internal append-only goal history, per-dispatch
reservations and launch resolutions, and finalization receipts. Frozen wire
schemas are unchanged. Existing attempt start/finish callers keep their behavior.

## Goal persistence

Core owns `goal::persistence::GoalPersistencePortV1`, implemented by the trusted
`TrustedCoreStore`. The port supports load/create/advance/reserve/resolve and
can be replaced by a controlled fixture. It imports no Runtime type.

`GoalDefinitionV1` binds project, goal, graph, immutable verbatim spec reference
and SHA-256. Its dispatch-count budget is immutable. Revisions, epochs, used
budget and limit use canonical unbounded decimal integers. Each reservation
charges exactly one dispatch. This is not provider monetary accounting.
Continuation references name immutable artifacts; State does not execute them.

`reserve_goal_dispatch_v1` checks expected revision, current project epoch,
current graph version and node membership. It atomically appends attempt start,
reservation and charged goal revision. Different nodes can reserve against
successive revisions while prior nodes run. An unresolved or still-running
launch blocks a replacement of that node. State-compatible ULIDs identify
attempts. Admission binds the live store, operation, project and graph.

Only `Applied` allows the caller to attempt that new external launch. `Replayed`
never permits relaunch. A reservation with no launch resolution means the
external boundary is uncertain after a crash. Reconcile or block before retry;
a database commit proves no exactly-once external launch. A resolution is
immutable and requires a trusted evidence reference. `UNCERTAIN` remains
blocked for escalation. `NOT_LAUNCHED` permits a new, charged reservation;
it does not reuse an attempt ID or erase the earlier history.

`advance_goal_v1` performs revision/epoch CAS while preserving spec and budget.
Reads/replays grant no dispatch, review, semantic completion or recovery authority.
Artifact contents and spec digest verification belong to the trusted content
supplier; State records the binding without fetching bytes.

## Finalization and checkpoint persistence

`finish_with_finalization_v1` commits successful attempt finish and its immutable
receipt together. The receipt binds project, graph, attempt, workspace,
checkpoint, exact final SHA, passing write scope, executed checks and existing
project output refs. Check results must equal the stored checkpoint's checks;
checked SHAs equal the final SHA and results cannot describe failed checks.
Exact retries replay; changed receipts/finish data refuse. Mechanical success
is not review acceptance.

Workspace's `AttemptEvidenceSinkV1` has a trusted adapter over admitted State
writes. It checks checkpoint attempt/project/workspace binding and conflicting
metadata before writing output refs and then checkpoint. An interruption may
leave output metadata without the checkpoint; stable identities make retry
safe. The private-constructor receipt acknowledges persisted metadata only.
Store immutable output files before calling it; it does not verify their bytes.
Do not supply the trusted adapter or store to workers.

## Read-only Host view

`project_persisted_graph_view_v1` produces `ReadOnlyGraphViewV1` with project ID,
the existing `GraphSnapshot` and validated edges in edge-ID byte order. The
single existing projection boundary checks project/graph/current version,
topology and digest before constructing either part. Callers cannot combine
independent edge and snapshot sources. Host tree/economy presentation stays in
Host. The original snapshot projection remains available.

## Bounded cancellation preparation

Ordinary `LiveProcessAttempt` and `ProtectedAttemptView` expose
`prepare_cancellation` and `cancel_prepared`. A preparation contains bounded
output, the existing process deadline and private attempt identity. It is not
process identity after restart and carries no durable-write authority.

The caller captures workspace evidence and persists it through the trusted sink
outside lifecycle locks, then requests cancellation with the preparation.
`CancelAccepted` establishes that this explicit cancellation won the terminal
claim after the capture. `ForeignAttempt`, `PreparationExpired` or
`AlreadyTerminalOrTerminating` cannot establish that ordering. Retain captured
evidence and reconcile a lost race. Running snapshots have no full-stream digest;
content-address stored retained bytes without claiming that they are the whole
untruncated stream.

No controller, timeout or Drop path calls or waits for external persistence.
A stalled or failed sink therefore adds no time to the existing cleanup bounds.
The old launch/cancel/collect path remains available. Runtime owns requested
reasons and asynchronous events, and must deliver accepted output before the
single terminal event. Workspace owns the actual first-winner cause and cleanup.

This split boundary does not guarantee durable workspace capture before timeout,
Drop or controller-failure cleanup. Such a barrier needs additional bounded
persistence mechanics and a durable acknowledgement protocol. It is not ready
for that stronger guarantee. Destructive workspace reset refuses without durable
evidence and verified physical continuity; this setup adds no reset operation.

## Recovery and qualification limits

Recovery defaults to `RESET_TO_LAST_ACCEPTED`. A checkpoint can supply that
target only when verified to represent the exact accepted commit. Preserve
unaccepted checkpoints as evidence. Missing accepted target, unverifiable
process identity or ambiguous continuity retains data and escalates.

No persisted PID, current-target observation, launch resolution or checkpoint
receipt grants signal/reset authority. The existing live child/group ownership
is not a post-restart platform identity. This setup adds no durable process
identity or orphan execution feature. Unsupported continuity remains a refusal.

The isolated Codex Q01 procedure remains pending an authorized installed-host
run. No plugin registration or hook trust change is part of this setup. Q02,
live provider qualification, feature implementations and product wiring remain
separate.
