# Unprivileged fake-operation controls for custody.sh and the hosted sequence
# in run.sh. Sourced by `run.sh self-check`; not executable on its own.
#
# Kernel and root operations (mount, umount, mount table, namespace and anchor
# identity) are replaced by fakes. Ordinary file operations (mkdir, exclusive
# create, rm, rmdir, stat, links) run for real inside a private temp sandbox,
# so ownership tracking, refusal without touching external objects, retained
# records and cleanup verification are exercised against real objects. These
# controls do NOT prove real nsfs mounts, root ownership, the initial
# namespaces or the unchanged product admission: only the hosted run does.

custody_self_check() {
  [ "$(id -u)" != 0 ] || {
    echo "custody self-check must run unprivileged"
    exit 1
  }
  local box sb fake rc before after

  # ---- fake kernel/root operations ----
  if ! stat -c %u / >/dev/null 2>&1; then # BSD stat on a workstation
    cu_meta() {
      local raw
      raw="$(stat -f '%HT|%u|%Lp|%l|%d:%i' -- "$1")" || return 1
      cu_meta_norm "$raw"
    }
  fi
  cu_mount_table() { cat "$fake/mounts"; }
  cu_ns_id() {
    local who=$2
    [ "$who" != 1 ] || who=pid1
    eval "printf '%s\\n' \"\${f_${who}_$1}\""
  }
  fake_key() { printf '%s' "$1" | tr '/' '_'; }
  cu_anchor_id() {
    local file="$fake/id.$(fake_key "$1")"
    [ -f "$file" ] && cat "$file"
  }
  cu_mount_bind() {
    local ns=${1##*/} name=${2##*/} id
    echo "bind $1 $2" >>"$fake/calls"
    [ ! -e "$fake/bind-fails-$name" ] || return 1
    id="$(cu_ns_id "$ns" 1)"
    [ ! -e "$fake/bind-wrong-$name" ] || id=9:999
    printf '%s\n' "$id" >"$fake/id.$(fake_key "$2")"
    echo "100 1 0:4 $ns:[1] $2 rw - nsfs nsfs rw" >>"$fake/mounts"
  }
  cu_umount() {
    echo "umount $1" >>"$fake/calls"
    [ ! -e "$fake/umount-fails" ] || return 1
    awk -v p="$1" '$5 != p' "$fake/mounts" >"$fake/mounts.new"
    mv "$fake/mounts.new" "$fake/mounts"
    rm -f "$fake/id.$(fake_key "$1")"
  }

  # ---- sandbox and assertions ----
  fail_check() {
    echo "CUSTODY SELF-CHECK FAILED: $*"
    [ ! -f "$out/diagnostic.log" ] || cat "$out/diagnostic.log"
    [ ! -f "$out/result.txt" ] || cat "$out/result.txt"
    exit 1
  }
  check() { # description, then a command that must succeed
    local what=$1
    shift
    "$@" || fail_check "$what"
  }
  new_box() {
    box="$work/c-$1"
    sb="$box/sb"
    fake="$box/fake"
    out="$box/out"
    cgroup="$box/cgroup"
    mkdir -p "$sb/var/lib" "$fake" "$out"
    chmod 755 "$sb" "$sb/var" "$sb/var/lib"
    : >"$out/diagnostic.log"
    : >"$out/provenance.txt"
    : >"$fake/mounts"
    : >"$fake/calls"
    printf 'ID=ubuntu\nVERSION_ID="24.04"\n' >"$fake/os-release"
    custody_base="$sb/var/lib"
    custody_parent="$custody_base/receipts"
    custody_root="$custody_parent/protected-custody"
    custody_stop="$sb"
    custody_uid="$(id -u)"
    custody_os_release="$fake/os-release"
    custody_container_markers="$sb/.dockerenv $sb/run/.containerenv"
    RUNNER_ENVIRONMENT=github-hosted RUNNER_OS=Linux RUNNER_ARCH=X64 ImageOS=ubuntu24 ImageVersion=fake
    f_self_pid=4:4026531836 f_pid1_pid=4:4026531836
    f_self_user=4:4026531837 f_pid1_user=4:4026531837
    f_self_mnt=4:4026532500 f_pid1_mnt=4:4026532500
    custody_reset
    test_status=unknown cleanup_status=unknown setup_status=unknown
    custody_cleanup_status=unknown primary_failure= finished=0
    fake_test_rc=0
  }
  snap() { # kind, owner, mode and identity of every sandbox object
    local p
    find "$sb" | LC_ALL=C sort | while IFS= read -r p; do
      printf '%s %s\n' "$p" "$(cu_meta "$p")"
    done
  }
  calls() { grep -c "^$1" "$fake/calls" || true; }
  mounts() { cu_count_lines "$(cat "$fake/mounts")"; }
  log_has() { grep -q -- "$1" "$out/diagnostic.log"; }
  result_has() { grep -qx -- "$1" "$out/result.txt"; }
  result_prefix() { grep -q -- "^$1" "$out/result.txt"; }
  failure_has() { case "$custody_setup_failure" in *"$1"*) return 0 ;; *) return 1 ;; esac }
  setup_rc() {
    rc=0
    custody_setup || rc=$?
  }
  ok() { echo "ok custody $*"; }
  exist_all() { # every path exists (as a file, directory or link)
    local f
    for f in "$@"; do [ -e "$f" ] || [ -L "$f" ] || return 1; done
  }
  exist_none() {
    local f
    for f in "$@"; do ! { [ -e "$f" ] || [ -L "$f" ]; } || return 1; done
  }
  result_lacks() { ! result_prefix "$1"; }

  # ---- clean setup and teardown ----
  new_box clean
  before="$(snap)"
  setup_rc
  check "clean setup succeeds" [ "$rc" = 0 ]
  check "state is 0" [ "$custody_setup_state" = 0 ]
  check "two nsfs mounts" [ "$(mounts)" = 2 ]
  check "entries are lock, host-pid and host-mnt" [ "$(cu_list_dir "$custody_root")" = "$(printf 'host-mnt\nhost-pid\nlock')" ]
  cu_read_meta "$custody_root"
  check "root mode 0700" [ "$m_mode" = 700 ]
  cu_read_meta "$custody_root/lock"
  check "lock 0600 single-link file" [ "$m_kind|$m_mode|$m_nlink" = "file|600|1" ]
  check "seven owned resources (two directories, three files, two mounts)" [ "${#custody_owned_path[@]}" = 7 ]
  check "provenance carries observed namespace facts" grep -q '^custody_ns_pid self=4:4026531836 pid1=4:4026531836' "$out/provenance.txt"
  rc=0
  custody_cleanup || rc=$?
  check "clean cleanup succeeds" [ "$rc" = 0 ]
  check "sandbox restored exactly" [ "$(snap)" = "$before" ]
  check "no mounts remain" [ "$(mounts)" = 0 ]
  check "exactly the owned anchors were unmounted" [ "$(calls umount)" = 2 ]
  check "cleanup logged" log_has 'cleanup verified'
  ok "clean setup and teardown"

  # ---- unsupported or pre-existing state refuses without touching anything ----
  refuse_case() { # name expected-failure-text mutator
    new_box "refuse-$1"
    "$3"
    before="$(snap)"
    setup_rc
    check "$1 refuses" [ "$rc" != 0 ]
    check "$1 state is refused" [ "$custody_setup_state" = refused ]
    check "$1 owns nothing" [ "${#custody_owned_path[@]}" = 0 ]
    check "$1 names its reason" failure_has "$2"
    check "$1 left the tree untouched" [ "$(snap)" = "$before" ]
    check "$1 mounted nothing" [ "$(calls bind)$(calls umount)" = 00 ]
    rc=0
    custody_cleanup || rc=$?
    check "$1 cleanup has nothing to do" [ "$rc" = 0 ]
    check "$1 cleanup left the tree untouched" [ "$(snap)" = "$before" ]
    check "$1 failure is logged" log_has 'CUSTODY FAIL'
    ok "refuses $1"
  }
  m_parent_exists() {
    mkdir "$custody_parent"
    echo keep >"$custody_parent/sentinel"
  }
  refuse_case parent_exists "already exists" m_parent_exists
  check "pre-existing sentinel kept" [ "$(cat "$custody_parent/sentinel")" = keep ]
  m_parent_symlink() {
    mkdir "$sb/external"
    ln -s "$sb/external" "$custody_parent"
  }
  refuse_case parent_symlink "already exists" m_parent_symlink
  m_parent_dangling() { ln -s /nonexistent-target "$custody_parent"; }
  refuse_case parent_dangling_symlink "already exists" m_parent_dangling
  m_mount_below() { echo "1 1 0:4 / $custody_parent/x rw - tmpfs tmpfs rw" >>"$fake/mounts"; }
  refuse_case existing_mount_below "mounts already exist" m_mount_below
  m_group_writable() { chmod 775 "$custody_base"; }
  refuse_case ancestor_group_writable "unsafe ancestor" m_group_writable
  m_ancestor_link() {
    mv "$custody_base" "$sb/real-lib"
    ln -s "$sb/real-lib" "$custody_base"
  }
  refuse_case ancestor_symlink "unsafe ancestor" m_ancestor_link
  m_other_owner() { custody_uid=0; } # the unprivileged owner is not root
  refuse_case ancestor_wrong_owner "unsafe ancestor" m_other_owner
  m_self_hosted() { RUNNER_ENVIRONMENT=self-hosted; }
  refuse_case self_hosted "RUNNER_ENVIRONMENT" m_self_hosted
  m_wrong_image() { ImageOS=ubuntu22; }
  refuse_case wrong_image "runner context" m_wrong_image
  m_wrong_os() { printf 'ID=ubuntu\nVERSION_ID="22.04"\n' >"$custody_os_release"; }
  refuse_case wrong_os_release "Ubuntu 24.04" m_wrong_os
  m_container_marker() { : >"$sb/.dockerenv"; }
  refuse_case container_marker "container marker" m_container_marker
  # A container has its own PID 1, so self == PID 1 there; only the fixed
  # initial-namespace inode distinguishes the host.
  m_private_pid_ns() { f_self_pid=4:4026532999 f_pid1_pid=4:4026532999; }
  refuse_case private_pid_namespace "initial PID namespace" m_private_pid_ns
  m_private_user_ns() { f_self_user=4:4026532998; }
  refuse_case private_user_namespace "initial user namespace" m_private_user_ns
  m_mnt_differs() { f_self_mnt=4:4026532501; }
  refuse_case mount_namespace_differs "mount namespace differs" m_mnt_differs

  # ---- partial setup failure: every created object is tracked and unwound ----
  partial_case() { # name flag-file expected-failure-text expected-owned-count expected-umounts
    new_box "partial-$1"
    : >"$fake/$2"
    before="$(snap)"
    setup_rc
    check "$1 setup fails" [ "$rc" != 0 ]
    check "$1 state is failed" [ "$custody_setup_state" = failed ]
    check "$1 names its reason" failure_has "$3"
    check "$1 tracked its partial setup" [ "${#custody_owned_path[@]}" = "$4" ]
    rm -f "$fake/$2" # cleanup itself must work
    rc=0
    custody_cleanup || rc=$?
    check "$1 cleanup succeeds" [ "$rc" = 0 ]
    check "$1 tree restored" [ "$(snap)" = "$before" ]
    check "$1 no mount remains" [ "$(mounts)" = 0 ]
    check "$1 unmounted only what it mounted" [ "$(calls umount)" = "$5" ]
    ok "partial setup $1 unwinds"
  }
  partial_case bind_pid_fails bind-fails-host-pid "bind mount of the pid" 4 0
  partial_case bind_mnt_fails bind-fails-host-mnt "bind mount of the mnt" 6 1
  partial_case namespace_mismatch bind-wrong-host-mnt "does not match the mnt namespace" 7 2

  # ---- verification catches a setup that is not exactly right ----
  tamper_case() { # name expected-failure-text mutator
    new_box "tamper-$1"
    setup_rc
    check "$1 baseline setup ok" [ "$rc" = 0 ]
    "$3"
    custody_setup_failure=
    rc=0
    custody_verify || rc=$?
    check "$1 verification fails" [ "$rc" != 0 ]
    check "$1 names its reason" failure_has "$2"
    ok "verify rejects $1"
  }
  t_root_mode() { chmod 750 "$custody_root"; }
  tamper_case root_mode "not mode 0700" t_root_mode
  t_root_group_writable() { chmod 770 "$custody_root"; }
  tamper_case root_group_writable "unsafe ancestor" t_root_group_writable
  t_lock_mode() { chmod 644 "$custody_root/lock"; }
  tamper_case lock_mode "lock is not" t_lock_mode
  t_lock_hardlink() { ln "$custody_root/lock" "$sb/alias"; }
  tamper_case lock_hardlink "lock is not" t_lock_hardlink
  t_lock_symlink() {
    rm "$custody_root/lock"
    ln -s /nonexistent-target "$custody_root/lock"
  }
  tamper_case lock_symlink "lock is not" t_lock_symlink
  t_stray_entry() { : >"$custody_root/stray"; }
  tamper_case stray_entry "unexpected entries" t_stray_entry
  t_foreign_fs() { sed 's/ nsfs nsfs / tmpfs tmpfs /' "$fake/mounts" >"$fake/m" && mv "$fake/m" "$fake/mounts"; }
  tamper_case anchor_not_nsfs "not exactly one nsfs" t_foreign_fs
  t_stacked() { echo "101 1 0:4 pid:[1] $custody_root/host-pid rw - nsfs nsfs rw" >>"$fake/mounts"; }
  tamper_case anchor_stacked "not exactly one nsfs" t_stacked
  t_id_mismatch() { echo 9:999 >"$fake/id.$(fake_key "$custody_root/host-mnt")"; }
  tamper_case anchor_identity_mismatch "does not match the mnt namespace" t_id_mismatch
  t_nested_mount() { echo "102 1 0:5 / $custody_root/x rw - tmpfs tmpfs rw" >>"$fake/mounts"; }
  tamper_case nested_mount "unexpected mounts below" t_nested_mount

  # ---- cleanup preserves uncertainty and never acts on foreign objects ----
  cleanup_fail_case() { # name expected-log-text mutator
    new_box "cleanup-$1"
    setup_rc
    check "$1 baseline setup ok" [ "$rc" = 0 ]
    "$3"
    rc=0
    custody_cleanup || rc=$?
    check "$1 cleanup fails" [ "$rc" != 0 ]
    check "$1 logs its reason" log_has "$2"
  }
  c_record() {
    printf 'receipts-registry-v1\nretained\n' >"$custody_root/r-attempt"
    cp "$custody_root/r-attempt" "$fake/record.copy"
  }
  cleanup_fail_case retained_record RETAINED_ENTRY c_record
  check "retained record intact" cmp -s "$custody_root/r-attempt" "$fake/record.copy"
  check "retained record kept every anchor mounted" [ "$(mounts)" = 2 ]
  check "retained record kept lock and files" exist_all "$custody_root/lock" "$custody_root/host-pid" "$custody_root/host-mnt"
  check "retained record triggered no unmount" [ "$(calls umount)" = 0 ]
  ok "cleanup retains custody records and every owned resource"
  c_unmount_fails() { : >"$fake/umount-fails"; }
  cleanup_fail_case unmount_fails UNMOUNT_FAILED c_unmount_fails
  check "failed unmount left the mount" [ "$(mounts)" = 2 ]
  check "failed unmount kept the anchor files" exist_all "$custody_root/host-pid" "$custody_root/host-mnt"
  check "failed unmount kept the directories" exist_all "$custody_root" "$custody_parent"
  ok "cleanup reports an unmount failure and removes nothing under a live mount"
  c_foreign_mount() { sed 's/ nsfs nsfs / tmpfs tmpfs /' "$fake/mounts" >"$fake/m" && mv "$fake/m" "$fake/mounts"; }
  cleanup_fail_case foreign_mount UNVERIFIED_MOUNT c_foreign_mount
  check "foreign mounts were never unmounted" [ "$(calls umount)" = 0 ]
  check "foreign mounts remain" [ "$(mounts)" = 2 ]
  ok "cleanup refuses to unmount a mount that is not the owned nsfs mount"
  c_mount_identity() { echo 9:999 >"$fake/id.$(fake_key "$custody_root/host-mnt")"; }
  cleanup_fail_case mount_identity UNVERIFIED_MOUNT c_mount_identity
  check "changed mount identity was not unmounted" [ "$(calls "umount $custody_root/host-mnt")" = 0 ]
  ok "cleanup refuses a mount whose identity changed"
  c_mount_missing() {
    : >"$fake/mounts"
    rm -f "$fake"/id.*
  }
  cleanup_fail_case mount_missing UNVERIFIED_MOUNT c_mount_missing
  ok "cleanup reports a mount that vanished as unknown state"
  c_lock_replaced() { # created while the original exists, so it is a different object
    : >"$custody_root/lock.new"
    rm "$custody_root/lock"
    mv "$custody_root/lock.new" "$custody_root/lock"
  }
  cleanup_fail_case lock_replaced "REPLACED .*/protected-custody/lock" c_lock_replaced
  check "replaced lock was left in place" exist_all "$custody_root/lock"
  ok "cleanup does not remove a replaced lock"
  c_lock_alias() { ln "$custody_root/lock" "$sb/alias"; }
  cleanup_fail_case lock_alias "LINKED .*/protected-custody/lock" c_lock_alias
  check "aliased lock and alias remain" exist_all "$custody_root/lock" "$sb/alias"
  ok "cleanup does not remove a multiply linked lock"
  c_root_swapped() {
    mv "$custody_root" "$sb/moved"
    ln -s "$sb/moved" "$custody_root"
  }
  cleanup_fail_case root_swapped "REPLACED .*/protected-custody (not the object" c_root_swapped
  check "nothing removed through the swapped link" exist_all "$sb/moved/lock" "$sb/moved/host-pid" "$sb/moved/host-mnt"
  check "no unmount through the swapped link" [ "$(calls umount)" = 0 ]
  ok "cleanup never acts through a swapped directory link"

  # ---- finish(): cleanup failure and unknown setup never become PASS ----
  finish_case() { # name expected-verdict expected-cleanup expected-custody-cleanup [flag]
    new_box "finish-$1"
    setup_rc
    check "$1 baseline setup ok" [ "$rc" = 0 ]
    setup_status=0
    test_status=0
    [ -z "${5:-}" ] || : >"$fake/$5"
    if finish; then rc=PASS; else rc=FAIL; fi
    check "$1 verdict" [ "$rc" = "$2" ]
    check "$1 result verdict" [ "$(sed -n 1p "$out/result.txt")" = "$2" ]
    check "$1 cleanup status" result_has "cleanup_status=$3"
    check "$1 custody cleanup status" result_has "custody_cleanup_status=$4"
    check "$1 setup status" result_has "setup_status=0"
  }
  finish_case custody_clean PASS 0 0
  check "clean finish removed every mount" [ "$(mounts)" = 0 ]
  check "clean finish removed the directories" exist_none "$custody_parent"
  ok "finish passes only after verified custody cleanup"
  finish_case custody_unmount_fails FAIL 1 1 umount-fails
  ok "finish fails when owned custody cannot be removed"
  fixture_uid_in_use() { return 0; }
  finish_case worker_cleanup_unproven FAIL 1 skipped
  fixture_uid_in_use() { return 1; }
  check "custody mounts retained while worker cleanup is unproven" [ "$(mounts)" = 2 ]
  check "no unmount while worker cleanup is unproven" [ "$(calls umount)" = 0 ]
  check "retention is logged" log_has 'RETAINED host custody setup'
  ok "finish retains custody when worker/cgroup cleanup is unproven"
  new_box finish-unknown-setup
  test_status=0
  if finish; then rc=PASS; else rc=FAIL; fi
  check "unknown setup status is FAIL" [ "$rc" = FAIL ]
  check "unknown setup recorded" result_has setup_status=unknown
  ok "an unknown setup status is never PASS"

  # ---- the hosted sequence, with fake test runs ----
  run_test() {
    echo "$2" >>"$fake/tests-run"
    return "$fake_test_rc"
  }
  seq_run() { # sets rc; the subshell keeps set -e and its own EXIT trap
    set +e
    (
      set -e
      "${seq_prologue:-:}"
      hosted_sequence "$fake/binary"
    )
    rc=$?
    set -e
  }
  ran() { cu_count_lines "$(cat "$fake/tests-run" 2>/dev/null || true)"; }
  seq_prologue=:
  new_box seq-pass
  before="$(snap)"
  seq_run
  check "sequence passes" [ "$rc" = 0 ]
  check "all three tests ran" [ "$(ran)" = 3 ]
  check "PASS recorded with every status" result_has PASS
  check "setup status 0" result_has setup_status=0
  check "tests status 0" result_has test_status=0
  check "cleanup status 0" result_has cleanup_status=0
  check "sequence restored the sandbox" [ "$(snap)" = "$before" ]
  check "no failure line on success" result_lacks primary_failure
  ok "sequence: verified setup, tests, verified cleanup -> PASS"

  new_box seq-refused
  RUNNER_ENVIRONMENT=self-hosted
  seq_run
  check "refused sequence fails" [ "$rc" = 1 ]
  check "no test ran" [ "$(ran)" = 0 ]
  check "FAIL recorded" result_has FAIL
  check "tests NOT RUN" result_has test_status=unknown
  check "setup refused" result_has setup_status=refused
  check "cleanup clean" result_has cleanup_status=0
  check "primary failure recorded" result_prefix 'primary_failure=host custody setup refused: RUNNER_ENVIRONMENT'
  check "refusal mounted nothing" [ "$(calls bind)$(calls umount)" = 00 ]
  check "refusal created no custody objects" exist_none "$custody_parent"
  ok "sequence: unsupported context refuses, tests NOT RUN, evidence honest"

  new_box seq-partial
  : >"$fake/bind-fails-host-mnt"
  seq_run
  check "partial sequence fails" [ "$rc" = 1 ]
  check "no test ran after failed setup" [ "$(ran)" = 0 ]
  check "setup failed" result_has setup_status=failed
  check "partial setup was unwound" result_has cleanup_status=0
  check "partial cleanup recorded" result_has custody_cleanup_status=0
  check "primary failure preserved" result_prefix 'primary_failure=host custody setup failed: bind mount of the mnt namespace failed'
  check "partial setup removed" exist_none "$custody_parent"
  check "partial setup mounts removed" [ "$(mounts)" = 0 ]
  ok "sequence: partial setup failure is unwound and the primary failure is kept"

  new_box seq-partial-cleanup-fails
  : >"$fake/bind-fails-host-mnt"
  : >"$fake/umount-fails"
  seq_run
  check "double failure fails" [ "$rc" = 1 ]
  check "primary failure still first" result_prefix 'primary_failure=host custody setup failed: bind mount of the mnt namespace failed'
  check "cleanup failure reported separately" result_has cleanup_status=1
  check "custody cleanup failure reported" result_has custody_cleanup_status=1
  check "partial state retained for evidence" [ "$(mounts)" = 1 ]
  ok "sequence: cleanup failure cannot hide the primary failure"

  new_box seq-test-fails
  fake_test_rc=3
  seq_run
  check "failing test fails the sequence" [ "$rc" = 1 ]
  check "test status kept" result_has test_status=3
  check "cleanup still verified" result_has cleanup_status=0
  check "all tests were attempted" [ "$(ran)" = 3 ]
  check "custody removed after failing tests" exist_none "$custody_parent"
  ok "sequence: test failure is preserved and custody still cleaned"

  new_box seq-cleanup-fails
  : >"$fake/umount-fails"
  seq_run
  check "tests passed but cleanup failure fails the sequence" [ "$rc" = 1 ]
  check "test status 0" result_has test_status=0
  check "cleanup status 1" result_has cleanup_status=1
  check "FAIL recorded" result_has FAIL
  ok "sequence: passing tests with failed custody cleanup is FAIL"

  new_box seq-abort
  seq_prologue=abort_bind
  abort_bind() { cu_mount_bind() { exit 9; }; }
  before="$(snap)"
  seq_run
  seq_prologue=:
  check "aborted sequence fails" [ "$rc" = 1 ]
  check "no test ran" [ "$(ran)" = 0 ]
  check "abort leaves setup unknown" result_has setup_status=unknown
  check "tests NOT RUN" result_has test_status=unknown
  check "EXIT trap unwound the partial setup" result_has cleanup_status=0
  check "partial setup removed after abort" exist_none "$custody_parent"
  check "no mounts after abort" [ "$(mounts)" = 0 ]
  ok "sequence: an abort mid-setup is unwound by the EXIT trap and is FAIL"

  new_box seq-worker-unproven
  seq_prologue=process_stays
  process_stays() { fixture_uid_in_use() { return 0; }; }
  seq_run
  seq_prologue=:
  check "unproven worker cleanup fails" [ "$rc" = 1 ]
  check "custody retained" result_has custody_cleanup_status=skipped
  check "mounts retained" [ "$(mounts)" = 2 ]
  check "nothing unmounted" [ "$(calls umount)" = 0 ]
  ok "sequence: custody is retained, not removed, while worker cleanup is unproven"

  new_box seq-cgroup-present
  mkdir "$cgroup"
  before="$(snap)"
  seq_run
  check "occupied cgroup refuses" [ "$rc" != 0 ]
  check "nothing ran" [ "$(ran)" = 0 ]
  check "nothing created" exist_none "$custody_parent"
  check "nothing mounted" [ "$(calls bind)" = 0 ]
  check "pre-existing cgroup directory untouched" [ -d "$cgroup" ]
  check "occupied cgroup is reported, not silent" result_has "FAIL cgroup occupied"
  ok "sequence: a pre-existing cgroup refuses before custody setup"

  echo "custody self-check passed: setup, refusal, partial failure, retained records and cleanup failure never yield PASS; real nsfs/root/namespace behavior NOT RUN"
}
