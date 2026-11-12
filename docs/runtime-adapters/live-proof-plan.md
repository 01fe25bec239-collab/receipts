# Live-proof plan (prepared, NOT RUN)

This plan needs A1's separate disposition of the execution context before any
case runs. Technical login is not policy eligibility: the frozen provider
policy records Anthropic subscription credentials as disallowed for a
third-party external worker, and an OpenAI consumer subscription as
needs-review for a paid third-party external worker. Running a case is a
compatibility observation in the dispositioned context, nothing more.

## Executables (passive observation, 2026-11-12)

| Runtime | Path | Version |
| --- | --- | --- |
| Claude | `/Users/omkar/.local/bin/claude` | Claude Code 2.1.322 |
| Codex | `/Users/omkar/.local/bin/codex` | codex-cli 0.167.2 |

Previous observation (2026-11-07): Claude Code 2.1.319 and codex-cli 0.167.0. This is version drift, not compatibility evidence.
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

## Qualification cases (prepared, NOT RUN; 2026-11-12)

These are additional to L1/L2. They use `ChildEnvAuthMode::ApiKey`, not the
subscription environment above. A1 must disposition the exact provider,
account, model, API-key external-worker use and runner before execution.
An Anthropic subscription is not an eligible substitute. The operator supplies
`ANTHROPIC_API_KEY` or `OPENAI_API_KEY` outside the evidence record; do not
print, capture, inspect or copy its value. The child also receives the base
names listed above, filtered absolute `PATH`, and the corresponding optional
`CLAUDE_CONFIG_DIR` or `CODEX_HOME`. No proxy or extra names are admitted.
An API key being present does not establish eligibility or network access.

Use a clean exact reviewed SHA and an empty, operator-owned mode-0700 evidence
directory outside the repository, separately for each invocation. Recheck
`--version` on each actual executable immediately before the run, record its
path, version, OS, SHA and date without credentials, and stop on drift pending
an updated plan. The observations above are macOS installations: they do not
assert any Linux installation version. The live tests are ignored and require
both the named opt-in and explicit absolute executable path; each case has a
180-second deadline plus five-second grace. Do not retry a case automatically.

* Claude: an eligible API-key provider context on a disposable runner where the
  CLI and its config are usable, with `/usr/bin/git`, Rust and an unprivileged
  test process. Plan runs as an ordinary read-only task and reports reduced
  isolation, not a sandbox. A separate ordinary task asks for a native JSON
  schema and two-turn limit. A persistent implementation task starts once and
  resumes once using its bound single-use ticket; the harness also verifies a
  read-only resume and ticket reuse are refused. FIRST/SECOND and schema
  outcomes are compatibility observations, not a guarantee of model behavior.
  The resume case intentionally enables provider session persistence: its
  session data may remain under the operator's CLI home. Do not copy or remove
  that data; report its existence and leave deletion to its owner.

  ```bash
  RECEIPTS_QUALIFY_CLAUDE=1 RECEIPTS_LIVE_CLAUDE_EXE=<absolute-claude-path> \
  RECEIPTS_LIVE_EVIDENCE_DIR=<private-absolute-dir> \
  cargo test --locked -p receipts-runtime-adapters --lib \
    qualification_tests::live_claude_qualification -- --ignored --exact --nocapture --test-threads=1
  ```

* Codex: a disposable, non-container Linux VM with cgroup v2 delegation and
  the Workspace protected-custody host anchors administered by the operator.
  Run the already-built test binary as the authorized host controller (root),
  with controller UID/GID 61001, worker GID 61002 and unused worker UIDs
  62000–62063, as required by the existing hosted fixture. The CLI and its
  dependencies and eligible API-key context must be usable by the isolated
  worker; the operator must arrange that without exposing a credential in
  evidence. `RECEIPTS_RA_HOSTED_CGROUP` names the absolute, dedicated,
  delegated cgroup. The host setup and teardown must establish and verify
  owned custody anchors, cgroup emptiness, process reaping and cleanup; the
  existing `.github/scripts/ra-w1-completion-linux/run.sh` is a synthetic-only
  reference and does not invoke this case or forward provider credentials.
  Do not run this on a workstation or adopt/delete existing custody objects.
  Build unprivileged with `cargo test --locked -p receipts-runtime-adapters
  --lib --no-run`, then invoke that exact test binary in the prepared host
  context with these names:

  ```text
  RECEIPTS_QUALIFY_CODEX=1
  RECEIPTS_LIVE_CODEX_EXE=<absolute-linux-codex-path>
  RECEIPTS_LIVE_EVIDENCE_DIR=<private-absolute-dir>
  RECEIPTS_RA_HOSTED_CGROUP=<absolute-delegated-cgroup>
  OPENAI_API_KEY=<operator-provided eligible context; never record the value>
  <test-binary> qualification_tests::live_codex_protected_reviewer --ignored --exact --nocapture --test-threads=1
  ```

  The case provisions a review workspace at the exact fixture commit. PASS
  requires protected collection, a completed protocol, a nonzero execution of
  the exact bounded write command with permission/read-only filesystem denial,
  unchanged tracked bytes, unchanged Git status/head, successful custody
  release and fixture removal. A model refusing to attempt the command, an
  unrecognized output shape, truncated stdout or an unrelated command failure
  is inconclusive and fails the test. If custody is unproven the fixture is
  retained; the operator must preserve and investigate it, not force-delete it.
  A PASS is limited to this run, host and CLI, not a universal sandbox claim.

Each started and collected case saves bounded raw stdout/stderr privately,
with separate head/tail files on truncation. Summaries contain only case,
verdict and cleanup status; a start or collection refusal can lack raw streams.
Create-new writes prevent overwriting existing evidence. A failed or interrupted
run is retained, never relabeled as success. Operator inspection and any
sanitization follow the output handling above. Synthetic `qualification_tests`
exercise success, malformed/silent failure, write-refusal recognition, private
create-new evidence and retained-on-unproven-cleanup behavior offline; they do
not establish real provider behavior or Linux custody.
