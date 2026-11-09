#!/usr/bin/env bash
# Shared helpers for acquire.sh and run.sh (sourced, never executed).
#
# Bounded execution: every external command runs under timeout(1) with a
# TERM -> KILL escalation, in its own process group, and is waited for, so a
# TERM-resistant process cannot outlive its bound and a signal to the script
# reaps the in-flight command. A process that leaves the group on purpose
# (setsid) is outside this guarantee.

: "${TOOL_TIMEOUT_SECONDS:=300}"
: "${KILL_AFTER_SECONDS:=10}"
: "${VERSION_TIMEOUT_SECONDS:=30}"
: "${FETCH_TIMEOUT_SECONDS:=300}"

if command -v timeout >/dev/null 2>&1; then TIMEOUT_BIN=timeout
elif command -v gtimeout >/dev/null 2>&1; then TIMEOUT_BIN=gtimeout
else echo "::error::timeout(1) is required for bounded execution"; exit 1
fi

CURRENT_PID=""

# bounded SECONDS CMD... : exit 124 (TERM) / 137 (KILL) mean the bound hit.
bounded() {
  local secs=$1 rc
  shift
  "$TIMEOUT_BIN" -k "$KILL_AFTER_SECONDS" -s TERM "$secs" "$@" &
  CURRENT_PID=$!
  wait "$CURRENT_PID"
  rc=$?
  CURRENT_PID=""
  return "$rc"
}
capped() { bounded "$TOOL_TIMEOUT_SECONDS" "$@"; }

timed_out() { [ "$1" -eq 124 ] || [ "$1" -eq 137 ]; }

# Reaps the in-flight bounded command after an abnormal exit: TERM its
# group, wait up to the kill-after bound, then KILL the group.
reap_current() {
  local pid=${CURRENT_PID:-} i=0
  [ -n "$pid" ] || return 0
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null
  while kill -0 "$pid" 2>/dev/null && [ "$i" -lt $((KILL_AFTER_SECONDS * 10 + 10)) ]; do
    sleep 0.1
    i=$((i + 1))
  done
  kill -KILL -- "-$pid" 2>/dev/null || true
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  CURRENT_PID=""
}

now_epoch() { date -u +%s; }
utc_of() { date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ; }

sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1
  else shasum -a 256 | cut -d' ' -f1
  fi
}
sha256_file() { sha256_stdin < "$1"; }

# Content identity of a dataset checkout: every file except .git, by path.
dataset_content_sha() { # dir
  (cd "$1" && find . -path ./.git -prune -o -type f -print0 | LC_ALL=C sort -z |
    while IFS= read -r -d '' f; do printf '%s  %s\n' "$(sha256_file "$f")" "$f"; done) | sha256_stdin
}

# ---- adopted execution window (Q-06 dataset disposition) ---------------------
# A dataset snapshot is observed ONCE at acquisition. It is eligible for
# scans that start and finish within WINDOW seconds of that observation.
# Commit age is a different quantity and is not used.

# window_state OBSERVED EXPIRES NOW -> OK | NOT_YET | EXPIRED
window_state() {
  if [ "$3" -lt "$1" ]; then echo NOT_YET
  elif [ "$3" -ge "$2" ]; then echo EXPIRED
  else echo OK
  fi
}

# commit_time_ok COMMITTED OBSERVED : a commit dated after the observation
# is an anomaly (never "negative age"), not freshness.
commit_time_ok() { [ "$1" -le "$2" ]; }

# validate_dataset DB ORIGIN FLOOR OBSERVED : checks one observed dataset and
# sets DS_COMMIT / DS_TREE / DS_COMMITTED / DS_CONTENT. Pure over a local
# checkout (acquire.sh clones it; the controls build one).
validate_dataset() {
  local db=$1 origin=$2 floor=$3 observed=$4 got
  got=$(git -C "$db" remote get-url origin 2>/dev/null) || got=""
  if [ "$got" != "$origin" ]; then
    echo "::error::advisory dataset origin '$got' is not the pinned '$origin'"
    return 1
  fi
  DS_COMMIT=$(git -C "$db" rev-parse HEAD) || return 1
  DS_TREE=$(git -C "$db" rev-parse 'HEAD^{tree}') || return 1
  DS_COMMITTED=$(git -C "$db" show -s --format=%ct HEAD) || return 1
  if ! git -C "$db" merge-base --is-ancestor "$floor" "$DS_COMMIT" 2>/dev/null; then
    echo "::error::advisory dataset $DS_COMMIT does not descend from floor $floor"
    return 1
  fi
  if ! commit_time_ok "$DS_COMMITTED" "$observed"; then
    echo "::error::advisory dataset commit time $DS_COMMITTED is after its observation $observed (timestamp or clock anomaly)"
    return 1
  fi
  if [ -n "$(git -C "$db" status --porcelain)" ]; then
    echo "::error::advisory dataset checkout is not clean"
    return 1
  fi
  DS_CONTENT=$(dataset_content_sha "$db")
}
