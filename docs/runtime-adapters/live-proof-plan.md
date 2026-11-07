# Live-proof plan (prepared, NOT RUN)

This plan needs A1's separate disposition of the execution context before any
case runs. Technical login is not policy eligibility: the frozen provider
policy records Anthropic subscription credentials as disallowed for a
third-party external worker, and an OpenAI consumer subscription as
needs-review for a paid third-party external worker. Running a case is a
compatibility observation in the dispositioned context, nothing more.

## Executables (passive observation, 2026-11-07)

| Runtime | Path | Version |
| --- | --- | --- |
| Claude | `/Users/omkar/.local/bin/claude` | Claude Code 2.1.319 |
| Codex | `/Users/omkar/.local/bin/codex` | codex-cli 0.167.0 |

Re-run `--version` immediately before execution. A different version needs a
new plan entry; it is not silently substituted.

## Path and environment

Cases use the ordinary, non-authoritative adapter path on this Mac (protected
launch is unsupported on Darwin), read-only policy (Claude `plan`, Codex
`read-only`), timeout 180 s with 5 s grace, Workspace's default capture limit,
and an explicit `ChildEnvPolicy` in `Subscription` mode. Admitted NAMES only
(values are never printed): `HOME`, `USER`, `LOGNAME`, `TERM`, `LANG`,
`LC_ALL`, `LC_CTYPE`, `TZ`, `TMPDIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
`XDG_CACHE_HOME`, `XDG_STATE_HOME`, absolute `PATH` entries, plus
`CLAUDE_CONFIG_DIR` (Claude) or `CODEX_HOME` (Codex) when set. No API-key name
is admitted in this mode. Saved login is used where the CLI finds it; no
credential file, keychain item, token or value is inspected or copied.

## Cases (one attempt each, no retry)

| Case | What | Expected record |
| --- | --- | --- |
| L1-success | Trivial read-only capsule in a fresh scratch Git worktree: reply OK, change nothing | PASS only for `Completed`, exit 0, no forced kill, no failure class, Claude `Success` / Codex `Completed`, no cancellation, successful Git inspection and an unchanged worktree at its base revision |
| L2-cancel | Long read-only capsule; cancel with `UserRequested` after the first event | PASS only when the first event is a record, the cancellation was accepted (`UserRequested` reason), the cause is `Cancelled` and the class `USER_CANCELLED`, with successful Git inspection and an unchanged worktree. The provider's own termination of an intentional cancel is recorded, not asserted |

Both cases always run once and the test fails if either verdict fails, so a
silent exit, an unknown or invalid result, a missing first record, an
unaccepted cancellation or a failed Git inspection is never a pass. Owned
synthetic executables exercise exactly these verdicts in the ordinary suite
(`live_tests::synthetic_*`); that is harness evidence only, never provider
evidence. Each summary also records truncation, total and retained bytes and,
for N11, `stdout_records` / `stdout_blank_records` / trailing-delimiter facts
of a complete capture (empty output has zero records; the final newline is a
delimiter, not a blank record). A truncated capture reports
`stdout_blank_records=unavailable(truncated)`: its middle is missing, so head
and tail are never joined or counted.

Four provider runs at most (two per CLI). No failure is induced.

Commands, after A1 disposition and with an existing private directory:

```bash
RECEIPTS_LIVE_CLAUDE=1 RECEIPTS_LIVE_CLAUDE_EXE=/Users/omkar/.local/bin/claude \
RECEIPTS_LIVE_EVIDENCE_DIR=<private absolute dir outside the repository> \
cargo test --locked -p receipts-runtime-adapters --lib \
  live_tests::live_claude_ordinary_success_and_cancel -- --ignored --exact --test-threads=1
```

```bash
RECEIPTS_LIVE_CODEX=1 RECEIPTS_LIVE_CODEX_EXE=/Users/omkar/.local/bin/codex \
RECEIPTS_LIVE_EVIDENCE_DIR=<private absolute dir outside the repository> \
cargo test --locked -p receipts-runtime-adapters --lib \
  live_tests::live_codex_ordinary_success_and_cancel -- --ignored --exact --test-threads=1
```

## Output handling and cleanup

Raw stdout/stderr go only to `<runtime>-<case>.{stdout,stderr}` (mode 0600,
create-new) in the private evidence directory; a truncated stream is kept as
separate `.head` and `.tail` fragments instead; the test refuses a directory
inside the repository. The `.summary` line carries no payload. Nothing is
committed. A sanitized fixture may be proposed only after redaction (session,
account, organization and request identifiers, home paths, any token-like
value), a secret scan, provenance (CLI version, date, argv, sanitization) and a
separate A1 disposition. One capture supports that CLI version only.

Scratch repositories and worktrees are removed when each fixture drops.
Claude runs with `--no-session-persistence`. Codex may write session logs under
its own home; record only whether new files appeared, do not copy them, and
leave any deletion to USER.
