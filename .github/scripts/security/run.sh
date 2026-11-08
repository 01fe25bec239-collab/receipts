#!/usr/bin/env bash
# Negative controls first, then the real scans of the tested commit. Each
# control must DETECT its planted problem; a control that passes means the
# scanner cannot be trusted and the job fails. Every tool run is capped at
# 300 s; any failure, timeout, missing tool or cleanup failure exits 1.
# Usage: run.sh <work-dir> <source-checkout>
set -uo pipefail
dir=${1:?work dir}
src=${2:?source checkout}
# shellcheck source=/dev/null
. "$dir/tools.env" || { echo "::error::tools.env missing"; exit 1; }
unset GITLEAKS_CONFIG GITLEAKS_CONFIG_TOML
work="$dir/work"
rm -rf "$work"
mkdir -p "$work"
status=0
summary=()

note() { # name result detail
  summary+=("| $1 | $2 | $3 |")
  if [ "$2" != PASS ]; then status=1; echo "::error::$1: $2 $3"; fi
}
capped() { timeout 300 "$@"; }

# ---- adopted configuration -------------------------------------------------
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

audit() { # lockfile report
  CARGO_HOME="$cargo_home" capped "$CARGO_AUDIT" audit --file "$1" --db "$ADVISORY_DB" \
    --no-fetch --no-yanked --deny warnings --json > "$2" 2> "$2.stderr"
}
leaks() { # directory report
  capped "$GITLEAKS" dir "$1" --config "$work/gitleaks.toml" \
    --gitleaks-ignore-path "$work/empty.gitleaksignore" --ignore-gitleaks-allow \
    --redact=100 --report-format json --report-path "$2" --exit-code 1 \
    --no-banner --log-level warn
}

# Workflow trust policy over every *.yml in a directory; prints violations.
workflow_policy() { # dir
  local f bad=0 checkouts persisted
  for f in "$1"/*.yml; do
    grep -q '^permissions:' "$f" || { echo "$f: no top-level permissions"; bad=1; }
    if grep -nE '^[[:space:]]*[a-z-]+:[[:space:]]*write' "$f"; then echo "$f: write permission"; bad=1; fi
    if grep -nE '^[^#]*(pull_request_target|workflow_run)' "$f"; then echo "$f: privileged trigger"; bad=1; fi
    if grep -nE '^[^#]*[$][{][{][[:space:]]*secrets[.]' "$f"; then echo "$f: secret exposed to job"; bad=1; fi
    if grep -nE '^[[:space:]]*(- )?uses:' "$f" | grep -vE 'uses:[[:space:]]*(\./|[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}([[:space:]]|$))'; then
      echo "$f: action not pinned to a full commit SHA"; bad=1
    fi
    checkouts=$(grep -cE 'uses:[[:space:]]*actions/checkout@' "$f")
    persisted=$(grep -cE 'persist-credentials:[[:space:]]*false' "$f")
    [ "$checkouts" -le "$persisted" ] || { echo "$f: checkout persists credentials"; bad=1; }
  done
  return "$bad"
}

# ---- negative controls -----------------------------------------------------
# Planted credentials are generated here, so the repository never holds one.
mkdir -p "$work/control-secret"
key="AKIA$(LC_ALL=C tr -dc 'A-Z2-7' < /dev/urandom | head -c 16)"
pat="ghp_$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 36)"
printf 'aws_access_key_id = %s\ntoken = "%s"\n' "$key" "$pat" > "$work/control-secret/config.ini"
leaks "$work/control-secret" "$work/control-secret.json"; rc=$?
if [ "$rc" -eq 1 ] && grep -q '"RuleID"' "$work/control-secret.json"; then
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
audit "$work/control-lock/Cargo.lock" "$work/control-lock.json"; rc=$?
if [ "$rc" -ne 0 ] && grep -q 'RUSTSEC-2021-0003' "$work/control-lock.json"; then
  note control-vulnerable-lockfile PASS "RUSTSEC-2021-0003 detected (exit $rc)"
else
  note control-vulnerable-lockfile FAIL "known advisory not detected (exit $rc)"
fi

mkdir -p "$work/control-workflow"
printf 'on: push\njobs:\n  x:\n    runs-on: ubuntu-latest\n    stepz: []\n' > "$work/control-workflow/bad.yml"
capped "$ACTIONLINT" -no-color "$work/control-workflow/bad.yml" > "$work/control-workflow.txt" 2>&1; rc=$?
if [ "$rc" -eq 1 ]; then
  note control-invalid-workflow PASS "rejected (exit 1)"
else
  note control-invalid-workflow FAIL "invalid workflow accepted (exit $rc)"
fi

mkdir -p "$work/control-policy"
cat > "$work/control-policy/unsafe.yml" <<'YML'
on: pull_request_target
permissions:
  contents: write
jobs:
  x:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: echo "${{ secrets.TOKEN }}"
YML
workflow_policy "$work/control-policy" > "$work/control-policy.txt"; rc=$?
flagged=0
for want in "write permission" "privileged trigger" "secret exposed" "not pinned" "persists credentials"; do
  grep -q "$want" "$work/control-policy.txt" && flagged=$((flagged + 1))
done
if [ "$rc" -ne 0 ] && [ "$flagged" -eq 5 ]; then
  note control-unsafe-workflow PASS "trigger, write, secret, unpinned action and credentials flagged"
else
  note control-unsafe-workflow FAIL "unsafe workflow not fully flagged (exit $rc)"; cat "$work/control-policy.txt"
fi

# ---- scans of the tested commit --------------------------------------------
workflow_policy "$src/.github/workflows" > "$work/policy.txt"; rc=$?
if [ "$rc" -eq 0 ]; then note workflow-policy PASS "read-only, pinned, no secrets"
else note workflow-policy FAIL "violations follow"; cat "$work/policy.txt"; fi

if [ -e "$src/.cargo/audit.toml" ]; then
  note cargo-audit FAIL "project .cargo/audit.toml would override the adopted profile"
else
  audit "$src/Cargo.lock" "$work/audit.json"; rc=$?
  if [ "$rc" -eq 0 ]; then note cargo-audit PASS "no advisories (dataset $ADVISORY_DB_COMMIT)"
  else note cargo-audit FAIL "exit $rc; see report"; cat "$work/audit.json" "$work/audit.json.stderr"; fi
fi

# Tracked content of the exact commit only (no untracked or ignored files).
mkdir -p "$work/snapshot"
if git -C "$src" archive --format=tar HEAD | tar -x -C "$work/snapshot"; then
  leaks "$work/snapshot" "$work/gitleaks.json"; rc=$?
  if [ "$rc" -eq 0 ]; then note gitleaks PASS "no findings in tracked snapshot"
  else note gitleaks FAIL "exit $rc (redacted report follows)"; cat "$work/gitleaks.json"; fi
else
  note gitleaks FAIL "could not snapshot tracked content"
fi

(cd "$src" && capped "$ACTIONLINT" -no-color .github/workflows/*.yml) > "$work/actionlint.txt" 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then note actionlint PASS "all workflows"
else note actionlint FAIL "exit $rc"; cat "$work/actionlint.txt"; fi

# ---- report and owned cleanup ------------------------------------------------
{
  echo "Advisory dataset: $ADVISORY_DB_COMMIT (tree $ADVISORY_DB_TREE)"
  echo
  echo "| component | result | detail |"
  echo "|---|---|---|"
  printf '%s\n' "${summary[@]}"
} | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
if ! rm -rf "$work"; then
  echo "::error::cleanup of $work failed"
  status=1
fi
exit "$status"
