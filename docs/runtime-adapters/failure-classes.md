# Runtime failure classes

`classifyFailure` maps a failure to one of the twelve frozen classes only from
evidence that establishes it. Anything else is `UNKNOWN`, which is a legitimate
answer, not a fallback for a guess. Provider prose, exit codes and stream
contents never select a class by text matching.

No real recorded Claude or Codex CLI failure capture exists yet. Capturing one
requires running a provider CLI under an explicitly admitted
`PERSONAL_LOCAL_MODE` context, which has not been admitted. That live capture
is an open proof gate (NOT RUN). Until it runs, every provider-specific class
below stays a documented `UNKNOWN`. Synthetic test text is never cited as
provider evidence.

Passively observed CLIs on the build host (version only, 2026-10-30): Claude
Code 2.1.314 and codex-cli 0.166.0. These versions were not exercised.

| Class | Basis |
| --- | --- |
| `RATE_LIMITED` | Documented UNKNOWN: no recorded CLI rate-limit capture. "429" or rate-limit text stays UNKNOWN. |
| `SESSION_EXHAUSTED` | Documented UNKNOWN: no recorded capture. Claude `error_max_turns` and `error_max_budget_usd` are caller budgets, not provider session exhaustion. |
| `AUTH_REQUIRED` | Documented UNKNOWN for failed attempts: no recorded capture. Claude `auth status` exit 1 is reported as the separate `RuntimeAuthStatus::AuthRequired` observation, not as an attempt failure. |
| `PROVIDER_DOWN` | Documented UNKNOWN: no recorded capture. |
| `TIMEOUT` | Typed Workspace evidence: terminal cause `TimedOut`, or a start that timed out before READY. |
| `SANDBOX_DENIED` | Documented UNKNOWN: no recorded capture. Sandbox flags are requests, not proof of enforcement. |
| `SAFETY_CHECK_PENDING` | Documented UNKNOWN: no recorded capture. Safety text never maps to `RATE_LIMITED`. |
| `POLICY_BLOCKED` | Adapter refusal before launch: unsupported mandatory policy, unsupported capsule or workspace state. Never `RATE_LIMITED`. No provider policy text maps here. |
| `RUNTIME_CRASH` | Documented UNKNOWN: Workspace controller failure does not imply a runtime crash, and no recorded crash capture exists. |
| `INVALID_OUTPUT` | Documented UNKNOWN: protocol parse or truncation errors report availability only. Truncation is a capture limit, not proof of invalid output. |
| `USER_CANCELLED` | Typed evidence: Workspace terminal cause `Cancelled` and the adapter's `cancel` call with `UserRequested` was the accepted cancellation. |
| `UNKNOWN` | Everything else, including nonzero exits, Claude `is_error` results and Codex `turn.failed` or `error` events. |

## JSONL blank lines (N11)

The shared JSONL reader rejects blank records. No recorded CLI output shows a
blank line, so tolerance is not added. Recorded live output decides this when
the live capture gate runs.
