#!/usr/bin/env bash
# RA W1 Runtime-final hosted diagnostic: both concrete Runtime adapters through
# the merged Workspace protected launch. Derived from the reviewed
# we-w1-completion-linux/run.sh; only the test binary, test list, cgroup,
# fixture glob and environment names differ, `prepare` also builds the
# synthetic native worker unprivileged, and self-check sources the
# Runtime-local custody-self-check.sh adapted to this script's two tests
# (diff the files). Host custody administration is the UNCHANGED
# we-w1-completion-linux/custody.sh, sourced from its reviewed location.
# Synthetic worker only: no provider, secret or network use, and no result
# here is Claude/Codex provider evidence.
# Disposable GitHub VM only: the hosted phase runs as root and creates/deletes
# only its own cgroup and temp fixture. Never run `hosted` with sudo on a
# workstation.
#
# Result contract: setup, test and owned-cleanup statuses, the number of tests
# that actually completed and the FIRST failed test (identity and status) are
# recorded separately in result.txt, and PASS/exit 0 requires ALL of: setup 0,
# every test completed with status 0, cleanup 0, no interruption AND the final
# result actually published. Nothing is initialized to success: result.txt
# starts as INCOMPLETE (or the run stops before creating anything) and becomes
# PASS or FAIL only through one atomic rename at the very end; a write or
# rename failure exits nonzero and leaves the previous result. A retained,
# unknown or unowned resource (including an uncertain create) is a cleanup
# failure, never success. SIGTERM/SIGINT/SIGHUP received before the command
# right after the final rename are recorded (a just-renamed PASS is
# republished as FAIL), cleaned up after and re-exited with 128+n; after
# that point they are ignored, so the exit status matches the published
# verdict. SIGKILL or machine loss cannot be handled and does NOT guarantee
# cleanup: before the final rename it leaves INCOMPLETE, after it the
# published verdict stays (even PASS) with a killed exit status. Host custody
# administration (anchors the product admission requires) lives in custody.sh.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
we="$here/../we-w1-completion-linux"
# shellcheck source=../we-w1-completion-linux/custody.sh
. "$we/custody.sh"

if [ "${1:-}" = self-check ]; then out=; else out="${DIAGNOSTIC_OUT:?}"; fi
bin_record="$out/test-binary"
worker="$out/protected-worker"
cgroup="/sys/fs/cgroup/receipts-ra-w1-diagnostic"
cleanup_polls=50
cgroup_pending=0 # 1 only while mkdir "$cgroup" may have created it without our knowing
tests=(
  "protected_launch_tests::hosted_protected_adapters_collect_events_fidelity_and_channels"
  "protected_launch_tests::hosted_protected_cancel_drop_and_refusals_never_fall_back"
)

prepare() {
  mkdir -p "$out"
  test "$(git rev-parse HEAD)" = "${DIAGNOSTIC_HEAD:?}"
  test -z "$(git status --porcelain)"
  {
    echo "head=$DIAGNOSTIC_HEAD"
    echo "image=${ImageOS:-unknown}/${ImageVersion:-unknown}"
    uname -srmv
    git --version
    rustc --version
    cargo --version
    stat -f -c 'cgroupfs=%T' /sys/fs/cgroup
    cat /sys/kernel/security/lsm 2>/dev/null | sed 's/^/lsm=/' || true
    sha256sum Cargo.lock
  } >"$out/provenance.txt"
  # Unprivileged locked build of the exact library test binary.
  cargo test --locked -p receipts-runtime-adapters --lib --no-run \
    --message-format=json 2>/dev/null |
    sed -n 's/.*"executable":"\([^"]*receipts_runtime_adapters-[^"]*\)".*/\1/p' |
    tail -n 1 >"$bin_record"
  test -x "$(cat "$bin_record")"
  sha256sum "$(cat "$bin_record")" >>"$out/provenance.txt"
  # Unprivileged build of the synthetic native worker the hosted tests run:
  # it observes its own initial exec environment (no shell adds PWD). The
  # hosted phase only copies it; nothing is ever compiled as root.
  rustc --edition=2024 src/adapters/runtime/protected_launch_worker.rs -o "$worker"
  test -x "$worker"
  sha256sum "$worker" >>"$out/provenance.txt"
}

# True when any live process runs as a fixture identity (controller, worker
# or the dedicated worker range).
fixture_uid_in_use() {
  ps -eo uid= | awk '$1 == 61001 || $1 == 61002 || ($1 >= 62000 && $1 <= 62063) { found = 1 } END { exit !found }'
}

# True when a hosted-test fixture directory is still present.
fixture_dirs_retained() {
  compgen -G "${TMPDIR:-/tmp}/ra-w1-native-*" >/dev/null
}

# Removes one proven-empty owned cgroup directory (cgroupfs allows rmdir with
# its pseudo-files present). Overridden only by the unprivileged self-check.
remove_cgroup_dir() {
  rmdir "$1"
}

log_cleanup() {
  echo "CLEANUP $*" >>"$out/diagnostic.log"
}

# True for a regular file whose name is a cgroup-v2 interface file of the
# delegated cgroup (`cgroup.*` plus the controller prefixes). Directories,
# links and any other name are never control files, so an unexpected child
# cgroup or stray file is still reported as unowned.
cgroup_control_file() {
  [ -f "$1" ] && [ ! -L "$1" ] || return 1
  case "${1##*/}" in
    cgroup.* | cpu.* | cpuset.* | hugetlb.* | io.* | irq.* | memory.* | misc.* | pids.* | rdma.*) return 0 ;;
    *) return 1 ;;
  esac
}

# Owned cleanup with verification. Returns 0 only when this diagnostic's own
# cgroup tree is killed, proven unpopulated and removed, no fixture identity
# process remains and no fixture temp directory is retained. Every failure is
# logged and counted; nothing is masked.
cleanup_owned() {
  local failed=0 entry empty
  if [ "$cgroup_pending" = 1 ] && { [ -e "$cgroup" ] || [ -L "$cgroup" ]; }; then
    # mkdir was started but not known to have completed. It was absent just
    # before, yet ownership is unproven: never adopt, kill or remove it.
    log_cleanup "UNCERTAIN $cgroup (creation was interrupted; ownership unproven, retained)"
    failed=1
  elif [ -e "$cgroup" ]; then
    for entry in "$cgroup"/*; do
      # Unmatched glob: no entries. A dangling symlink still counts as an entry.
      [ -e "$entry" ] || [ -L "$entry" ] || continue
      # Never follow or act through a link.
      if [ -L "$entry" ]; then
        log_cleanup "UNSAFE_LINK $entry"
        failed=1
        continue
      fi
      if cgroup_control_file "$entry"; then
        continue # kernel control pseudo-file of the delegated cgroup, not a child
      fi
      case "$entry" in
        "$cgroup"/receipts-*) ;;
        *)
          log_cleanup "UNOWNED $entry"
          failed=1
          continue
          ;;
      esac
      if [ ! -d "$entry" ]; then
        log_cleanup "UNOWNED $entry"
        failed=1
        continue
      fi
      if ! echo 1 >"$entry/cgroup.kill"; then
        log_cleanup "KILL_FAILED $entry"
        failed=1
        continue
      fi
      empty=0
      for _ in $(seq "$cleanup_polls"); do
        if grep -qx 'populated 0' "$entry/cgroup.events"; then
          empty=1
          break
        fi
        sleep 0.1
      done
      if [ "$empty" != 1 ]; then
        log_cleanup "NOT_PROVEN_EMPTY $entry"
        failed=1
        continue
      fi
      if ! remove_cgroup_dir "$entry"; then
        log_cleanup "RETAINED $entry"
        failed=1
      fi
    done
    if ! remove_cgroup_dir "$cgroup"; then
      log_cleanup "RETAINED $cgroup"
      failed=1
    fi
  fi
  # Killed members may need a moment to be reaped before identities are free.
  for _ in $(seq "$cleanup_polls"); do
    fixture_uid_in_use || break
    sleep 0.1
  done
  if fixture_uid_in_use; then
    log_cleanup "PROCESS_REMAINS as a fixture identity"
    failed=1
  fi
  if fixture_dirs_retained; then
    log_cleanup "RETAINED fixture directory"
    failed=1
  fi
  return "$failed"
}

test_status=unknown # first failed test's status, 0 only after ALL tests completed with 0
cleanup_status=unknown
setup_status=unknown
custody_cleanup_status=unknown
primary_failure=
tests_completed=0
first_failed_test=
first_failed_status=
interrupted=
finished=0
finalized=0
result_verdict=FAIL

# Atomically replaces result.txt (a killed or failed rename leaves the
# previous file). Callers check its status.
publish_result() { # file-with-content
  mv -f "$1" "$out/result.txt"
}

# Placeholder written before anything can fail: whatever kills the run later
# (even SIGKILL) leaves INCOMPLETE, never a stale or success verdict.
write_incomplete() {
  printf 'INCOMPLETE\ntest_status=unknown\ncleanup_status=unknown\nsetup_status=unknown\ncustody_cleanup_status=unknown\ntests_completed=0\ntests_total=%s\n' "${#tests[@]}" >"$out/result.txt.tmp" &&
    publish_result "$out/result.txt.tmp"
}

# Computes the verdict from recorded facts and publishes result.txt once.
# Returns nonzero, with finalized=0 and the previous result.txt kept, when the
# temporary file cannot be written or published. Explicit checks: callers run
# it in conditional contexts where errexit does not apply.
# Lines 1-3 (verdict, test_status, cleanup_status) keep their positions. The
# summary line names the highest-precedence failure: setup, then an abnormal
# end, then the first failed test; each fact stays in its own field.
write_result() {
  local primary=$primary_failure
  result_verdict=FAIL
  if [ "$setup_status" = 0 ] && [ "$test_status" = 0 ] && [ "$tests_completed" = "${#tests[@]}" ] &&
    [ "$cleanup_status" = 0 ] && [ -z "$interrupted" ]; then
    result_verdict=PASS
  fi
  if [ -z "$primary" ] && [ -n "$interrupted" ]; then primary="interrupted by $interrupted"; fi
  if [ -z "$primary" ] && [ -n "$first_failed_test" ]; then
    primary="test $first_failed_test exited $first_failed_status"
  fi
  if [ -z "$primary" ] && [ "$result_verdict" = FAIL ] && [ "$test_status" = unknown ]; then
    primary="run did not complete (setup_status=$setup_status tests_completed=$tests_completed/${#tests[@]})"
  fi
  local lines=("$result_verdict" "test_status=$test_status" "cleanup_status=$cleanup_status"
    "setup_status=$setup_status" "custody_cleanup_status=$custody_cleanup_status"
    "tests_completed=$tests_completed" "tests_total=${#tests[@]}")
  [ -z "$first_failed_test" ] || lines+=("first_failed_test=$first_failed_test" "first_failed_status=$first_failed_status")
  [ -z "$interrupted" ] || lines+=("interruption=$interrupted")
  [ -z "$primary" ] || lines+=("primary_failure=$primary")
  printf '%s\n' "${lines[@]}" >"$out/result.txt.tmp" || return 1
  publish_result "$out/result.txt.tmp" || return 1
  # The terminal boundary. A cancellation still pending from before this line
  # runs on_signal first, which republishes FAIL. From here on cancellation is
  # ignored so the exit status cannot contradict the published verdict.
  trap '' TERM INT HUP
  finalized=1
}

# Runs owned cleanup exactly once and writes the verdict. Used for the normal
# path and, through the EXIT trap, for any abort (where test_status stays
# "unknown" and the verdict is therefore FAIL). Host custody is removed only
# after worker/controller/cgroup cleanup is proven; otherwise it is retained
# and reported. Statuses become known only when computed. Cancellation stays
# live until write_result has published (see there); a result that could not be
# published is a failure even when every test and cleanup passed.
finish() {
  [ "$finished" = 0 ] || return 0
  finished=1
  local cs=0 ccs=n/a rc
  cleanup_owned || cs=$?
  if [ "${#custody_owned_path[@]}" -gt 0 ]; then
    if [ "$cs" = 0 ]; then
      rc=0
      custody_cleanup || rc=$?
      case "$rc" in
        0) ccs=0 ;;
        2) ccs=uncertain ;;
        *) ccs=1 ;;
      esac
    else
      ccs=skipped
      cu_log "RETAINED host custody setup: worker/controller/cgroup cleanup is not proven"
    fi
    [ "$ccs" = 0 ] || cs=1
  fi
  cleanup_status=$cs
  custody_cleanup_status=$ccs
  if ! write_result; then
    echo "RESULT NOT PUBLISHED: result.txt keeps its previous content" >&2
    return 1
  fi
  own_evidence
  [ "$result_verdict" = PASS ]
}

# Evidence written by root stays readable and sealable by the runner user.
own_evidence() {
  if [ "$(id -u)" = 0 ] && [ -e "$out/provenance.txt" ]; then
    chown -R "$(stat -c %u "$out/provenance.txt"):$(stat -c %g "$out/provenance.txt")" "$out"
  fi
}

# EXIT: finish if nothing has, keeping any nonzero status the run already had.
on_exit() {
  local rc=$?
  if ! finish; then
    [ "$rc" != 0 ] || rc=1
  fi
  exit "$rc"
}

# SIGTERM/SIGINT/SIGHUP (job cancel or timeout): record it, clean what is
# owned if cleanup has not started, never write PASS, exit 128+n.
on_signal() { # NAME NUMBER
  trap '' TERM INT HUP
  interrupted=$1
  cu_log "INTERRUPTED by $1"
  finish || true
  if [ "$finalized" != 1 ]; then # cleanup or publication was interrupted midway
    write_result || echo "RESULT NOT PUBLISHED after $1: result.txt keeps its previous content" >&2
  fi
  trap - EXIT
  exit $((128 + $2))
}

# One hosted test, bounded and with a sanitized environment (overridden only
# by the unprivileged self-check).
run_test() {
  timeout --kill-after=10 300 env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    RECEIPTS_RA_HOSTED_CGROUP="$cgroup" RECEIPTS_RA_HOSTED_WORKER="$worker" \
    "$1" "$2" --exact --ignored --nocapture --test-threads=1 \
    >>"$out/diagnostic.log" 2>&1
}

# Everything after the guards. Order: INCOMPLETE placeholder -> refuse an
# occupied cgroup (nothing created yet) -> traps -> account for and create the
# cgroup -> host custody setup -> tests only if setup verified -> owned cleanup
# and verdict. test_status is set only after EVERY test has run.
hosted_sequence() {
  local name rc=0
  if ! write_incomplete; then
    echo "FAIL could not publish the INCOMPLETE placeholder; nothing was created" >&2
    exit 1
  fi
  if [ -e "$cgroup" ] || [ -L "$cgroup" ]; then
    echo "FAIL cgroup occupied" >"$out/result.txt.tmp"
    publish_result "$out/result.txt.tmp"
    own_evidence
    exit 1
  fi
  trap on_exit EXIT
  trap 'on_signal TERM 15' TERM
  trap 'on_signal INT 2' INT
  trap 'on_signal HUP 1' HUP
  cgroup_pending=1 # recorded BEFORE the side effect
  mkdir "$cgroup"
  cgroup_pending=0
  if custody_setup; then
    setup_status=0
    for name in "${tests[@]}"; do
      rc=0
      run_test "$1" "$name" || rc=$?
      tests_completed=$((tests_completed + 1))
      if [ "$rc" != 0 ] && [ -z "$first_failed_test" ]; then
        first_failed_test=$name
        first_failed_status=$rc
      fi
    done
    if [ -n "$first_failed_test" ]; then test_status=$first_failed_status; else test_status=0; fi
  else
    setup_status="$custody_setup_state"
    primary_failure="host custody setup $custody_setup_state: $custody_setup_failure"
  fi
  rc=0
  finish || rc=1
  # After a successful publication cancellation is ignored: nothing changes
  # the exit status between the published verdict and exit. If publication
  # failed, a signal here still exits 128+n.
  trap - EXIT
  exit "$rc"
}

hosted() {
  test "$(id -u)" = 0
  test "$(uname -m)" = x86_64
  local binary
  binary="$(cat "$bin_record")"
  test -x "$worker"
  # Refuse occupied fixture identities instead of reusing them.
  if fixture_uid_in_use; then
    echo "FAIL identity occupied" >"$out/result.txt"
    own_evidence
    exit 1
  fi
  hosted_sequence "$binary"
}

seal() {
  mkdir -p "$out"
  for file in provenance.txt diagnostic.log result.txt; do
    [ -e "$out/$file" ] || echo "NOT RUN" >"$out/$file"
  done
  (cd "$out" && sha256sum provenance.txt diagnostic.log result.txt >SHA256SUMS)
}

# Unprivileged control test of the verdict logic against a fake cgroup tree.
# It proves that cleanup failures are never masked as PASS; real cgroup kill,
# emptiness and removal properties are proven only by the hosted run.
self_check() {
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  fixture_uid_in_use() { return 1; }
  fixture_dirs_retained() { return 1; }
  remove_cgroup_dir() { rm -rf "$1"; }
  cleanup_polls=2

  # scenario <name> <test_status> <expected verdict> <expected cleanup_status> <setup fn>
  scenario() {
    local name="$1" status="$2" verdict="$3" cleanup="$4" setup="$5" got
    out="$work/$name-out"
    cgroup="$work/$name-cgroup"
    mkdir -p "$out" "$cgroup"
    : >"$out/diagnostic.log"
    # A real cgroup-v2 directory always carries kernel control pseudo-files.
    for control in cgroup.controllers cgroup.events cgroup.freeze cgroup.kill \
      cgroup.max.depth cgroup.procs cgroup.stat cgroup.type cpu.stat memory.current \
      memory.events pids.current io.stat; do
      : >"$cgroup/$control"
    done
    "$setup"
    test_status="$status"
    cleanup_status=unknown
    setup_status=0
    tests_completed=${#tests[@]}
    first_failed_test=
    first_failed_status=
    interrupted=
    cgroup_pending=0
    custody_reset
    primary_failure=
    finished=0
    finalized=0
    if finish; then got=PASS; else got=FAIL; fi
    if [ "$got" != "$verdict" ] ||
      [ "$(sed -n 1p "$out/result.txt")" != "$verdict" ] ||
      [ "$(sed -n 2p "$out/result.txt")" != "test_status=$status" ] ||
      [ "$(sed -n 3p "$out/result.txt")" != "cleanup_status=$cleanup" ]; then
      echo "SELF-CHECK FAILED: $name"
      cat "$out/result.txt" "$out/diagnostic.log"
      exit 1
    fi
    echo "ok $name -> $verdict (test=$status cleanup=$cleanup)"
  }
  leaf() { # <empty|populated>
    mkdir -p "$cgroup/receipts-leaf"
    : >"$cgroup/receipts-leaf/cgroup.kill"
    echo "populated $([ "$1" = empty ] && echo 0 || echo 1)" >"$cgroup/receipts-leaf/cgroup.events"
  }
  setup_clean() { leaf empty; }
  setup_kill_fails() { leaf empty; rm "$cgroup/receipts-leaf/cgroup.kill"; mkdir "$cgroup/receipts-leaf/cgroup.kill"; }
  setup_never_empty() { leaf populated; }
  setup_unowned() { leaf empty; mkdir "$cgroup/stranger"; }
  setup_rmdir_fails() {
    leaf empty
    remove_cgroup_dir() { return 1; }
  }
  restore_remove() { remove_cgroup_dir() { rm -rf "$1"; }; }
  setup_stranger_file() { leaf empty; : >"$cgroup/stranger.txt"; }
  setup_unknown_control_prefix() { leaf empty; : >"$cgroup/cgroupish"; }
  setup_symlinked_control() {
    leaf empty
    rm "$cgroup/cgroup.procs"
    ln -s /nonexistent-target "$cgroup/cgroup.procs"
  }
  setup_symlinked_child() {
    leaf empty
    mkdir "$work/link-target"
    : >"$work/link-target/cgroup.kill"
    echo "populated 0" >"$work/link-target/cgroup.events"
    ln -s "$work/link-target" "$cgroup/receipts-link"
  }
  setup_dangling_child_link() { leaf empty; ln -s /nonexistent-target "$cgroup/receipts-dangling"; }
  setup_child_is_file() { leaf empty; : >"$cgroup/receipts-file"; }
  setup_process_remains() {
    leaf empty
    fixture_uid_in_use() { return 0; }
  }
  restore_uid() { fixture_uid_in_use() { return 1; }; }

  scenario clean 0 PASS 0 setup_clean
  scenario primary_fails_cleanup_ok 1 FAIL 0 setup_clean
  scenario kill_fails 0 FAIL 1 setup_kill_fails
  scenario never_empty 0 FAIL 1 setup_never_empty
  scenario unowned_entry 0 FAIL 1 setup_unowned
  scenario rmdir_fails 0 FAIL 1 setup_rmdir_fails
  restore_remove
  scenario both_fail 7 FAIL 1 setup_kill_fails
  scenario stranger_file 0 FAIL 1 setup_stranger_file
  scenario unknown_control_prefix 0 FAIL 1 setup_unknown_control_prefix
  scenario symlinked_control 0 FAIL 1 setup_symlinked_control
  scenario symlinked_child 0 FAIL 1 setup_symlinked_child
  [ ! -s "$work/link-target/cgroup.kill" ] || { echo "SELF-CHECK FAILED: wrote through a symlink"; exit 1; }
  scenario dangling_child_link 0 FAIL 1 setup_dangling_child_link
  scenario child_is_file 0 FAIL 1 setup_child_is_file
  scenario process_remains 0 FAIL 1 setup_process_remains
  restore_uid
  scenario aborted_before_tests unknown FAIL 0 setup_clean
  fixture_dirs_retained() { return 0; }
  scenario fixture_retained 0 FAIL 1 setup_clean
  echo "cgroup self-check passed: cleanup failure never yields PASS; hosted cgroup behavior NOT RUN"
  fixture_dirs_retained() { return 1; }
  # Runtime-local adaptation for this script's two tests; the Workspace
  # custody-self-check.sh stays unchanged.
  # shellcheck source=custody-self-check.sh
  . "$here/custody-self-check.sh"
  custody_self_check
}

case "${1:?prepare|hosted|seal|self-check}" in
  prepare | hosted | seal) "$1" ;;
  self-check) self_check ;;
  *)
    echo "usage: run.sh prepare|hosted|seal|self-check" >&2
    exit 2
    ;;
esac
