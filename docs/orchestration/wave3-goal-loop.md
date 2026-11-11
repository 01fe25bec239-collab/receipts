# Goal continuation library

`goal::engine` supplies `start_goal_v1`, `tick_goal_v1`, `resume_goal_v1`
and `snapshot_goal_v1` over the setup's admitted goal persistence interface.
Start uses the existing compiler and returns its criterion clarification outcome.
Trusted composition must persist the compiler's verbatim spec bytes and genesis
idempotently before the goal definition is created.

A tick loads durable progress and asks the trusted loop consumer to reconcile
recovery, rehydration, completions, review and repair. Its prepared dispatch must
come from the existing scheduler, capsule validation and admitted routing at the
current epoch. State rechecks project, graph, epoch, revision, membership and
budget in the reservation transaction. Only Applied calls launch. Replay reads
existing evidence; unresolved launch produces BLOCKED and never launches again.
A launch call failing after reservation leaves the durable uncertainty intact.
A fresh resume has no privileged in-memory ticket.

The continuation supplier stores immutable artifacts before returning a reference
for revision-CAS publication. It must bind graph/goal/epoch, pending reservation
and review identities, runtime selection and budget counters. Reservation charging
and continuation publication are separate transactions: restart must reconcile
attempts even when the latest continuation predates a reservation.

`goal::evaluate` executes declared deterministic criterion checks only after the
mechanical prerequisites hold. The runner must bind results to the exact current
integrated SHA and treat timeout, missing output and execution errors as failure.
Semantic criteria remain named human-required gaps. No semantic inference exists.
An empty criterion set cannot complete. Assessment construction is restricted so
consumers obtain COMPLETE through the evaluator.

`policy::FreePolicyV1` checks a selected eligible host-native provider/runtime and
limits retry eligibility to attached failed/rejected results on rejected nodes.
The trusted caller must retain the selection across continuation and use the
shared mutation engine for retries. Accepted history is read without alteration.

Remaining implementation: concrete goal-owned scheduling/review/repair
continuation, durable policy selection, multidimensional budget accounting,
event emission and start/clarification controls. The loop consumer currently
specifies these obligations; it does not implement them. This is a partial
library change, not completion of OR-W3-1–3 or Wave 4 product composition.
