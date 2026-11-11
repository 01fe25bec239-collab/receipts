# Runtime observations and lifecycle controls

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
is recorded as Timeout. An observed Claude error_max_turns under a configured
native cap records BudgetExhausted. Superseded, BudgetExhausted and
SafetyInterruption do not guess a provider failure classification.

Claude's optional turn cap and structured schema require complete, successful,
unambiguous help evidence for --max-turns and --json-schema. That evidence uses
the same configured TTL. The native flags enforce the requested values; neither
constraint is translated into prompt instructions. Capsule turn budgets require
a positive policy cap no larger than the capsule budget. Invalid schema JSON,
missing controls and unsupported network/filesystem/sandbox constraints refuse
before worker launch. Codex continues refusing turn/schema constraints at the
adapter boundary; no unsupported enforcement is claimed. Workspace owns process
timeouts, output capture, cancellation and cleanup for both providers.

Session persistence is off by default. With explicit opt-in, an eligible Claude
task may issue an opaque ticket after successful process and provider collection.
`resume_with_policy` requires that ticket plus current task, workspace and policy
and fresh admission by the caller. Tickets bind the adapter instance, exact
workspace and task, time/turn ceilings and schema. Tickets are single-use and are
consumed before launch, including uncertain launch failures. Foreign, replayed,
widened or disabled-policy continuations refuse. Changing the admitted child
environment invalidates previous tickets. Tickets are in-memory continuation
evidence, not a durable recovery mechanism. ID-only RuntimeAdapter::resume stays
unsupported because a historical ID cannot supply current policy authority.
Reviewers remain non-resumable, including read-only task sessions. Claude plan
mode remains reduced isolation, never proof of filesystem confinement.

`conformance::run_conformance` is a reusable asynchronous lifecycle control for
any adapter with the existing stream and result bindings. It checks start and
refusal, stream termination, collection, cancellation outcomes and classification.
Its verifier runs after handle cleanup and must inspect actual process/filesystem
facts, including reviewer writes and structured-output contents. Unknown adapters
must be run under an external deadline. A verifier failure fails the report;
provider prose or a clean exit cannot supply isolation proof. Existing Runtime
controls also exercise actual wakeups, partial records, terminal ordering,
truncation, owned process cleanup and protected reviewer refusal.

Real CLI qualification is separate. No model invocation or installed-provider
read-only qualification was run for this change: no eligible admitted live path
was established. The existing live proof plan and opt-in controls remain the
qualification procedure. Darwin cannot run the Linux protected-launch witness;
Codex reviewer write refusal still needs its admitted real-CLI witness there.
Synthetic tests do not qualify a provider or establish OS confinement. Independent
A3 and applicable Q06 must assess the exact final commit before acceptance.
