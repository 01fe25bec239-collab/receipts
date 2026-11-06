# Host custody ADMINISTRATION for the WE W1 completion hosted diagnostic.
# Sourced by run.sh; not executable on its own.
#
# Trusted premise (external, recorded not self-certified): the job is the
# existing same-repository, exact-head, branch-gated standard GitHub-hosted
# ubuntu-24.04 job, running directly on its fresh VM with NO job container,
# service container or self-hosted substitution. This file is host
# administration. It is distinct from the worker's self-admission: the product
# launcher never provisions these anchors and its admission checks run
# unchanged afterwards.
#
# It provides, for production_registry (src/workspace/protected_custody.rs):
#   /var/lib/receipts                              root 0755 (new, owned here)
#   /var/lib/receipts/protected-custody            root 0700 (new, owned here)
#     lock                                         root 0600 single-link file
#     host-pid / host-mnt                          nsfs bind mounts of the
#                                                  initial PID / mount namespace
# It refuses anything pre-existing, linked, unexpected or mismatched without
# touching it, records every object it actually creates (including partial
# setup) and removes only those verified objects: no recursive removal, no lazy
# or forced unmount, no chmod/chown, no adoption, no force-unlock or deletion
# of retained custody records. Nothing is fabricated: a failed or unknown step
# is a failure, never a pass.
#
# Every operation that needs the real kernel or root sits behind a small cu_*
# function so the unprivileged self-check can substitute fakes. Ordinary file
# operations (mkdir, rm, rmdir, stat) run for real against the configured paths.

custody_base=/var/lib                        # must pre-exist and be safe
custody_parent=/var/lib/receipts             # must NOT pre-exist
custody_root=/var/lib/receipts/protected-custody
custody_stop=/                               # top of the ancestor safety walk
custody_uid=0                                # required owner of safe objects
custody_os_release=/etc/os-release
custody_container_markers="/.dockerenv /run/.containerenv"
# Kernel constants (include/linux/proc_ns.h): the initial PID and user
# namespaces have fixed inode numbers, so they prove host origin where a
# container's private PID 1 cannot.
custody_init_pid_ino=4026531836
custody_init_user_ino=4026531837

custody_owned_kind=()
custody_owned_path=()
custody_owned_id=()
custody_setup_failure=
custody_setup_state=unknown

custody_reset() {
  custody_owned_kind=()
  custody_owned_path=()
  custody_owned_id=()
  custody_setup_failure=
  custody_setup_state=unknown
}

cu_log() { echo "CUSTODY $*" >>"$out/diagnostic.log"; }

# Records the FIRST failure (the primary one) on one line; later failures only
# add log lines.
cu_fail() {
  local text
  text="$(printf '%s' "$*" | tr '\n' ' ')"
  [ -n "$custody_setup_failure" ] || custody_setup_failure="$text"
  cu_log "FAIL $text"
  echo "custody: $text" >&2
}

# ---- kernel / root operations (replaced by the unprivileged self-check) ----

cu_mount_table() { cat /proc/self/mountinfo; }
cu_mount_bind() { timeout 30 mount --bind "$1" "$2"; }
cu_umount() { timeout 30 umount "$1"; } # never -l, -f or -R
# dev:inode of /proc/<self|1>/ns/<name>
cu_ns_id() { stat -L -c '%d:%i' -- "/proc/$2/ns/$1"; }
# dev:inode of the object an anchor path resolves to
cu_anchor_id() { stat -L -c '%d:%i' -- "$1"; }

# Object metadata WITHOUT following links: kind|uid|mode|nlink|dev:inode
cu_meta_norm() {
  local kind rest
  kind="$(printf '%s' "${1%%|*}" | tr 'A-Z' 'a-z')"
  rest="${1#*|}"
  case "$kind" in
    regular*file) kind=file ;;
    directory) kind=dir ;;
    symbolic*link) kind=link ;;
    *) kind=other ;;
  esac
  printf '%s|%s\n' "$kind" "$rest"
}
cu_meta() {
  local raw
  raw="$(stat -c '%F|%u|%a|%h|%d:%i' -- "$1")" || return 1
  cu_meta_norm "$raw"
}

# ---- read-only helpers ----

cu_read_meta() {
  local raw
  raw="$(cu_meta "$1")" || return 1
  IFS='|' read -r m_kind m_uid m_mode m_nlink m_id <<<"$raw"
}

cu_list_dir() { LC_ALL=C ls -A1 -- "$1"; }

# "fstype maj:min root" for every mount whose mount point is exactly $1.
cu_mounts_at() {
  cu_mount_table | awk -v p="$1" '{ for (i = 7; i < NF; i++) if ($i == "-") { if ($5 == p) print $(i + 1), $3, $4; break } }'
}

# Number of mounts at or below $1.
cu_mounts_within() {
  cu_mount_table | awk -v p="$1" '$5 == p || index($5, p "/") == 1 { n++ } END { print n + 0 }'
}

cu_count_lines() {
  if [ -n "$1" ]; then printf '%s\n' "$1" | wc -l | tr -d ' '; else echo 0; fi
}

custody_owned_index() { # path [kind] -> index of the owned record, or failure
  local i=0
  while [ "$i" -lt "${#custody_owned_path[@]}" ]; do
    if [ "${custody_owned_path[$i]}" = "$1" ] && { [ -z "${2:-}" ] || [ "${custody_owned_kind[$i]}" = "$2" ]; }; then
      echo "$i"
      return 0
    fi
    i=$((i + 1))
  done
  return 1
}

# Every directory from $1 up to $custody_stop must be a real (non-link)
# directory owned by $custody_uid and not group/other writable, mirroring the
# ancestor walk of production_registry.
cu_safe_chain() {
  local path=$1
  while :; do
    cu_read_meta "$path" || {
      cu_fail "cannot inspect ancestor $path"
      return 1
    }
    case "$m_mode" in '' | *[!0-7]*)
      cu_fail "unreadable mode of $path"
      return 1
      ;;
    esac
    if [ "$m_kind" != dir ] || [ "$m_uid" != "$custody_uid" ] || [ $((8#$m_mode & 8#022)) != 0 ]; then
      cu_fail "unsafe ancestor $path (kind=$m_kind uid=$m_uid mode=$m_mode)"
      return 1
    fi
    [ "$path" != "$custody_stop" ] || return 0
    if [ "$path" = / ]; then
      cu_fail "ancestor walk never reached $custody_stop"
      return 1
    fi
    path="${path%/*}"
    [ -n "$path" ] || path=/
  done
}

# Observed runner/image/namespace facts, appended to provenance.txt (sealed
# with the other evidence). Recorded even when the context is then refused.
custody_facts() {
  local ns comm
  comm="$(cat /proc/1/comm 2>/dev/null)" || comm=unavailable
  {
    echo "custody_runner=${RUNNER_ENVIRONMENT:-}/${RUNNER_OS:-}/${RUNNER_ARCH:-} image=${ImageOS:-}/${ImageVersion:-}"
    echo "custody_pid1_comm=$comm"
    for ns in pid user mnt; do
      echo "custody_ns_$ns self=$(cu_ns_id "$ns" self 2>/dev/null || echo unavailable) pid1=$(cu_ns_id "$ns" 1 2>/dev/null || echo unavailable)"
    done
    echo "custody_root_mount=$(cu_mounts_at / 2>/dev/null | tr '\n' ';')"
  } >>"$out/provenance.txt"
}

# Contradictory or unsupported context refuses; nothing is guessed.
custody_host_context() {
  local marker who
  [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ] || {
    cu_fail "RUNNER_ENVIRONMENT is not github-hosted"
    return 1
  }
  [ "${RUNNER_OS:-}" = Linux ] && [ "${RUNNER_ARCH:-}" = X64 ] && [ "${ImageOS:-}" = ubuntu24 ] || {
    cu_fail "runner context is not the Linux X64 ubuntu24 image"
    return 1
  }
  grep -qx 'ID=ubuntu' "$custody_os_release" 2>/dev/null &&
    grep -qx 'VERSION_ID="24.04"' "$custody_os_release" 2>/dev/null || {
    cu_fail "operating system is not Ubuntu 24.04"
    return 1
  }
  for marker in $custody_container_markers; do
    if [ -e "$marker" ] || [ -L "$marker" ]; then
      cu_fail "container marker $marker present"
      return 1
    fi
  done
  for who in self 1; do
    [ "$(cu_ns_id pid "$who" | sed 's/.*://')" = "$custody_init_pid_ino" ] || {
      cu_fail "PID namespace of $who is not the initial PID namespace"
      return 1
    }
    [ "$(cu_ns_id user "$who" | sed 's/.*://')" = "$custody_init_user_ino" ] || {
      cu_fail "user namespace of $who is not the initial user namespace"
      return 1
    }
  done
  # In the initial PID namespace PID 1 is the real init, so agreement with it
  # means the host's initial mount namespace.
  [ "$(cu_ns_id mnt self)" = "$(cu_ns_id mnt 1)" ] || {
    cu_fail "mount namespace differs from PID 1"
    return 1
  }
}

# Read-only: refuses without creating or changing anything.
custody_preflight() {
  local lines
  custody_host_context || return 1
  cu_safe_chain "$custody_base" || return 1
  if [ -e "$custody_parent" ] || [ -L "$custody_parent" ]; then
    cu_fail "$custody_parent already exists; refusing to adopt it"
    return 1
  fi
  lines="$(cu_mounts_within "$custody_parent")" || {
    cu_fail "mount table unreadable"
    return 1
  }
  [ "$lines" = 0 ] || {
    cu_fail "mounts already exist at or below $custody_parent"
    return 1
  }
  cu_log "preflight ok: initial namespaces, no container marker, $custody_parent absent"
}

# ---- creation (only what this run creates is ever recorded as owned) ----

custody_record() { # kind path
  local id
  if [ "$1" = mount ]; then
    id="$(cu_anchor_id "$2")" || id=unknown
  elif cu_read_meta "$2"; then
    id="$m_kind|$m_id"
  else
    id=unknown
  fi
  custody_owned_kind+=("$1")
  custody_owned_path+=("$2")
  custody_owned_id+=("$id")
  cu_log "owned $1 $2 id=$id"
}

# Exclusive creation: mkdir fails on any existing object; the file create uses
# noclobber inside the directory this run just made (private, root-only).
custody_make() { # dir|file path mode
  local error
  if [ "$1" = dir ]; then
    error="$(
      umask 077
      mkdir -m "$3" -- "$2" 2>&1
    )" || {
      cu_fail "cannot create $2: $error"
      return 1
    }
  else
    error="$(
      umask 077
      set -C
      { : >"$2"; } 2>&1
    )" || {
      cu_fail "cannot create $2: $error"
      return 1
    }
  fi
  custody_record "$1" "$2"
}

custody_pin() { # namespace-name anchor-path
  if ! cu_mount_bind "/proc/1/ns/$1" "$2"; then
    # A failed bind should leave nothing. If a mount still appeared on this
    # private, freshly created path, it is ours to verify and unwind.
    if [ -n "$(cu_mounts_at "$2" 2>/dev/null)" ]; then custody_record mount "$2"; fi
    cu_fail "bind mount of the $1 namespace failed"
    return 1
  fi
  custody_record mount "$2"
}

# ---- verification of the finished setup, before the unchanged admission ----

custody_verify() {
  local ns path lines id recorded index expected
  cu_safe_chain "$custody_root" || return 1
  cu_read_meta "$custody_root" && [ "$m_mode" = 700 ] || {
    cu_fail "$custody_root is not mode 0700"
    return 1
  }
  cu_read_meta "$custody_root/lock" &&
    [ "$m_kind" = file ] && [ "$m_uid" = "$custody_uid" ] && [ "$m_mode" = 600 ] && [ "$m_nlink" = 1 ] || {
    cu_fail "lock is not a root-owned 0600 single-link regular file"
    return 1
  }
  expected="$(printf 'host-mnt\nhost-pid\nlock')"
  [ "$(cu_list_dir "$custody_root")" = "$expected" ] || {
    cu_fail "unexpected entries in $custody_root"
    return 1
  }
  for ns in pid mnt; do
    path="$custody_root/host-$ns"
    lines="$(cu_mounts_at "$path")" || {
      cu_fail "mount table unreadable"
      return 1
    }
    [ "$(cu_count_lines "$lines")" = 1 ] && [ "${lines%% *}" = nsfs ] || {
      cu_fail "$path is not exactly one nsfs mount"
      return 1
    }
    [ "$(cu_mounts_within "$path")" = 1 ] || {
      cu_fail "mounts are stacked or nested at $path"
      return 1
    }
    id="$(cu_anchor_id "$path")" || {
      cu_fail "cannot identify $path"
      return 1
    }
    [ "$id" = "$(cu_ns_id "$ns" self)" ] && [ "$id" = "$(cu_ns_id "$ns" 1)" ] || {
      cu_fail "$path does not match the $ns namespace of this process and PID 1"
      return 1
    }
    index="$(custody_owned_index "$path" mount)" || {
      cu_fail "$path is not an owned mount"
      return 1
    }
    recorded="${custody_owned_id[$index]}"
    [ "$id" = "$recorded" ] || {
      cu_fail "$path changed since it was created"
      return 1
    }
    cu_log "verified $path nsfs id=$id equals self and PID 1"
  done
  [ "$(cu_mounts_within "$custody_root")" = 2 ] || {
    cu_fail "unexpected mounts below $custody_root"
    return 1
  }
}

custody_setup_steps() {
  custody_facts
  custody_preflight || return 1
  custody_make dir "$custody_parent" 0755 || return 1
  custody_make dir "$custody_root" 0700 || return 1
  custody_make file "$custody_root/lock" 0600 || return 1
  custody_make file "$custody_root/host-pid" 0600 || return 1
  custody_pin pid "$custody_root/host-pid" || return 1
  custody_make file "$custody_root/host-mnt" 0600 || return 1
  custody_pin mnt "$custody_root/host-mnt" || return 1
  custody_verify || return 1
}

# Sets custody_setup_state to 0, "refused" (nothing was created) or "failed"
# (partial setup exists and is unwound by custody_cleanup).
custody_setup() {
  custody_setup_state=unknown
  if custody_setup_steps; then
    custody_setup_state=0
    cu_log "setup verified"
    return 0
  fi
  if [ "${#custody_owned_path[@]}" = 0 ]; then
    custody_setup_state=refused
  else
    custody_setup_state=failed
  fi
  [ -n "$custody_setup_failure" ] || custody_setup_failure="setup stopped without a recorded reason"
  cu_log "setup $custody_setup_state"
  return 1
}

# ---- owned cleanup ----

custody_unpin() { # path recorded-id
  local lines
  lines="$(cu_mounts_at "$1")" || {
    cu_log "UNVERIFIED_MOUNT $1 (mount table unreadable)"
    return 1
  }
  if [ "$(cu_count_lines "$lines")" != 1 ] || [ "${lines%% *}" != nsfs ] ||
    [ "$(cu_mounts_within "$1")" != 1 ] || [ "$(cu_anchor_id "$1" 2>/dev/null)" != "$2" ]; then
    cu_log "UNVERIFIED_MOUNT $1 (not the single nsfs mount created here; left in place)"
    return 1
  fi
  cu_umount "$1" || {
    cu_log "UNMOUNT_FAILED $1"
    return 1
  }
  if [ -n "$(cu_mounts_at "$1")" ]; then
    cu_log "STILL_MOUNTED $1"
    return 1
  fi
  cu_log "unmounted $1"
}

custody_still_ours() { # path recorded-id (kind|dev:inode); never follows links
  cu_read_meta "$1" || {
    cu_log "MISSING $1"
    return 1
  }
  if [ "$m_kind|$m_id" != "$2" ]; then
    cu_log "REPLACED $1 (not the object created here; left in place)"
    return 1
  fi
  if [ -n "$(cu_mounts_at "$1")" ]; then
    cu_log "STILL_MOUNTED $1"
    return 1
  fi
}

custody_unlink() { # path recorded-id
  custody_still_ours "$1" "$2" || return 1
  [ "$m_nlink" = 1 ] || {
    cu_log "LINKED $1 (more than one link; left in place)"
    return 1
  }
  rm -- "$1" || {
    cu_log "RETAINED $1"
    return 1
  }
  cu_log "removed $1"
}

custody_rmdir() { # path recorded-id
  custody_still_ours "$1" "$2" || return 1
  rmdir -- "$1" || {
    cu_log "RETAINED $1 (not empty or not removable)"
    return 1
  }
  cu_log "removed $1"
}

# Returns 0 only when everything this run created is verified and removed.
# Any record, entry or mount that is not exactly an owned object keeps the
# whole setup in place: custody uncertainty is preserved, never cleaned up.
custody_cleanup() {
  local failed=0 i entry
  [ "${#custody_owned_path[@]}" -gt 0 ] || return 0
  if custody_owned_index "$custody_root" >/dev/null && [ -d "$custody_root" ] && [ ! -L "$custody_root" ]; then
    while IFS= read -r entry; do
      [ -n "$entry" ] || continue
      if ! custody_owned_index "$custody_root/$entry" >/dev/null; then
        cu_log "RETAINED_ENTRY $entry in $custody_root (custody record or unknown object; every owned resource left in place)"
        return 1
      fi
    done <<<"$(cu_list_dir "$custody_root")"
  fi
  # Never act through a swapped directory: every owned directory must still be
  # the object created here before anything beneath it is touched.
  i=0
  while [ "$i" -lt "${#custody_owned_path[@]}" ]; do
    if [ "${custody_owned_kind[$i]}" = dir ] &&
      ! custody_still_ours "${custody_owned_path[$i]}" "${custody_owned_id[$i]}"; then
      cu_log "owned directory ${custody_owned_path[$i]} is not intact; every owned resource left in place"
      return 1
    fi
    i=$((i + 1))
  done
  i=${#custody_owned_path[@]}
  while [ "$i" -gt 0 ]; do
    i=$((i - 1))
    case "${custody_owned_kind[$i]}" in
      mount) custody_unpin "${custody_owned_path[$i]}" "${custody_owned_id[$i]}" || failed=1 ;;
      file) custody_unlink "${custody_owned_path[$i]}" "${custody_owned_id[$i]}" || failed=1 ;;
      dir) custody_rmdir "${custody_owned_path[$i]}" "${custody_owned_id[$i]}" || failed=1 ;;
      *)
        cu_log "UNKNOWN_KIND ${custody_owned_kind[$i]} ${custody_owned_path[$i]}"
        failed=1
        ;;
    esac
  done
  [ "$failed" = 0 ] || return 1
  cu_log "cleanup verified: every owned resource removed"
}
