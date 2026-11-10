#!/usr/bin/env bash
# Hermetic controls for the security scripts. No network, no downloads and no
# real scanner: stub tools stand in for cargo-audit, Gitleaks and actionlint,
# so these prove the SCRIPTS' behaviour (window, snapshot coverage, YAML
# policy, bounded termination, cleanup, evidence retention), not scanner
# detection quality. Real scanner behaviour needs the hosted run.
# Usage: selftest.sh        (exit 0 only when every control behaves)
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=pins.env
. "$here/pins.env"
# shellcheck source=lib.sh
. "$here/lib.sh"
T=$(mktemp -d)
cleanup() { chmod -R u+rwX "$T" 2>/dev/null; rm -rf "${T:?}"; }
trap cleanup EXIT
fails=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; fails=$((fails + 1)); }
check() { # name command...   (command succeeds => PASS)
  local name=$1
  shift
  if "$@"; then pass "$name"; else fail "$name" "condition false"; fi
}
has() { grep -qF -- "$2" "$1"; }

# ---- stub tools and a stub dataset ------------------------------------------------
make_env() { # name -> $T/name/{tools,src,advisory-db,tools.env,evidence}
  local e="$T/$1" now
  now=$(now_epoch)
  mkdir -p "$e/tools" "$e/src/.github/workflows"
  cat > "$e/tools/cargo-audit" <<'STUB'
#!/usr/bin/env bash
case "$1" in --version) echo "cargo-audit stub"; exit 0;; esac
f=""; while [ $# -gt 0 ]; do [ "$1" = --file ] && f=$2; shift; done
[ -n "${STUB_PIDFILE:-}" ] && [ -n "${STUB_AUDIT_HANG:-}" ] && echo $$ > "$STUB_PIDFILE"
[ -n "${STUB_AUDIT_HANG:-}" ] && { trap '' TERM; sleep 120; }
[ -n "${STUB_AUDIT_SLEEP:-}" ] && sleep "$STUB_AUDIT_SLEEP"
if grep -q smallvec "$f" && grep -q '1.6.0' "$f"; then
  echo '{"vulnerabilities":{"found":true,"list":[{"advisory":{"id":"RUSTSEC-2021-0003"}}]}}'; exit 1
fi
echo '{"vulnerabilities":{"found":false}}'
STUB
  cat > "$e/tools/gitleaks" <<'STUB'
#!/usr/bin/env bash
case "$1" in version) echo "gitleaks stub"; exit 0;; esac
dirp=$2; rp=""; while [ $# -gt 0 ]; do [ "$1" = --report-path ] && rp=$2; shift; done
if [ -n "${STUB_LEAKS_HANG:-}" ]; then echo $$ > "$STUB_PIDFILE"; trap '' TERM; sleep 120; fi
if [ -n "${STUB_LEAKS_BLOCK_CLEANUP:-}" ]; then
  parent=$(dirname "$(dirname "$dirp")")   # the run's work-dir parent
  chmod 555 "$parent"
fi
hit=$(grep -rhE 'AKIA[A-Z2-7]{16}|ghp_[A-Za-z0-9]{36}|PLANTED_SECRET' "$dirp" 2>/dev/null | head -n1)
if [ -n "$hit" ]; then
  if [ -n "${STUB_LEAKS_LEAK:-}" ]; then echo "[{\"RuleID\":\"stub-rule\",\"Match\":\"$hit\"}]" > "$rp"
  else echo '[{"RuleID":"stub-rule","Secret":"REDACTED"}]' > "$rp"; fi
  exit 1
fi
echo '[]' > "$rp"
STUB
  cat > "$e/tools/actionlint" <<'STUB'
#!/usr/bin/env bash
case "$1" in -version) echo "actionlint stub"; exit 0;; esac
for f in "$@"; do [ -f "$f" ] && grep -q stepz "$f" && { echo "$f: unexpected key stepz"; exit 1; }; done
exit 0
STUB
  chmod +x "$e/tools/"*
  # a stub dataset: floor commit then a head, origin = the pinned origin
  git init -q "$e/advisory-db"
  git -C "$e/advisory-db" config user.email t@t.invalid
  git -C "$e/advisory-db" config user.name t
  git -C "$e/advisory-db" config commit.gpgsign false
  echo one > "$e/advisory-db/a.txt"; git -C "$e/advisory-db" add -A; git -C "$e/advisory-db" commit -q -m floor
  FLOOR_FOR_TESTS=$(git -C "$e/advisory-db" rev-parse HEAD)
  echo two > "$e/advisory-db/b.txt"; git -C "$e/advisory-db" add -A; git -C "$e/advisory-db" commit -q -m head
  git -C "$e/advisory-db" remote add origin "$ADVISORY_DB_URL"
  # a source checkout with a safe workflow and a clean lock
  git init -q "$e/src"
  git -C "$e/src" config user.email t@t.invalid
  git -C "$e/src" config user.name t
  git -C "$e/src" config commit.gpgsign false
  cp "$here/controls/safe-block.yml" "$e/src/.github/workflows/ci.yml"
  printf 'version = 3\n\n[[package]]\nname = "demo"\nversion = "0.1.0"\n' > "$e/src/Cargo.lock"
  echo 'fn main() {}' > "$e/src/main.rs"
  git -C "$e/src" add -A; git -C "$e/src" commit -q -m src
  write_tools_env "$e" $((now - 100)) $((now + 86000))
}

write_tools_env() { # env observed expires
  local e=$1 observed=$2 expires=$3 db="$1/advisory-db"
  {
    echo "CARGO_AUDIT=$e/tools/cargo-audit"
    echo "GITLEAKS=$e/tools/gitleaks"
    echo "ACTIONLINT=$e/tools/actionlint"
    echo "CARGO_AUDIT_BIN_SHA256=$(sha256_file "$e/tools/cargo-audit")"
    echo "GITLEAKS_BIN_SHA256=$(sha256_file "$e/tools/gitleaks")"
    echo "ACTIONLINT_BIN_SHA256=$(sha256_file "$e/tools/actionlint")"
    echo "ADVISORY_DB=$db"
    echo "ADVISORY_DB_ORIGIN=$ADVISORY_DB_URL"
    echo "ADVISORY_DB_COMMIT=$(git -C "$db" rev-parse HEAD)"
    echo "ADVISORY_DB_TREE=$(git -C "$db" rev-parse 'HEAD^{tree}')"
    echo "ADVISORY_DB_COMMITTED=$(git -C "$db" show -s --format=%ct HEAD)"
    echo "ADVISORY_DB_CONTENT_SHA256=$(dataset_content_sha "$db")"
    echo "ADVISORY_DB_OBSERVED_AT=$observed"
    echo "ADVISORY_DB_WINDOW_EXPIRES=$expires"
  } > "$e/tools.env"
}

run() { # env [extra env assignments via env(1) outside]
  bash "$here/run.sh" "$1" "$1/src" > "$1/run.out" 2>&1
}
summary() { cat "$1/evidence/summary.md" 2>/dev/null; }
row() { summary "$1" | grep -F "| $2 |" | head -n1; }

echo "== F10 window semantics (pure functions) =="
check "window: inside" test "$(window_state 100 200 150)" = OK
check "window: expired at the boundary" test "$(window_state 100 200 200)" = EXPIRED
check "window: clock before observation" test "$(window_state 100 200 99)" = NOT_YET
check "commit time after observation is an anomaly, not freshness" bash -c ". '$here/lib.sh'; ! commit_time_ok 1000 999"
check "commit time equal to observation is accepted" bash -c ". '$here/lib.sh'; commit_time_ok 1000 1000"
check "pins carry the 24-hour window and no commit-age rule" bash -c \
  "grep -q '^ADVISORY_DB_WINDOW_SECONDS=86400' '$here/pins.env' && ! grep -rq 'MAX_AGE' '$here/run.sh' '$here/acquire.sh' '$here/lib.sh' '$here/pins.env'"

echo "== F10 dataset observation =="
make_env ds
db="$T/ds/advisory-db"
now=$(now_epoch)
check "validate_dataset accepts the pinned origin, floor ancestry and a past commit" \
  bash -c ". '$here/lib.sh'; validate_dataset '$db' '$ADVISORY_DB_URL' '$FLOOR_FOR_TESTS' $now"
check "validate_dataset refuses another origin" bash -c \
  ". '$here/lib.sh'; ! validate_dataset '$db' 'https://example.invalid/x.git' '$FLOOR_FOR_TESTS' $now >/dev/null"
check "validate_dataset refuses a head that does not descend from the floor" bash -c \
  ". '$here/lib.sh'; ! validate_dataset '$db' '$ADVISORY_DB_URL' 0000000000000000000000000000000000000000 $now >/dev/null"
future_db="$T/future-db"
git clone -q "$db" "$future_db"
git -C "$future_db" remote set-url origin "$ADVISORY_DB_URL"
git -C "$future_db" config user.email t@t.invalid; git -C "$future_db" config user.name t
echo f > "$future_db/f.txt"; git -C "$future_db" add -A
GIT_COMMITTER_DATE="@$((now + 86400))" GIT_AUTHOR_DATE="@$((now + 86400))" git -C "$future_db" commit -q -m future
check "validate_dataset refuses a future-dated head (no negative age)" bash -c \
  ". '$here/lib.sh'; ! validate_dataset '$future_db' '$ADVISORY_DB_URL' '$FLOOR_FOR_TESTS' $now >/dev/null"
old_db="$T/old-db"
git clone -q "$db" "$old_db"; git -C "$old_db" remote set-url origin "$ADVISORY_DB_URL"
git -C "$old_db" config user.email t@t.invalid; git -C "$old_db" config user.name t
echo o > "$old_db/o.txt"; git -C "$old_db" add -A
GIT_COMMITTER_DATE="@$((now - 90 * 86400))" GIT_AUTHOR_DATE="@$((now - 90 * 86400))" git -C "$old_db" commit -q -m old
check "an old head is NOT rejected by age (commit age is not the window)" bash -c \
  ". '$here/lib.sh'; validate_dataset '$old_db' '$ADVISORY_DB_URL' '$FLOOR_FOR_TESTS' $now >/dev/null"

echo "== happy path, retained evidence, planted-input cleanup =="
make_env ok
run "$T/ok"; rc=$?
check "happy path exits 0" test "$rc" -eq 0
for want in "dataset-eligibility | PASS" "control-planted-secret | PASS" "control-vulnerable-lockfile | PASS" \
  "control-invalid-workflow | PASS" "control-workflow-policy | PASS" "workflow-policy | PASS" \
  "cargo-audit | PASS" "gitleaks | PASS" "actionlint | PASS"; do
  check "summary: $want" has "$T/ok/evidence/summary.md" "$want"
done
for f in summary.md SHA256SUMS steps.tsv tools.env audit.json audit.json.stderr gitleaks.json \
  snapshot-manifest.txt snapshot-entries.txt workflow-policy.txt actionlint.txt Cargo.lock.scanned \
  config/audit.toml config/gitleaks.toml config/empty.gitleaksignore \
  controls/control-secret.json controls/control-secret.identity controls/control-lock.json \
  controls/control-lock.Cargo.lock controls/control-workflow.bad.yml controls/policy-expected.txt; do
  check "evidence retained: $f" test -s "$T/ok/evidence/$f" -o -f "$T/ok/evidence/$f"
done
check "planted inputs removed" test ! -e "$T/ok/work"
check "summary records cleanup" has "$T/ok/evidence/summary.md" "Planted-input cleanup: OK"
check "no planted value in retained evidence" bash -c \
  "! grep -rEq 'AKIA[A-Z2-7]{16}|ghp_[A-Za-z0-9]{36}' '$T/ok/evidence'"
check "SHA256SUMS matches the retained files" bash -c \
  "cd '$T/ok/evidence' && while read -r h f; do [ \"\$(. '$here/lib.sh'; sha256_file \"\$f\")\" = \"\$h\" ] || exit 1; done < SHA256SUMS"
check "step records carry argv, UTC and exit" bash -c "grep -q 'cargo-audit' '$T/ok/evidence/steps.tsv' && grep -qE '20[0-9]{2}-' '$T/ok/evidence/steps.tsv'"

echo "== F11a complete tracked snapshot (export-ignore) =="
make_env ei
printf 'hidden.txt export-ignore\n' > "$T/ei/src/.gitattributes"
echo 'PLANTED_SECRET value' > "$T/ei/src/hidden.txt"
git -C "$T/ei/src" add -A; git -C "$T/ei/src" commit -q -m "export-ignored file"
run "$T/ei"; rc=$?
check "an export-ignore'd tracked secret is still scanned and fails the job" test "$rc" -ne 0
check "gitleaks reported the finding" bash -c "grep -F '| gitleaks |' '$T/ei/evidence/summary.md' | grep -q FAIL"
check "the manifest lists the export-ignored file" has "$T/ei/evidence/snapshot-manifest.txt" "hidden.txt"
make_env gl
git -C "$T/gl/src" update-index --add --cacheinfo "160000,$(git -C "$T/gl/src" rev-parse HEAD),sub"
git -C "$T/gl/src" commit -q -m gitlink
run "$T/gl"; rc=$?
check "submodule entries are unsupported content, not silently skipped" test "$rc" -ne 0
check "…reported as such" has "$T/gl/evidence/summary.md" "unsupported content"

echo "== F11b workflow trust through YAML semantics =="
bad_inline="$T/wf-bad"
mkdir -p "$bad_inline"
while IFS='|' read -r file phrases; do
  case "$file" in ''|'#'*) continue ;; esac
  d="$T/policy-$file"; mkdir -p "$d"; cp "$here/controls/$file" "$d/"
  ruby "$here/workflow_policy.rb" "$d" > "$T/policy-$file.out" 2>&1; prc=$?
  if [ "$phrases" = PASS ]; then
    check "policy passes $file" test "$prc" -eq 0
  else
    check "policy flags $file" test "$prc" -ne 0
    IFS=',' read -ra want <<< "$phrases"
    for p in "${want[@]}"; do check "  $file reports '$p'" has "$T/policy-$file.out" "$p"; done
  fi
done < "$here/controls/expected.txt"
make_env wfi
printf 'on: push\npermissions: {contents: write}\njobs: {x: {runs-on: ubuntu-latest, steps: [{run: echo hi}]}}\n' \
  > "$T/wfi/src/.github/workflows/ci.yml"
git -C "$T/wfi/src" add -A; git -C "$T/wfi/src" commit -q -m inline-write
run "$T/wfi"; rc=$?
check "an inline write permission in the tested workflows fails the job" test "$rc" -ne 0
check "…as workflow-policy" bash -c "grep -F '| workflow-policy |' '$T/wfi/evidence/summary.md' | grep -q FAIL"
make_env wfs
cp "$here/controls/safe-inline.yml" "$T/wfs/src/.github/workflows/ci.yml"
git -C "$T/wfs/src" add -A; git -C "$T/wfs/src" commit -q -m safe-inline
run "$T/wfs"; rc=$?
check "a safe inline workflow is not over-blocked" test "$rc" -eq 0

echo "== F10 eligibility at execution =="
make_env ex
now=$(now_epoch)
write_tools_env "$T/ex" $((now - 200000)) $((now - 100))
run "$T/ex"; rc=$?
check "an expired window fails the run" test "$rc" -ne 0
check "…no scan used the expired dataset" bash -c "grep -F '| cargo-audit |' '$T/ex/evidence/summary.md' | grep -q FAIL"
check "…and says why (never extended)" has "$T/ex/evidence/summary.md" "expired"
check "…the independent secret scan still ran" has "$T/ex/evidence/summary.md" "gitleaks | PASS"
check "…no scanner report was produced from the expired dataset" test ! -s "$T/ex/evidence/audit.json"
make_env nyet
now=$(now_epoch)
write_tools_env "$T/nyet" $((now + 3600)) $((now + 3600 + 86400))
run "$T/nyet"; rc=$?
check "an observation in the future (clock anomaly) fails the run" test "$rc" -ne 0
check "…as not yet eligible" has "$T/nyet/evidence/summary.md" "before the dataset observation"
make_env mid
now=$(now_epoch)
write_tools_env "$T/mid" $((now - 100)) $((now + 5))
STUB_AUDIT_SLEEP=7 run "$T/mid"; rc=$?
check "a scan that finishes after the window closes fails" test "$rc" -ne 0
check "…reported as after-audit expiry" has "$T/mid/evidence/summary.md" "after audit"
make_env mut
echo tampered > "$T/mut/advisory-db/b.txt"
run "$T/mut"; rc=$?
check "a modified dataset checkout is refused" test "$rc" -ne 0
check "…as a changed snapshot" has "$T/mut/evidence/summary.md" "differs from the observed snapshot"
make_env org
git -C "$T/org/advisory-db" remote set-url origin https://example.invalid/other.git
run "$T/org"; rc=$?
check "a dataset from another origin is refused" test "$rc" -ne 0
check "…as an origin mismatch" has "$T/org/evidence/summary.md" "origin differs"
make_env swap
printf '#!/usr/bin/env bash\necho swapped\n' > "$T/swap/tools/gitleaks"
run "$T/swap"; rc=$?
check "a tool changed after acquisition is refused" test "$rc" -ne 0
check "…reported as an identity failure" has "$T/swap/evidence/summary.md" "changed since acquisition"
make_env noenv
rm -f "$T/noenv/tools.env"
run "$T/noenv"; rc=$?
check "a missing tools.env fails without silent re-acquisition" test "$rc" -ne 0

echo "== F12 bounded termination, reaping and cleanup =="
pidfile="$T/direct.pid"
s=$(now_epoch)
( TOOL_TIMEOUT_SECONDS=2 KILL_AFTER_SECONDS=1; . "$here/lib.sh"
  bounded 2 bash -c "trap '' TERM; echo \$\$ > '$pidfile'; sleep 60"; echo "$?" > "$T/direct.rc" )
e=$(now_epoch)
check "a TERM-resistant command is killed after the kill-after bound" bash -c \
  "[ \"\$(cat '$T/direct.rc')\" = 137 ] || [ \"\$(cat '$T/direct.rc')\" = 124 ]"
check "…within timeout + kill-after (+slack)" test $((e - s)) -le 8
sleep 0.3
check "…and the process is gone" bash -c "! kill -0 \$(cat '$pidfile') 2>/dev/null"

make_env term
pf="$T/term.pid"
s=$(now_epoch)
STUB_LEAKS_HANG=1 STUB_PIDFILE="$pf" TOOL_TIMEOUT_SECONDS=2 KILL_AFTER_SECONDS=1 run "$T/term"; rc=$?
e=$(now_epoch)
check "a TERM-resistant scanner fails the run" test "$rc" -ne 0
check "…as a timeout" has "$T/term/evidence/summary.md" "timed out"
check "…inside the bound (2s + 1s kill-after per hung tool, plus slack)" test $((e - s)) -le 25
sleep 0.3
check "…the scanner process was reaped" bash -c "! kill -0 \$(cat '$pf') 2>/dev/null"
check "…planted inputs removed after the failure" test ! -e "$T/term/work"

make_env sig
pf="$T/sig.pid"
STUB_LEAKS_HANG=1 STUB_PIDFILE="$pf" KILL_AFTER_SECONDS=1 bash "$here/run.sh" "$T/sig" "$T/sig/src" > "$T/sig/run.out" 2>&1 &
runpid=$!
for _ in $(seq 1 80); do [ -s "$pf" ] && break; sleep 0.25; done
check "the hung scanner started" test -s "$pf"
s=$(now_epoch)
kill -TERM "$runpid" 2>/dev/null
for _ in $(seq 1 60); do kill -0 "$runpid" 2>/dev/null || break; sleep 0.25; done
e=$(now_epoch)
check "TERM to the script ends it promptly" bash -c "! kill -0 $runpid 2>/dev/null"
check "…within kill-after + slack" test $((e - s)) -le 10
wait "$runpid" 2>/dev/null; rc=$?
check "…never green" test "$rc" -ne 0
sleep 0.3
check "…the in-flight scanner was reaped" bash -c "! kill -0 \$(cat '$pf') 2>/dev/null"
check "…planted inputs removed after the abnormal exit" test ! -e "$T/sig/work"
check "…evidence records an incomplete run" has "$T/sig/evidence/summary.md" "RUN INCOMPLETE"
check "…and the signal" has "$T/sig/evidence/summary.md" "signal=TERM"

if [ "$(id -u)" -ne 0 ]; then
  make_env clean
  STUB_LEAKS_BLOCK_CLEANUP=1 run "$T/clean"; rc=$?
  chmod -R u+rwX "$T/clean" 2>/dev/null
  check "unremovable planted inputs make the run red (unknown cleanup is never green)" test "$rc" -ne 0
  check "…cleanup state is FAILED" has "$T/clean/evidence/summary.md" "Planted-input cleanup: FAILED"
else
  echo "SKIP cleanup-failure control (running as root ignores directory permissions)"
fi

echo "== redaction guard =="
make_env leak
STUB_LEAKS_LEAK=1 run "$T/leak"; rc=$?
check "a scanner report that holds the planted value fails the run" test "$rc" -ne 0
check "…the offending file is removed from evidence" bash -c \
  "! grep -rEq 'AKIA[A-Z2-7]{16}|ghp_[A-Za-z0-9]{36}' '$T/leak/evidence'"
check "…and it says so" has "$T/leak/evidence/summary.md" "evidence-redaction"

echo "== acquire.sh: bounded version commands, partial cleanup =="
check "acquire.sh parses" bash -n "$here/acquire.sh"
check "no unbounded version command in acquire.sh" bash -c \
  "! grep -nE '^[^#]*\"\\\$(audit|leaks|lint)\" +(--version|version|-version)' '$here/acquire.sh' | grep -v tool_version"
check "acquire.sh bounds fetch, extraction and version commands" bash -c \
  "grep -q 'bounded \"\$FETCH_TIMEOUT_SECONDS\" curl' '$here/acquire.sh' && grep -q 'bounded \"\$VERSION_TIMEOUT_SECONDS\" tar' '$here/acquire.sh' && grep -q 'bounded \"\$VERSION_TIMEOUT_SECONDS\" \"\$bin\"' '$here/acquire.sh' && grep -q 'bounded \"\$FETCH_TIMEOUT_SECONDS\" git clone' '$here/acquire.sh'"
check "acquire.sh removes a partial acquisition and tools.env on abnormal exit" bash -c \
  "grep -q 'rm -f \"\$dir/tools.env\"' '$here/acquire.sh' && grep -q \"trap 'exit 143' TERM\" '$here/acquire.sh'"

echo
if [ "$fails" -eq 0 ]; then echo "SECURITY SCRIPT CONTROLS: ALL PASS"; else echo "SECURITY SCRIPT CONTROLS: $fails FAILED"; fi
exit "$fails"
