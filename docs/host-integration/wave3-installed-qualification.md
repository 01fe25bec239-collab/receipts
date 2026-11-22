# Wave 3 installed Codex qualification

Preparation only. Run independently of D4 from the reviewed
`codex/wave3-live-qualification` SHA. Synthetic Host/CLI tests and
`test_host.py` require no account. They do not prove installed delivery.
Retain the prior macOS Codex 0.169.0 five-event witness: SessionStart,
PostToolUse, manual PreCompact/PostCompact and SessionEnd, with untrusted and
hooks-disabled controls. It has no automatic-compaction, Linux, managed-policy
or pre-run global-settings baseline proof. Do not relabel it.

Use a disposable Linux VM with an admitted installed Codex executable, or a
separate disposable macOS root for the remaining macOS checks. Linux ancestry
must actually execute on Linux; a container parser test is insufficient.
Operator inputs: reviewed executable path/version/digest, an authorized
Codex account and direct isolated sign-in, UI project/hook trust, bounded model
budget and a supported isolated managed-policy supplier if available. Do not
use a subscription credential as an API key or copy any existing login store.
Do not run a provider turn during preparation.

## Credential-free preparation

Build the optimized observer with
`cargo build --release --locked -p receipts-cli --bin receipts`. Set
`RECEIPTS_BIN` to the canonical path of that release binary. Record the build
profile, source SHA/tree, OS, UTC, CLI version, executable hash, and hashes of
the three installed plugin files named by enrollment.

Codex 0.169.0 gives each
[SessionEnd hook a maximum of three seconds](https://github.com/openai/codex/blob/rust-v0.169.0/codex-rs/hooks/src/events/session_end.rs).
Observer startup and validation must fit inside that hook budget. On the
recorded ARM64 guest, enrollment against the same large Codex executable took
2.542s with the debug observer and 0.177s with the release observer. Use the
release profile for the installed witness and require its native SessionEnd
event within the host's existing budget.

Inspect `codex --help`, `codex login --help`, plugin add/remove and marketplace
add/remove help using this installed version. No login/status/config/cache
inspection is needed to prepare. Stop on incompatible help or plugin format.

In a fresh shell, substitute canonical absolute paths for CODEX_EXE and
RECEIPTS_BIN and an operator-owned private EVIDENCE directory outside the repo.
Capture settings existence/hashes BEFORE any registration or login. Enumerate
all applicable user/system/managed settings sources on this host; the examples
below are a minimum, not discovery of every policy source. Missing sources are
recorded as absent. Never select auth/session files.

```sh
umask 077
SOURCE=$(pwd -P)
QUALIFIER="$SOURCE/.github/scripts/wave3-qualification/host.py"
python3 -B "$QUALIFIER" settings \
  "$HOME/.codex/config.toml" "$HOME/.codex/requirements.toml" \
  /etc/codex/config.toml /etc/codex/requirements.toml \
  > "$EVIDENCE/settings-before.json"
HOST_FIXTURE=$(mktemp -d "${TMPDIR:-/tmp}/receipts-host-wave3.XXXXXXXX")
export HOST_FIXTURE
mkdir -m 700 "$HOST_FIXTURE/home" "$HOST_FIXTURE/codex" \
  "$HOST_FIXTURE/repo" "$HOST_FIXTURE/enrollment" "$HOST_FIXTURE/marketplace"
mkdir -p "$HOST_FIXTURE/marketplace/.agents/plugins" "$HOST_FIXTURE/marketplace/plugins"
cp -R "$SOURCE/plugins/codex" "$HOST_FIXTURE/marketplace/plugins/receipts"
# q01-fixture is excluded from the installed production plugin.
rm -r "$HOST_FIXTURE/marketplace/plugins/receipts/q01-fixture"
python3 - <<'PY'
import json, os
from pathlib import Path
path=Path(os.environ['HOST_FIXTURE'])/'marketplace/.agents/plugins/marketplace.json'
path.write_text(json.dumps(dict(name='receipts-wave3-qualification', plugins=[dict(
 name='receipts', source=dict(source='local',path='./plugins/receipts'),
 policy=dict(installation='AVAILABLE',authentication='ON_INSTALL'),category='Productivity')])))
PY
git -C "$HOST_FIXTURE/repo" init
export CODEX_HOME="$HOST_FIXTURE/codex"
export RECEIPTS_CODEX_ENROLLMENT="$HOST_FIXTURE/enrollment"
fixture_codex() {
  env -i PATH=/usr/bin:/bin LANG=C.UTF-8 TERM=xterm-256color \
    HOME="$HOST_FIXTURE/home" CODEX_HOME="$CODEX_HOME" \
    RECEIPTS_CODEX_ENROLLMENT="$RECEIPTS_CODEX_ENROLLMENT" \
    RECEIPTS_BIN="$RECEIPTS_BIN" "$CODEX_EXE" "$@"
}
cd "$HOST_FIXTURE/repo"
```

Prefer the installed host's supported file-only auth storage in the isolated
CODEX_HOME; verify the setting in that version's help/docs first. If an OS
keyring, managed override or other source prevents isolated login, stop and
report UNSUPPORTED. Do not fall back to the workstation cache.

## Installed positive and negative checks (later operator execution)

1. List existing isolated registrations; require no unrelated plugins/hooks.
   Use `fixture_codex` for every Codex invocation below, including login and
   interactive sessions. Add only `$HOST_FIXTURE/marketplace` with
   `fixture_codex plugin marketplace add`, then
   `fixture_codex plugin add receipts@receipts-wave3-qualification`. Inspect list
   output to find the **actual installed cache path**; require it under this
   CODEX_HOME. Compare manifest, hooks and launcher hashes to the source copy.
   Set `INSTALLED_PLUGIN` to its canonical absolute path. Enroll with:

       "$RECEIPTS_BIN" codex-native-enroll "$RECEIPTS_CODEX_ENROLLMENT" "$CODEX_EXE" "$HOST_FIXTURE/repo" "$INSTALLED_PLUGIN"

2. Only after separate live authority, sign in directly on this runner using
   the installed CLI's own flow. Start an untrusted fresh session and complete
   a bounded no-tool turn. Confirm the Receipts hooks were discovered/enabled
   but untrusted, and no inbox exists. No marker without a completed eligible
   session is UNKNOWN. Trust only the installed Receipts hooks through `/hooks`;
   never bypass hook trust. Start fresh, request one harmless shell operation,
   invoke `/compact`, wait for completion, and exit normally. Keep UI evidence
   private and sanitized; never retain raw hook input or login output.

       python3 -B "$QUALIFIER" inspect "$HOST_FIXTURE/repo/.receipts/inbox/host-events.jsonl" --trigger manual > "$EVIDENCE/native-manual.json"

3. Fingerprint the inbox, start fresh with `--disable hooks`, complete the
   same bounded no-tool turn and exit. Fingerprint again and compare the JSON
   records using `host.py compare BEFORE AFTER`. Require exact equality and a
   successful prior positive. This proves disabled suppression for that session.

4. Automatic compaction: use a separate fresh trusted session with the same
   pinned bytes. If the installed version exposes a supported auto-compaction
   threshold (`model_auto_compact_token_limit` in the current
   [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)),
   set it only in this isolated CODEX_HOME using its documented mechanism. Bound the session to the approved token/spend/time allowance;
   never loop until success. Do not invoke `/compact`. Require actual PRE and
   POST normalized events with `trigger=auto`, plus an observed session ending:

       python3 -B "$QUALIFIER" inspect "$HOST_FIXTURE/repo/.receipts/inbox/host-events.jsonl" --trigger auto > "$EVIDENCE/native-auto.json"

   The inspector checks event evidence, not causal history; retain the sanitized
   operator record that no manual compact command ran. No pair before the bound
   is INCONCLUSIVE. No supported isolated trigger is UNSUPPORTED, not PASS. Current documentation describes the setting;
   it does not establish support in an uninspected Linux binary.

5. Managed-policy suppression: only on an independent disposable VM or an
   installed-host-supported isolated administrative policy source. Establish the
   positive first. Provision `allow_managed_hooks_only=true` through that
   supplier; independently observe its effective source. Fresh session, completed
   bounded turn, exact unchanged inbox fingerprint. Restore only the owned
   policy and repeat a fresh positive. A local user config pretending to be
   managed policy is not proof. If the host offers no safe supplier, record
   UNSUPPORTED without modifying global or organization settings.

## Evidence and cleanup

Keep settings before/after, normalized event counts/phases/trigger and spool
hash, executable/plugin/source hashes, UTC/OS/version, session completion/exit,
trust/policy observations and separate outcomes for each case. The helper emits
no session IDs, paths from events, tool arguments or raw payloads. It cannot
prove trust or effective managed policy from the spool alone. Preserve failures.

After all sessions exit: remove only receipts@receipts-wave3-qualification and
its marketplace using the supported remove commands under the isolated HOME
and CODEX_HOME. Verify list outputs and installed cache residue. The operator
revokes/removes only the freshly created login through its supported flow; do
not upload its store. In the original shell, repeat the exact settings command
using original paths and compare with `host.py compare`. An absent initial
baseline can never be reconstructed after the run. Only after retained evidence,
verified owned paths and no live sessions, remove the exact HOST_FIXTURE root.
Record cleanup failure/residue separately; never delete unrelated registrations.
