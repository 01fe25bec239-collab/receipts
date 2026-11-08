#!/usr/bin/env bash
# Downloads the pinned scanners, verifies their archive SHA-256 against
# pins.env, and captures the RustSec advisory dataset once (its commit, tree
# and commit time are recorded; it is never updated during a run).
# Usage: acquire.sh <work-dir>
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=pins.env
. "$here/pins.env"
dir=${1:?work dir}
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ;;
  *) echo "::error::pinned scanner archives exist for Linux x86_64 only"; exit 1 ;;
esac
mkdir -p "$dir/archives" "$dir/tools"

fetch() { # name url sha256
  local name=$1 url=$2 want=$3 file="$dir/archives/$1.tgz" got
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
    --max-time 120 --retry 2 --output "$file" "$url"
  got=$(sha256sum "$file" | cut -d' ' -f1)
  if [ "$got" != "$want" ]; then
    echo "::error::$name archive digest $got does not match pinned $want"
    exit 1
  fi
  mkdir -p "$dir/tools/$name"
  tar -xzf "$file" -C "$dir/tools/$name"
  echo "$name archive sha256=$got"
}
fetch cargo-audit "$CARGO_AUDIT_URL" "$CARGO_AUDIT_SHA256"
fetch gitleaks "$GITLEAKS_URL" "$GITLEAKS_SHA256"
fetch actionlint "$ACTIONLINT_URL" "$ACTIONLINT_SHA256"

locate() { find "$dir/tools/$1" -type f -name "$1" -perm -u+x | head -n1; }
audit=$(locate cargo-audit); leaks=$(locate gitleaks); lint=$(locate actionlint)
for t in "$audit" "$leaks" "$lint"; do
  [ -n "$t" ] || { echo "::error::extracted tool missing"; exit 1; }
done

db="$dir/advisory-db"
timeout 300 git clone --quiet --filter=blob:none --no-tags "$ADVISORY_DB_URL" "$db"
commit=$(git -C "$db" rev-parse HEAD)
tree=$(git -C "$db" rev-parse 'HEAD^{tree}')
committed=$(git -C "$db" show -s --format=%ct HEAD)
if ! git -C "$db" merge-base --is-ancestor "$ADVISORY_DB_FLOOR" "$commit"; then
  echo "::error::advisory dataset $commit does not descend from floor $ADVISORY_DB_FLOOR"
  exit 1
fi
age=$(( $(date +%s) - committed ))
if [ "$age" -gt "$ADVISORY_DB_MAX_AGE_SECONDS" ]; then
  echo "::error::advisory dataset head is ${age}s old (stale)"
  exit 1
fi

{
  echo "CARGO_AUDIT=$audit"
  echo "GITLEAKS=$leaks"
  echo "ACTIONLINT=$lint"
  echo "ADVISORY_DB=$db"
  echo "ADVISORY_DB_COMMIT=$commit"
  echo "ADVISORY_DB_TREE=$tree"
  echo "ADVISORY_DB_COMMITTED=$committed"
} > "$dir/tools.env"
echo "advisory-db commit=$commit tree=$tree age=${age}s"
"$audit" --version
"$leaks" version
"$lint" -version
