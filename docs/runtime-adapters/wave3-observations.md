# Runtime observations

ClaudeAdapter and CodexAdapter expose bounded version/help installation probes
through `provider(workspace, timeout)`. Results use a monotonic TTL (300 seconds
by default, configurable through `with_probe_ttl`). An expired cache is reprobed
on the next provider request. `capabilities()` returns UNKNOWN after expiry;
it never performs hidden subprocess work. Failed probes replace old evidence.
Installation does not establish provider health, authentication, usage, quota,
model inventory or dispatch eligibility. Those observations remain UNKNOWN.
A missing required base invocation flag reports VERSION_UNSUPPORTED; this means
unsupported by this adapter, not an assertion about all uses of that CLI.
Trusted Model Intelligence remains responsible for durable registry writes.

Cancellation retains the first reason accepted by Workspace. A runner timeout
is recorded as Timeout. Superseded, BudgetExhausted and SafetyInterruption stay
distinct reasons without guessing a provider failure classification. Mandatory
unsupported constraints still refuse before launch. Reviewers remain fresh and
non-resumable; Claude plan mode is reduced isolation, not a filesystem sandbox.

This change does not complete RA-W3-1/2. Reusable lifecycle controls, supported
turn-budget enforcement and eligible policy-bound resume remain outstanding.
Existing asynchronous wakeup, terminal ordering, cleanup and reviewer controls
remain in the Runtime tests. Real CLI qualification is separate: no live model
invocation was authorized by an eligible path in this change, and no live
read-only or plan-mode qualification is claimed. Independent A3 and applicable
Q06 must assess the exact final commit before acceptance.
