#!/usr/bin/env bash
# Downloads the pinned scanners, verifies their archive SHA-256 against
# pins.env, and observes the RustSec advisory dataset ONCE: its origin,
# commit, tree, content identity and the observation time are recorded, and
# the adopted 24-hour execution window starts at that observation. The
# dataset is never updated afterwards. Every external command is bounded
# (TERM then KILL); an abnormal exit removes partial tools and any
# tools.env, and keeps the acquisition receipt under <work-dir>/evidence.
# Usage: acquire.sh <work-dir>
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=pins.env
. "$here/pins.env"
# shellcheck source=lib.sh
. "$here/lib.sh"
dir=${1:?work dir}
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ;;
  *) echo "::error::pinned scanner archives exist for Linux x86_64 only"; exit 1 ;;
esac
mkdir -p "$dir/archives" "$dir/tools" "$dir/evidence"
receipt="$dir/evidence/acquire-receipt.txt"
: > "$receipt"
ok=0
on_exit() {
  local rc=$?
  trap - EXIT
  reap_current
  if [ "$ok" -ne 1 ]; then
    # A partial acquisition must never look usable: no tools.env, no
    # half-extracted tools or half-cloned dataset. The receipt stays.
    rm -f "$dir/tools.env" "$dir/tools.env.tmp"
    rm -rf "$dir/archives" "$dir/tools" "$dir/advisory-db"
    echo "ACQUISITION_FAILED exit=$rc" >> "$receipt"
    [ "$rc" -ne 0 ] || rc=1
  fi
  exit "$rc"
}
trap on_exit EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP
log() { echo "$*" | tee -a "$receipt"; }

fetch() { # name url sha256
  local name=$1 url=$2 want=$3 file="$dir/archives/$1.tgz" got
  bounded "$FETCH_TIMEOUT_SECONDS" curl --proto '=https' --tlsv1.2 --fail --silent \
    --show-error --location --max-time 120 --retry 2 --output "$file" "$url"
  got=$(sha256_file "$file")
  if [ "$got" != "$want" ]; then
    echo "::error::$name archive digest $got does not match pinned $want"
    exit 1
  fi
  mkdir -p "$dir/tools/$name"
  bounded "$VERSION_TIMEOUT_SECONDS" tar -xzf "$file" -C "$dir/tools/$name"
  log "archive $name url=$url sha256=$got"
}
fetch cargo-audit "$CARGO_AUDIT_URL" "$CARGO_AUDIT_SHA256"
fetch gitleaks "$GITLEAKS_URL" "$GITLEAKS_SHA256"
fetch actionlint "$ACTIONLINT_URL" "$ACTIONLINT_SHA256"

locate() { find "$dir/tools/$1" -type f -name "$1" -perm -u+x | head -n1; }
audit=$(locate cargo-audit); leaks=$(locate gitleaks); lint=$(locate actionlint)
for t in "$audit" "$leaks" "$lint"; do
  [ -n "$t" ] || { echo "::error::extracted tool missing"; exit 1; }
done

# Version commands are bounded like every other tool run.
tool_version() { # name binary args...
  local name=$1 bin=$2 out="$dir/evidence/version-$1.txt"
  shift 2
  if ! bounded "$VERSION_TIMEOUT_SECONDS" "$bin" "$@" > "$out" 2>&1; then
    echo "::error::$name version command failed or timed out"
    exit 1
  fi
  log "tool $name binary_sha256=$(sha256_file "$bin") version=$(head -n1 "$out")"
}
tool_version cargo-audit "$audit" --version
tool_version gitleaks "$leaks" version
tool_version actionlint "$lint" -version

# ---- the dataset: observed once ----------------------------------------------
db="$dir/advisory-db"
bounded "$FETCH_TIMEOUT_SECONDS" git clone --quiet --filter=blob:none --no-tags "$ADVISORY_DB_URL" "$db"
observed=$(now_epoch)
validate_dataset "$db" "$ADVISORY_DB_URL" "$ADVISORY_DB_FLOOR" "$observed" || exit 1
origin=$ADVISORY_DB_URL
commit=$DS_COMMIT
tree=$DS_TREE
committed=$DS_COMMITTED
content=$DS_CONTENT
expires=$((observed + ADVISORY_DB_WINDOW_SECONDS))
log "dataset origin=$origin commit=$commit tree=$tree committed=$committed"
log "dataset content_sha256=$content observed_at=$observed ($(utc_of "$observed")) window=${ADVISORY_DB_WINDOW_SECONDS}s expires_at=$expires ($(utc_of "$expires"))"

{
  echo "CARGO_AUDIT=$audit"
  echo "GITLEAKS=$leaks"
  echo "ACTIONLINT=$lint"
  echo "CARGO_AUDIT_BIN_SHA256=$(sha256_file "$audit")"
  echo "GITLEAKS_BIN_SHA256=$(sha256_file "$leaks")"
  echo "ACTIONLINT_BIN_SHA256=$(sha256_file "$lint")"
  echo "ADVISORY_DB=$db"
  echo "ADVISORY_DB_ORIGIN=$origin"
  echo "ADVISORY_DB_COMMIT=$commit"
  echo "ADVISORY_DB_TREE=$tree"
  echo "ADVISORY_DB_COMMITTED=$committed"
  echo "ADVISORY_DB_CONTENT_SHA256=$content"
  echo "ADVISORY_DB_OBSERVED_AT=$observed"
  echo "ADVISORY_DB_WINDOW_EXPIRES=$expires"
} > "$dir/tools.env.tmp"
mv "$dir/tools.env.tmp" "$dir/tools.env"
ok=1
echo "ACQUISITION_OK" >> "$receipt"
