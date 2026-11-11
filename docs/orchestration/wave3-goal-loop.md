# Goal continuation

`CoreFacade` exposes host-neutral `start_goal`, `tick`, `resume_goal`,
`snapshot` and explicit `retry_failed` operations. A tick is one re-entrant
step; a host drives further steps while the returned state is INCOMPLETE.
Snapshot includes the durable evaluation state, exact gaps and evaluated graph
version/SHA. Resuming re-evaluates current evidence, including a previously
complete goal.

Start uses the existing compiler. Missing or unconfirmed measurable criteria
return its clarification outcome. The trusted compilation sink stores the exact
spec bytes and genesis idempotently before the goal definition is created.
Every subsequent snapshot verifies the original spec digest. Adopted criteria,
FREE runtime selection and resource limits are retained in the immutable
continuation artifact referenced by admitted State progress.

The driver recovers/rehydrates through the trusted consumer, loads its current
State-bound graph and attempt/result ledger, applies completion/review/repair
proposals through the existing evidence-backed mutation validator, then refreshes
observations. An admitted epoch change takes a separate no-dispatch step before
readiness is re-derived. The shared TaskDag and scheduler check dependencies,
D11 admission, budgets, concurrency and write collisions. Routing must match the
admitted provider/runtime and pinned FREE runtime. One admitted node is started
per tick; later ticks may fill allowed same-runtime concurrency.

Only a newly Applied State reservation can invoke launch. The driver publishes
dispatch transitions before the external boundary and records RUNNING only for
a confirmed launch. Replay reads existing launch evidence and grants no relaunch.
A failed call after reservation leaves the boundary uncertain. Uncertain and
not-launched outcomes remain explicit; safe recovery/retry requires evidence.
An unresolved reservation cannot be replaced, including after a restart whose
latest continuation predates that reservation.

FREE quality gates execute declared task checks at the exact code SHA through
the check consumer. They do not request cross-provider A4. Completion and review
consumers may propose results/verdicts, but Core applies the existing attempt,
code-domain and admitted review evidence checks. An explicit failed-node retry
can expand only a retained rejected review with attached failure evidence; the
fresh REPAIR/REVIEW round and accepted history retain their graph identities.

Goal completion runs declared criterion checks only after mechanical gates pass.
An empty ready set cannot complete by itself. Missing checks, findings, epoch,
security or integration evidence refuse completion. Semantic criteria retain
explicit HUMAN_REQUIRED gaps. A changed graph/version/epoch/integrated SHA between
evaluation and recording refuses COMPLETE. Goal/task lifecycle events carry their
actual graph version; trusted consumers persist events and evaluation records
idempotently through admitted writes.

Dispatch counts are charged atomically by the setup State port. Wall, cost and
repair limits are immutable continuation data; exhaustion or unknown capped cost
blocks new work with a reason. Trusted usage observations must include pending
reservations and running work. This is not provider monetary accounting. Limits
are not silently increased on resume; a changed limit requires a separately
admitted disposition, which this API does not invent.

`GoalConsumerPortV1` is the trusted composition boundary, not worker authority.
Its snapshot supplier must rebuild attempt classification and review/repair
ledger facts from admitted State evidence; its compilation, artifact, mutation,
event and evaluation sinks must be immutable/idempotent and refuse foreign or
missing evidence. Its routing adapter uses the existing task/repair capsule
builders. Orchestration imports no Runtime, Routing, Review or Host implementation.
Loop tests use controlled consumers; real State witnesses cover reservations,
continuation references, epoch CAS and reopen, including disk-backed artifacts.

Unsafe orphan continuity stays a recovery refusal. Semantic inference, provider
monetary accounting, production adapters and full product command wiring remain
later boundaries. Independent A3, applicable Q06 and A1 acceptance are separate
from these library controls.
