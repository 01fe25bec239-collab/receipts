# Q01 isolated Codex qualification runbook

Status: prepared, **not run**. This disposable observer is not the Receipts
plugin and does not qualify native delivery. The adapter stays SUPERVISED until
A1 admits installed-host evidence. The fixture records a fixed marker, not hook
input; it cannot admit work, emit normalized events, or write State. A marker
alone proves neither payload compatibility nor event provenance.

Sources: [OpenAI plugin packaging](https://developers.openai.com/plugins/build/plugins),
[OpenAI hooks and trust](https://learn.chatgpt.com/docs/hooks). CLI 0.167.2 help
was inspected; other versions require rechecking help and fixture compatibility.
Do not use `--dangerously-bypass-hook-trust`. Stop on unexpected paths, other
hooks, unexpected side effects, failed controls, or a changed CLI; preserve
results and report UNKNOWN rather than infer success. Never alter the real HOME,
real Codex configuration, global managed policy, or another registration.

## Preparation and authority

A later user-authorized operator must authorize local marketplace registration,
plugin installation/removal, project and hook trust changes in an isolated
CODEX_HOME, and the limited model call for skill invocation. Authentication may
require an independent user action in the isolated home; do not copy credentials.
If isolation cannot be established without touching real settings, stop. The
managed-policy control requires separately authorized isolated administrative
policy provisioning; if unavailable mark it UNKNOWN, not a pass.

In a terminal from this checkout, with the authorized revision checked out:

```sh
umask 077
Q01_BASE=$(mktemp -d "${TMPDIR:-/tmp}/receipts-q01.XXXXXXXX")
export Q01_BASE
mkdir -p "$Q01_BASE/home" "$Q01_BASE/repo" "$Q01_BASE/evidence"
cp -R plugins/codex/q01-fixture "$Q01_BASE/marketplace"
export CODEX_HOME="$Q01_BASE/home"
git -C "$Q01_BASE/repo" init
python3 "$Q01_BASE/marketplace/check.py"
command -v codex
codex --version
shasum -a 256 "$(command -v codex)"
uname -a
date -u '+%Y-%m-%dT%H:%M:%SZ'
(cd "$Q01_BASE/marketplace" && find . -type f -print0 | sort -z | xargs -0 shasum -a 256) > "$Q01_BASE/evidence/fixture-sha256.txt"
codex plugin marketplace add --help
codex plugin list --help
codex plugin add --help
codex plugin remove --help
codex plugin marketplace remove --help
```

On platforms without `sort -z`, use an equivalent binary-safe hash inventory.
Record executable real path, hashes, OS, UTC, checkout SHA/tree, HOME and
CODEX_HOME paths, help output and a snapshot of real user settings' existence
and hashes (never contents or credentials). Verify the isolated home is empty,
the repo is fresh and contains no inherited `.codex` hooks, and inspect active
system/managed sources; if unrelated hooks or writes cannot be excluded, stop.
Capture output and exit status for every command in private evidence, redacting
secrets. Retain evidence outside the disposable directory before cleanup.

## Discovery, installation and trust

From the separate fresh repo (`cd "$Q01_BASE/repo"`), keep `CODEX_HOME` set and
HOME unchanged. Do not set a project-local marketplace or bypass trust. Record
baseline `codex plugin marketplace list` and `codex plugin list --available
--json`; if network or unrelated sources prevent isolation, stop. Then execute:

```sh
codex plugin marketplace add "$Q01_BASE/marketplace" --json
codex plugin marketplace list
codex plugin list --marketplace receipts-q01-fixture --available --json
codex plugin add q01-observer@receipts-q01-fixture --json
codex plugin list --marketplace receipts-q01-fixture --json
```

Record exact discovery, selectors, outputs, errors and installed cache/data
paths. Verify they are within the isolated CODEX_HOME; do not proceed if the
host writes elsewhere. Open a fresh interactive session in the fixture repo,
review any project trust prompt and trust only this disposable repo if needed.
Inspect `/hooks`: record the fixture definition and untrusted state. Close and
start a fresh session before any hook trust; inspect the isolated plugin data
for `q01-delivery.log`. A missing marker is a negative control only if the hook
was discovered, enabled, eligible to run and SessionStart actually occurred.
Otherwise mark UNKNOWN. Record session identifiers and before/after counts.

In `/hooks`, review the exact installed definition and script and trust only
that fixture. Start a new session and check that the fixed marker count increases
by one; inspect `/hooks` state and record the installed paths and hashes. An
unexpected marker before trust or no marker after trust is a failure requiring
investigation, not qualification. Do not claim a raw payload or normalized
provenance from this marker.

In a fresh session, use the UI's actual skill picker to discover and invoke the
fixture skill with the literal namespaced syntax shown by this installed host;
record the exact invocation and response `RECEIPTS_Q01_SKILL_DISCOVERED`.
This is the only model call authorized by this runbook once USER approves it.
If a namespace or invocation is unavailable, record UNKNOWN; do not guess syntax.

## Separate controls

Use fresh sessions and record definition/script hashes, installed cache path,
trust state, marker counts and outcomes for each control. Change one variable at
a time and restore/re-establish the trusted baseline between controls. Record
refresh/reinstallation steps actually needed and confirm installed bytes changed;
a source-only edit proves nothing about cached bytes.

1. Change only the installed fixture hook definition (for example its
   `statusMessage`), refresh by a supported method, and check `/hooks` plus a
   fresh SessionStart before retrusting. Record whether re-review is required
   and delivery is suppressed. Restore and retrust baseline.
2. Change only the installed marker script (e.g. add a harmless comment),
   refresh, inspect `/hooks`, and start a fresh session before retrusting.
   Record whether script freshness changes trust and delivery. Restore baseline.
3. With trusted baseline, run a fresh session with `codex --disable hooks` and
   compare marker counts and effective settings; restore normal startup.
4. Only in an independently provisioned isolated managed-policy environment,
   set `allow_managed_hooks_only = true` in its managed `requirements.toml`;
   verify the effective source and test a fresh session. Record suppression and
   restore that isolated policy. If safe provisioning or observation is not
   available, report UNKNOWN. Never change machine or organization policy.

A missing marker is not conclusive unless the session started, the plugin and
hook remained discoverable, the relevant policy was effective and the positive
control passed. Record errors and UNKNOWN for unavailable controls.

## Evidence, cleanup and disposition

Record all transitions, timestamps, definition/script/package hashes, session
IDs, UI trust and enablement, marker counts, effective policy evidence, command
exit codes and redacted transcripts. No raw hook payload is captured by this
fixture. Separately compare real captured host payloads, if independently
authorized, to the bounded Host parser and source rules before claiming
normalized event compatibility; this run alone does not do so. Host events
remain untrusted and cannot admit work or write State.

Close all sessions, then use the inspected supported remove commands:

```sh
codex plugin remove q01-observer@receipts-q01-fixture
codex plugin marketplace remove receipts-q01-fixture
codex plugin list --marketplace receipts-q01-fixture --json
codex plugin marketplace list
```

Verify removal using list outputs and inspect the isolated cache/data. Preserve
redacted evidence outside `Q01_BASE`, compare real user settings existence and
hashes to the baseline, and only then remove the exact `Q01_BASE` directory
after verifying its path. Record cleanup results and any residue. If removal or
isolation fails, preserve it and report the failure; do not clean unrelated data.
Return all observations to A1 for review. No result here automatically enables
native mode.
