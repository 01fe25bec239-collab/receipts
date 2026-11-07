# Runtime failure classes

`classifyFailure` maps a failure to one of the twelve frozen classes only from
evidence that establishes it. Anything else is `UNKNOWN`, which is a legitimate
answer, not a fallback for a guess. Provider prose, exit codes and stream
contents never select a class by text matching.

No real recorded Claude or Codex CLI failure capture exists. The repository has
no provider fixture directory, and the 2026-11-07 reconciliation found no
sanitized provider recording in the private evidence store either. Capturing
one requires running a provider CLI under an execution context that A1 has
dispositioned; that live gate is NOT RUN (see `live-proof-plan.md`). Until it
runs, every provider-specific class below stays a documented `UNKNOWN`.
Synthetic test text is never cited as provider evidence, and failures are never
induced (no quota exhaustion, outage, crash, safety incident or bad credential).

Passively observed CLIs on the build host (version only, never exercised):
Claude Code 2.1.314 and codex-cli 0.166.0 on 2026-10-30; Claude Code 2.1.319
and codex-cli 0.167.0 on 2026-11-07.

## Evidence matrix

| Class | Basis | Evidence | Status |
| --- | --- | --- | --- |
| `RATE_LIMITED` | Documented UNKNOWN: no recorded capture. "429" or rate-limit text stays UNKNOWN. | none | UNKNOWN |
| `SESSION_EXHAUSTED` | Documented UNKNOWN: no recorded capture. Claude `error_max_turns` and `error_max_budget_usd` are caller budgets, not provider session exhaustion. | none | UNKNOWN |
| `AUTH_REQUIRED` | Documented UNKNOWN for failed attempts: no recorded capture. Claude `auth status` exit 1 is the separate `RuntimeAuthStatus::AuthRequired` observation, not an attempt failure. | none | UNKNOWN |
| `PROVIDER_DOWN` | Documented UNKNOWN: no recorded capture. | none | UNKNOWN |
| `TIMEOUT` | Typed Workspace evidence: terminal cause `TimedOut`, or a start that timed out before READY. | `claude_adapter_tests` / `codex_adapter_tests` `cancellation_timeout_drop_and_policy_refusal`; `failure_classification_tests` | Typed lifecycle (synthetic CLI) |
| `SANDBOX_DENIED` | Documented UNKNOWN: no recorded capture. Sandbox and permission flags are requests, not proof of enforcement, and never select protection. | none | UNKNOWN |
| `SAFETY_CHECK_PENDING` | Documented UNKNOWN: no recorded capture. Safety text never maps to `RATE_LIMITED`. | none | UNKNOWN |
| `POLICY_BLOCKED` | Adapter refusal before launch: unsupported mandatory policy, unsupported capsule, workspace state or capsule/workspace task, worktree, branch or start-revision mismatch; and a Workspace protected-launch refusal before any worker exists (unsupported host, invalid config, custody, binding, identity exhaustion, workspace state). Never `RATE_LIMITED`. No provider policy text maps here. | adapter refusal tests; `protected_launch_tests::{capsule_task_must_match_a_task_bound_workspace, protected_refusals_classify_conservatively}`; hosted `hosted_protected_cancel_drop_and_refusals_never_fall_back` (NOT RUN) | Typed refusal |
| `RUNTIME_CRASH` | Documented UNKNOWN: Workspace controller failure does not imply a runtime crash, and no recorded crash capture exists. | none | UNKNOWN |
| `INVALID_OUTPUT` | Documented UNKNOWN: protocol parse or truncation errors report availability only. Truncation is a capture limit, not proof of invalid output. | none | UNKNOWN |
| `USER_CANCELLED` | Typed evidence: Workspace terminal cause `Cancelled` and the adapter's `cancel` with `UserRequested` was the accepted cancellation. | adapter cancellation tests; hosted protected cancel case (NOT RUN) | Typed lifecycle (synthetic CLI) |
| `UNKNOWN` | Everything else, including nonzero exits, Claude `is_error` results, Codex `turn.failed` or `error` events, unproven protected cleanup and unrecognized protected-launch errors. | `process_and_provider_evidence_remain_independent_and_errors_hide_payloads` | Default |

"Typed lifecycle" rows rest on Workspace's own terminal evidence driven by
synthetic CLIs. They prove the classification path, not that a real provider
CLI produces that condition in the same way.

## JSONL blank lines (N11)

The shared JSONL reader rejects blank records. No recorded CLI output shows a
blank line, so tolerance is not added. Only an applicable genuine
version-bound recording can change this; the prepared live cases record a
blank-segment count for that purpose. One capture would support at most that
CLI version, not universal behavior.
