#!/usr/bin/env bash
# Negative controls first, then the real scans of the tested commit. Each
# control must DETECT its planted problem; a control that passes means the
# scanner cannot be trusted and the job fails. Every tool run is bounded
# (TERM then KILL, see lib.sh); any failure, timeout, missing or changed tool,
# ineligible dataset, incomplete snapshot or unknown cleanup exits 1.
#
# Evidence is RETAINED under <work-dir>/evidence: the frozen configurations,
# the control definitions, every raw report and stderr, per-step argv /
# UTC start / end / exit, tool and dataset identities, the tracked-snapshot
# manifest, a summary and SHA256SUMS. Planted secrets are never retained
# (the controls use redacted reports, and the evidence is searched for the
# planted values before the run can pass). Removal of the PLANTED inputs
# (<work-dir>/work) is separate and verified; an abnormal exit (failure,
# TERM, INT, HUP) still reaps the in-flight tool, removes the planted inputs
# and records the run as incomplete.
# Usage: run.sh <work-dir> <source-checkout>
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=pins.env
. "$here/pins.env"
# shellcheck source=lib.sh
. "$here/lib.sh"
dir=${1:?work dir}
src=${2:?source checkout}
evidence="$dir/evidence"
work="$dir/work"
mkdir -p "$evidence"
status=0
completed=0
summary=()
planted_key=""
planted_pat=""

note() { # name result detail
  summary+=("| $1 | $2 | $3 |")
  if [ "$2" != PASS ]; then status=1; echo "::error::$1: $2 $3"; fi
}

# ---- retained, redacted step record ---------------------------------------------
steps="$evidence/steps.tsv"
[ -f "$steps" ] || printf 'name\tstart_utc\tend_utc\texit\targv\n' > "$steps"
step() { # name rc start end argv...
  local name=$1 rc=$2 s=$3 e=$4
  shift 4
  printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$(utc_of "$s")" "$(utc_of "$e")" "$rc" "$*" >> "$steps"
}
# bounded run recorded as a step; returns the tool's exit status
run_step() { # name argv...
  local name=$1 s e rc
  shift
  s=$(now_epoch)
  capped "$@"
  rc=$?
  e=$(now_epoch)
  step "$name" "$rc" "$s" "$e" "$@"
  return "$rc"
}

# ---- cleanup of PLANTED inputs, evidence retention, abnormal exit ---------------
cleanup_state="NOT_RUN"
cleanup_inputs() {
  if [ ! -e "$work" ]; then cleanup_state="OK"; return 0; fi
  if rm -rf "$work" 2>/dev/null && [ ! -e "$work" ]; then cleanup_state="OK"; return 0; fi
  # Permissions can block removal; restore write access and try once more.
  chmod -R u+rwX "$work" 2>/dev/null
  if rm -rf "$work" 2>/dev/null && [ ! -e "$work" ]; then cleanup_state="OK"; return 0; fi
  cleanup_state="FAILED"
  return 1
}

write_final() { # exit-code
  local rc=$1 signal=${2:-}
  {
    echo "Source: $(git -C "$src" rev-parse HEAD 2>/dev/null) tree $(git -C "$src" rev-parse 'HEAD^{tree}' 2>/dev/null)"
    echo "Advisory dataset: ${ADVISORY_DB_COMMIT:-unknown} (tree ${ADVISORY_DB_TREE:-unknown}) origin ${ADVISORY_DB_ORIGIN:-unknown}"
    echo "Dataset observed: ${ADVISORY_DB_OBSERVED_AT:-?} window expires: ${ADVISORY_DB_WINDOW_EXPIRES:-?}"
    echo "Run started: $(utc_of "$run_started")  ended: $(utc_of "$(now_epoch)")"
    echo "Planted-input cleanup: $cleanup_state"
    if [ "$completed" -ne 1 ]; then echo "RUN INCOMPLETE: exit=$rc ${signal:+signal=$signal}"; fi
    echo
    echo "| component | result | detail |"
    echo "|---|---|---|"
    printf '%s\n' "${summary[@]:-}"
  } > "$evidence/summary.md"
  (cd "$evidence" && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z |
    while IFS= read -r -d '' f; do printf '%s  %s\n' "$(sha256_file "$f")" "$f"; done > SHA256SUMS)
  cat "$evidence/summary.md" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
}

on_exit() {
  local rc=$?
  trap - EXIT
  reap_current
  [ "$rc" -eq 0 ] || status=1
  [ "$completed" -eq 1 ] || status=1
  # Redaction guard: a retained planted value turns the run red.
  if [ -n "$planted_key$planted_pat" ] && [ -d "$evidence" ]; then
    if grep -rqF -e "$planted_key" -e "$planted_pat" "$evidence" 2>/dev/null; then
      # Never publish it: drop every file that holds a planted value.
      grep -rlF -e "$planted_key" -e "$planted_pat" "$evidence" 2>/dev/null | while IFS= read -r leaked; do
        rm -f "$leaked"
      done
      summary+=("| evidence-redaction | FAIL | a planted secret value was found in retained evidence and the file was removed |")
      status=1
      echo "::error::planted secret found in retained evidence"
    fi
  fi
  cleanup_inputs || { status=1; summary+=("| cleanup | FAIL | planted inputs could not be removed (state unknown) |"); echo "::error::cleanup of $work failed"; }
  write_final "$rc" "${ABORT_SIGNAL:-}"
  exit "$status"
}
trap on_exit EXIT
trap 'ABORT_SIGNAL=TERM; exit 143' TERM
trap 'ABORT_SIGNAL=INT; exit 130' INT
trap 'ABORT_SIGNAL=HUP; exit 129' HUP

run_started=$(now_epoch)
rm -rf "$work"
mkdir -p "$work" "$evidence/config" "$evidence/controls"

# ---- tools and dataset eligibility ----------------------------------------------
# shellcheck source=/dev/null
if ! . "$dir/tools.env"; then
  note acquisition FAIL "tools.env missing: nothing was acquired (no silent re-acquisition)"
  exit 1
fi
unset GITLEAKS_CONFIG GITLEAKS_CONFIG_TOML
command -v ruby >/dev/null 2>&1 || { note workflow-policy FAIL "ruby (YAML parser) is not available"; exit 1; }
cp "$dir/tools.env" "$evidence/tools.env"

tools_ok=1
for pair in "CARGO_AUDIT:CARGO_AUDIT_BIN_SHA256" "GITLEAKS:GITLEAKS_BIN_SHA256" "ACTIONLINT:ACTIONLINT_BIN_SHA256"; do
  tool_name=${pair%%:*}
  hash_name=${pair##*:}
  bin=${!tool_name:-}
  want=${!hash_name:-}
  if [ -z "$bin" ] || [ ! -x "$bin" ] || [ -z "$want" ] || [ "$(sha256_file "$bin")" != "$want" ]; then
    tools_ok=0
    note "tool-identity-$tool_name" FAIL "binary missing or changed since acquisition"
  fi
done

# The dataset is an immutable snapshot observed once; the adopted 24-hour
# window starts at that observation. No renewal, no re-observation here.
dataset_eligible=1
dataset_why=""
window_gate() { # stage -> 0 when inside the window
  local state now
  now=$(now_epoch)
  state=$(window_state "${ADVISORY_DB_OBSERVED_AT:-0}" "${ADVISORY_DB_WINDOW_EXPIRES:-0}" "$now")
  if [ -z "${ADVISORY_DB_OBSERVED_AT:-}" ] || [ -z "${ADVISORY_DB_WINDOW_EXPIRES:-}" ]; then
    state=UNBOUND
  fi
  case "$state" in
    OK) return 0 ;;
    NOT_YET) AUDIT_FAULT="$1: clock is before the dataset observation (anomaly)" ;;
    EXPIRED) AUDIT_FAULT="$1: the dataset's execution window expired at $(utc_of "$ADVISORY_DB_WINDOW_EXPIRES"); a fresh observation is required (never extended)" ;;
    *) AUDIT_FAULT="$1: the dataset has no recorded observation/window" ;;
  esac
  return 1
}
dataset_unchanged() {
  [ "$(git -C "$ADVISORY_DB" rev-parse HEAD 2>/dev/null)" = "${ADVISORY_DB_COMMIT:-}" ] &&
    [ "$(git -C "$ADVISORY_DB" rev-parse 'HEAD^{tree}' 2>/dev/null)" = "${ADVISORY_DB_TREE:-}" ] &&
    [ -z "$(git -C "$ADVISORY_DB" status --porcelain 2>/dev/null)" ] &&
    [ "$(dataset_content_sha "$ADVISORY_DB")" = "${ADVISORY_DB_CONTENT_SHA256:-}" ]
}
AUDIT_FAULT=""
if [ ! -d "${ADVISORY_DB:-/nonexistent}" ]; then
  dataset_eligible=0; dataset_why="dataset missing"
elif [ "$(git -C "$ADVISORY_DB" remote get-url origin 2>/dev/null)" != "${ADVISORY_DB_ORIGIN:-unset}" ] ||
  [ "${ADVISORY_DB_ORIGIN:-}" != "$ADVISORY_DB_URL" ]; then
  # ADVISORY_DB_URL comes from pins.env; the recorded origin must still be it.
  dataset_eligible=0; dataset_why="dataset origin differs from the pinned origin"
elif ! dataset_unchanged; then
  dataset_eligible=0; dataset_why="dataset identity/content differs from the observed snapshot"
elif ! window_gate "start"; then
  dataset_eligible=0; dataset_why=$AUDIT_FAULT
fi
if [ "$dataset_eligible" -eq 1 ]; then
  note dataset-eligibility PASS "observed $(utc_of "$ADVISORY_DB_OBSERVED_AT"), eligible until $(utc_of "$ADVISORY_DB_WINDOW_EXPIRES")"
else
  note dataset-eligibility FAIL "$dataset_why"
fi
AUDIT_FAULT=""
[ "$tools_ok" -eq 1 ] || { completed=0; exit 1; }

# ---- adopted configuration -----------------------------------------------------
cargo_home="$work/cargo-home"
mkdir -p "$cargo_home"
# Adopted Q-06 audit profile (same keys/values as the recorded local
# profile; only database.path differs per run).
cat > "$cargo_home/audit.toml" <<TOML
[advisories]
ignore = []
informational_warnings = ["unmaintained", "unsound", "notice"]

[database]
path = "$ADVISORY_DB"
url = "https://github.com/RustSec/advisory-db.git"
fetch = false
stale = false

[output]
deny = ["warnings"]
format = "json"
quiet = false
show_tree = true
TOML
cat > "$work/gitleaks.toml" <<'TOML'
title = "receipts Q-06: pinned Gitleaks defaults"
[extend]
useDefault = true
TOML
: > "$work/empty.gitleaksignore"
cp "$cargo_home/audit.toml" "$evidence/config/audit.toml"
cp "$work/gitleaks.toml" "$evidence/config/gitleaks.toml"
cp "$work/empty.gitleaksignore" "$evidence/config/empty.gitleaksignore"

audit() { # lockfile report name -> tool exit status, or 1 with AUDIT_FAULT set
  AUDIT_FAULT=""
  if [ "$dataset_eligible" -ne 1 ]; then AUDIT_FAULT="dataset ineligible: $dataset_why"; return 1; fi
  window_gate "before audit" || return 1
  local rc
  CARGO_HOME="$cargo_home" run_step "$3" "$CARGO_AUDIT" audit --file "$1" --db "$ADVISORY_DB" \
    --no-fetch --no-yanked --deny warnings --json > "$2" 2> "$2.stderr"
  rc=$?
  # A scan must start AND finish inside the window, on an unmodified dataset.
  window_gate "after audit" || return 1
  dataset_unchanged || { AUDIT_FAULT="dataset changed during the run"; return 1; }
  return "$rc"
}
leaks() { # directory report name
  run_step "$3" "$GITLEAKS" dir "$1" --config "$work/gitleaks.toml" \
    --gitleaks-ignore-path "$work/empty.gitleaksignore" --ignore-gitleaks-allow \
    --redact=100 --report-format json --report-path "$2" --exit-code 1 \
    --no-banner --log-level warn > "$2.log" 2>&1
}

# Workflow trust policy over every workflow in a directory: evaluated on the
# parsed YAML (block and inline forms alike), not on text.
workflow_policy() { # dir
  capped ruby "$here/workflow_policy.rb" "$1"
}

# ---- negative controls ---------------------------------------------------------
# Planted credentials are generated here, so the repository never holds one;
# only the redacted scanner reports are retained.
mkdir -p "$work/control-secret"
planted_key="AKIA$(LC_ALL=C tr -dc 'A-Z2-7' < /dev/urandom | head -c 16)"
planted_pat="ghp_$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 36)"
printf 'aws_access_key_id = %s\ntoken = "%s"\n' "$planted_key" "$planted_pat" > "$work/control-secret/config.ini"
printf '%s  control-secret/config.ini (planted values not retained)\n' "$(sha256_file "$work/control-secret/config.ini")" > "$evidence/controls/control-secret.identity"
leaks "$work/control-secret" "$evidence/controls/control-secret.json" control-secret; rc=$?
if timed_out "$rc"; then note control-planted-secret FAIL "timed out (exit $rc)"
elif [ "$rc" -eq 1 ] && grep -q '"RuleID"' "$evidence/controls/control-secret.json"; then
  note control-planted-secret PASS "detected (exit 1)"
else
  note control-planted-secret FAIL "planted secret not detected (exit $rc)"
fi

mkdir -p "$work/control-lock"
cat > "$work/control-lock/Cargo.lock" <<'LOCK'
version = 3

[[package]]
name = "receipts-security-control"
version = "0.0.0"
dependencies = ["smallvec"]

[[package]]
name = "smallvec"
version = "1.6.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
LOCK
cp "$work/control-lock/Cargo.lock" "$evidence/controls/control-lock.Cargo.lock"
audit "$work/control-lock/Cargo.lock" "$evidence/controls/control-lock.json" control-lock; rc=$?
if [ -n "$AUDIT_FAULT" ]; then note control-vulnerable-lockfile FAIL "$AUDIT_FAULT"
elif timed_out "$rc"; then note control-vulnerable-lockfile FAIL "timed out (exit $rc)"
elif [ "$rc" -ne 0 ] && grep -q 'RUSTSEC-2021-0003' "$evidence/controls/control-lock.json"; then
  note control-vulnerable-lockfile PASS "RUSTSEC-2021-0003 detected (exit $rc)"
else
  note control-vulnerable-lockfile FAIL "known advisory not detected (exit $rc)"
fi

mkdir -p "$work/control-workflow"
printf 'on: push\njobs:\n  x:\n    runs-on: ubuntu-latest\n    stepz: []\n' > "$work/control-workflow/bad.yml"
cp "$work/control-workflow/bad.yml" "$evidence/controls/control-workflow.bad.yml"
run_step control-workflow "$ACTIONLINT" -no-color "$work/control-workflow/bad.yml" > "$evidence/controls/control-workflow.txt" 2>&1; rc=$?
if timed_out "$rc"; then note control-invalid-workflow FAIL "timed out (exit $rc)"
elif [ "$rc" -eq 1 ]; then note control-invalid-workflow PASS "rejected (exit 1)"
else note control-invalid-workflow FAIL "invalid workflow accepted (exit $rc)"
fi

# Policy controls: every unsafe spelling must be flagged with its phrases and
# every safe spelling must pass (so a blanket-reject policy cannot pass).
policy_ok=1
policy_cases=0
while IFS='|' read -r file phrases; do
  case "$file" in ''|'#'*) continue ;; esac
  d="$work/control-policy/$file.d"
  mkdir -p "$d"
  cp "$here/controls/$file" "$d/$file"
  policy_cases=$((policy_cases + 1))
  workflow_policy "$d" > "$evidence/controls/policy-$file.txt" 2>&1; rc=$?
  if [ "$phrases" = PASS ]; then
    [ "$rc" -eq 0 ] || { policy_ok=0; echo "control $file: safe workflow was flagged"; }
  else
    [ "$rc" -ne 0 ] || { policy_ok=0; echo "control $file: unsafe workflow passed"; }
    IFS=',' read -ra want <<< "$phrases"
    for p in "${want[@]}"; do
      grep -q "$p" "$evidence/controls/policy-$file.txt" || { policy_ok=0; echo "control $file: '$p' not reported"; }
    done
  fi
done < "$here/controls/expected.txt"
cp "$here/controls/expected.txt" "$evidence/controls/policy-expected.txt"
if [ "$policy_ok" -eq 1 ] && [ "$policy_cases" -gt 0 ]; then
  note control-workflow-policy PASS "$policy_cases YAML spellings (block, inline, quoted, anchored, safe) behave as expected"
else
  note control-workflow-policy FAIL "policy control mismatch"
fi

# ---- scans of the tested commit --------------------------------------------------
workflow_policy "$src/.github/workflows" > "$evidence/workflow-policy.txt" 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then
  note workflow-policy PASS "parsed YAML: top-level read-only permissions, no privileged trigger, no secrets, pinned actions, no persisted credentials"
else
  note workflow-policy FAIL "violations follow"; cat "$evidence/workflow-policy.txt"
fi

if [ -e "$src/.cargo/audit.toml" ]; then
  note cargo-audit FAIL "project .cargo/audit.toml would override the adopted profile"
else
  cp "$src/Cargo.lock" "$evidence/Cargo.lock.scanned"
  audit "$src/Cargo.lock" "$evidence/audit.json" cargo-audit; rc=$?
  if [ -n "$AUDIT_FAULT" ]; then note cargo-audit FAIL "$AUDIT_FAULT"
  elif timed_out "$rc"; then note cargo-audit FAIL "timed out (exit $rc)"
  elif [ "$rc" -eq 0 ]; then note cargo-audit PASS "no advisories (dataset $ADVISORY_DB_COMMIT, window until $(utc_of "$ADVISORY_DB_WINDOW_EXPIRES"))"
  else note cargo-audit FAIL "exit $rc; see report"; cat "$evidence/audit.json" "$evidence/audit.json.stderr"
  fi
fi

# Complete tracked content of the exact commit: every index entry of HEAD,
# written with checkout-index (which, unlike `git archive`, ignores
# export-ignore). The entry list is bound and the snapshot is verified
# complete; submodule entries are unsupported content and fail.
snapshot="$work/snapshot"
mkdir -p "$snapshot"
snap_why=""
snapshot_tracked() {
  local idx="$work/snapshot.index" listing="$work/snapshot.ls" entry path n=0
  GIT_INDEX_FILE="$idx" git -C "$src" read-tree HEAD || { snap_why="cannot read HEAD tree"; return 1; }
  GIT_INDEX_FILE="$idx" git -C "$src" ls-files -s -z > "$listing" || { snap_why="cannot list entries"; return 1; }
  if tr '\0' '\n' < "$listing" | grep -q '^160000 '; then snap_why="submodule (gitlink) entries are unsupported content"; return 1; fi
  GIT_INDEX_FILE="$idx" git -C "$src" checkout-index --all --force --prefix="$snapshot/" || { snap_why="checkout-index failed"; return 1; }
  while IFS= read -r -d '' entry; do
    path=${entry#*$'\t'}
    if [ ! -e "$snapshot/$path" ] && [ ! -L "$snapshot/$path" ]; then snap_why="missing from snapshot: $path"; return 1; fi
    n=$((n + 1))
  done < "$listing"
  tr '\0' '\n' < "$listing" > "$evidence/snapshot-manifest.txt"
  echo "$n" > "$evidence/snapshot-entries.txt"
  [ "$n" -gt 0 ] || { snap_why="empty snapshot"; return 1; }
  return 0
}
if snapshot_tracked; then
  leaks "$snapshot" "$evidence/gitleaks.json" gitleaks; rc=$?
  entries=$(cat "$evidence/snapshot-entries.txt")
  if timed_out "$rc"; then note gitleaks FAIL "timed out (exit $rc)"
  elif [ "$rc" -eq 0 ]; then note gitleaks PASS "no findings in the complete tracked snapshot ($entries entries, manifest $(sha256_file "$evidence/snapshot-manifest.txt"))"
  else note gitleaks FAIL "exit $rc (redacted report retained)"; cat "$evidence/gitleaks.json"
  fi
else
  note gitleaks FAIL "tracked snapshot incomplete: $snap_why"
fi

(cd "$src" && run_step actionlint "$ACTIONLINT" -no-color .github/workflows/*.yml) > "$evidence/actionlint.txt" 2>&1; rc=$?
if timed_out "$rc"; then note actionlint FAIL "timed out (exit $rc)"
elif [ "$rc" -eq 0 ]; then note actionlint PASS "all workflows"
else note actionlint FAIL "exit $rc"; cat "$evidence/actionlint.txt"
fi

completed=1
# on_exit writes the summary, verifies planted-input cleanup and exits with
# the accumulated status.
