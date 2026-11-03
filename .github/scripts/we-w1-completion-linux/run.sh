#!/usr/bin/env bash
# WE W1 completion hosted diagnostic. Disposable GitHub VM only: the hosted
# phase runs as root and creates/deletes only its own cgroup and temp fixture.
# Never run `hosted` with sudo on a workstation.
#
# Result contract: the primary test status and the owned-cleanup status are
# recorded separately in result.txt, and PASS/exit 0 requires BOTH to be 0.
# A retained, unknown or unowned resource is a cleanup failure, never success.
set -euo pipefail

if [ "${1:-}" = self-check ]; then out=; else out="${DIAGNOSTIC_OUT:?}"; fi
bin_record="$out/test-binary"
cgroup="/sys/fs/cgroup/receipts-we-w1-diagnostic"
cleanup_polls=50
tests=(
  "protected_linux_tests::hosted_protected_worker_controller_and_cleanup"
  "protected_linux_tests::hosted_exclusive_custody_and_linked_target_refusals"
  "protected_linux_tests::hosted_cross_repository_nested_custody_refuses_without_transfer"
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
  cargo test --locked -p receipts-workspace-execution --lib --no-run \
    --message-format=json 2>/dev/null |
    sed -n 's/.*"executable":"\([^"]*receipts_workspace_execution-[^"]*\)".*/\1/p' |
    tail -n 1 >"$bin_record"
  test -x "$(cat "$bin_record")"
  sha256sum "$(cat "$bin_record")" >>"$out/provenance.txt"
}

# True when any live process runs as a fixture identity (controller, worker
# or the dedicated worker range).
fixture_uid_in_use() {
  ps -eo uid= | awk '$1 == 61001 || $1 == 61002 || ($1 >= 62000 && $1 <= 62063) { found = 1 } END { exit !found }'
}

# True when a hosted-test fixture directory is still present.
fixture_dirs_retained() {
  compgen -G "${TMPDIR:-/tmp}/receipts-workspace-test-we-hosted-*" >/dev/null
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
  if [ -e "$cgroup" ]; then
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

test_status=unknown
cleanup_status=unknown
finished=0

# Runs owned cleanup exactly once and writes the verdict. Used for the normal
# path and, through the EXIT trap, for any abort (where test_status stays
# "unknown" and the verdict is therefore FAIL).
finish() {
  [ "$finished" = 0 ] || return 0
  finished=1
  cleanup_status=0
  cleanup_owned || cleanup_status=$?
  local verdict=FAIL
  if [ "$test_status" = 0 ] && [ "$cleanup_status" = 0 ]; then
    verdict=PASS
  fi
  {
    echo "$verdict"
    echo "test_status=$test_status"
    echo "cleanup_status=$cleanup_status"
  } >"$out/result.txt"
  if [ "$(id -u)" = 0 ] && [ -e "$out/provenance.txt" ]; then
    chown -R "$(stat -c %u "$out/provenance.txt"):$(stat -c %g "$out/provenance.txt")" "$out"
  fi
  [ "$verdict" = PASS ]
}

hosted() {
  test "$(id -u)" = 0
  test "$(uname -m)" = x86_64
  # Host administration must pin the actual initial namespaces here before
  # this diagnostic; private PID 1 is never used to self-certify host origin.
  local domain=/var/lib/receipts/protected-custody
  if [ ! -d "$domain" ] || [ ! -f "$domain/lock" ] ||
    ! mountpoint -q "$domain/host-pid" || ! mountpoint -q "$domain/host-mnt"; then
    echo "FAIL host custody anchor unavailable" >"$out/result.txt"
    exit 1
  fi
  local binary
  binary="$(cat "$bin_record")"
  # Refuse occupied fixture identities instead of reusing them.
  if fixture_uid_in_use; then
    echo "FAIL identity occupied" >"$out/result.txt"
    exit 1
  fi
  test ! -e "$cgroup"
  mkdir "$cgroup"
  trap 'finish || exit 1' EXIT
  test_status=0
  for test in "${tests[@]}"; do
    timeout --kill-after=10 300 env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
      RECEIPTS_WE_HOSTED_CGROUP="$cgroup" \
      "$binary" "$test" --exact --ignored --nocapture --test-threads=1 \
      >>"$out/diagnostic.log" 2>&1 || test_status=$?
  done
  local verdict=0
  finish || verdict=1
  trap - EXIT
  exit "$verdict"
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
    finished=0
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
  echo "self-check passed: cleanup failure never yields PASS; hosted cgroup behavior NOT RUN"
}

case "${1:?prepare|hosted|seal|self-check}" in
  prepare | hosted | seal) "$1" ;;
  self-check) self_check ;;
  *)
    echo "usage: run.sh prepare|hosted|seal|self-check" >&2
    exit 2
    ;;
esac
