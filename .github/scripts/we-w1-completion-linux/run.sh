#!/usr/bin/env bash
# WE W1 completion hosted diagnostic. Disposable GitHub VM only: the hosted
# phase runs as root and creates/deletes only its own cgroup and temp fixture.
# Never run `hosted` with sudo on a workstation.
set -euo pipefail

out="${DIAGNOSTIC_OUT:?}"
bin_record="$out/test-binary"
cgroup="/sys/fs/cgroup/receipts-we-w1-diagnostic"
tests=(
  "protected_linux_tests::hosted_protected_worker_controller_and_cleanup"
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

hosted() {
  test "$(id -u)" = 0
  test "$(uname -m)" = x86_64
  local binary status=0
  binary="$(cat "$bin_record")"
  # Refuse occupied fixture identities instead of reusing them.
  for id in 61001 61002 $(seq 62000 62063); do
    if ps -eo uid= | grep -qx " *$id"; then
      echo "identity $id occupied" >"$out/result.txt"
      exit 1
    fi
  done
  test ! -e "$cgroup"
  mkdir "$cgroup"
  cleanup() {
    # Owned cleanup: kill only this diagnostic's cgroup tree, then remove it.
    if [ -d "$cgroup" ]; then
      for leaf in "$cgroup"/receipts-*; do
        [ -d "$leaf" ] || continue
        echo 1 >"$leaf/cgroup.kill" || true
        for _ in $(seq 50); do
          grep -qx 'populated 0' "$leaf/cgroup.events" && break
          sleep 0.1
        done
        rmdir "$leaf" || echo "RETAINED $leaf" >>"$out/diagnostic.log"
      done
      rmdir "$cgroup" || echo "RETAINED $cgroup" >>"$out/diagnostic.log"
    fi
  }
  trap cleanup EXIT
  for test in "${tests[@]}"; do
    timeout --kill-after=10 300 env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
      RECEIPTS_WE_HOSTED_CGROUP="$cgroup" \
      "$binary" "$test" --exact --ignored --nocapture --test-threads=1 \
      >>"$out/diagnostic.log" 2>&1 || status=$?
  done
  if [ "$status" = 0 ]; then echo PASS >"$out/result.txt"; else echo "FAIL $status" >"$out/result.txt"; fi
  chown -R "$(stat -c %u "$out/provenance.txt"):$(stat -c %g "$out/provenance.txt")" "$out"
  exit "$status"
}

seal() {
  mkdir -p "$out"
  for file in provenance.txt diagnostic.log result.txt; do
    [ -e "$out/$file" ] || echo "NOT RUN" >"$out/$file"
  done
  (cd "$out" && sha256sum provenance.txt diagnostic.log result.txt >SHA256SUMS)
}

"${1:?prepare|hosted|seal}"
