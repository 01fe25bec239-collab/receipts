# Workspace Wave1 completion: behavior and limits

Scope: WE-W1-1 … WE-W1-5. A Git worktree alone is never a sandbox; protection
exists only through an admitted protected launch.

## Git hardening (all Workspace Git calls)

- One resolved absolute Git, empty inherited environment plus
  `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_TERMINAL_PROMPT=0`, `GIT_NO_REPLACE_OBJECTS=1`.
- Command-line overrides: `core.hooksPath=/dev/null`, `core.fsmonitor=false`,
  `core.untrackedCache=false`, `commit.gpgSign=false`, `tag.gpgSign=false`,
  `protocol.allow=never`, `core.splitIndex=false`.
- Repositories declaring `filter.*`, `include.*` or `includeIf.*` are refused
  before provisioning, teardown status, WorktreeCreate attach and protected
  admission. All includes are refused, including ones inside custody.
- Every call runs through the existing bounded runner: dedicated process
  group, 120 s deadline, SIGTERM → 2 s grace → SIGKILL, verified reap, 1 MiB
  per-stream bounded capture. Timeout and oversize output are typed errors and
  truncated output is never parsed.

## Provisioning, rollback and teardown

- Preflight: Git >= 2.36, refused configuration, base commit exists, branch
  absent, target not registered, target absent or empty. IDs: 1–200 Unicode
  characters for new provisioning (observed handles are not rewritten).
- Handles store the canonical worktree path (`/var` → `/private/var`).
- Rollback removes only what the failed attempt created: its clean registered
  worktree (non-force) and its new branch via conditional
  `update-ref -d <ref> <base>`. Preexisting targets, worker data, moved refs
  or unknown state are retained and reported as
  `ProvisionRollbackIncomplete`.
- Non-force teardown from `PROVISIONED`, `CHECKPOINTED` or `RECOVERED` only.

## Lifecycle and checkpoints

- Six states with one checked oracle: start observed → `ACTIVE`; successful
  capture → `CHECKPOINTED`; new admitted work → `ACTIVE`; unknown attempt
  cleanup → `ORPHANED`; `ORPHANED` → `RECOVERED` has no public producer
  (recovery execution is Wave3); `TORN_DOWN` is terminal.
- `WorkspaceCheckpointWire` decodes/encodes the schema exactly: optional
  absence vs present-empty arrays, strings, and `exit_code` as the exact JSON
  integer token (any magnitude; `1e2`, `-0`). `date-time` is an annotation on
  the wire; conversion to the operational value validates timestamps, `i64`
  exit codes and array presence and reports unsupported domains explicitly.
  Re-encoding exported data is byte-identical.

## R9

- Write-scope evidence is the commit-to-commit tree diff: changed tracked files
  count even if ignore rules match them; ignored untracked content is outside
  the evidence and never reported as inspected.
- Case aliases: on a filesystem probed case-insensitive or unknown, a changed
  path whose ASCII case-fold equals another changed/tracked path is a
  collision; non-ASCII changed paths are reported as unestablished. Both fail
  verification. The A0 matcher stays case-sensitive; paths are not normalized.
  Limit: one probe at the worktree root (per-directory casefold not detected).

## WorktreeCreate

Task context: hardened provisioning, or idempotent attach only to the exact
registered worktree for the same branch whose base is still an ancestor of
HEAD; reports actual HEAD/cleanliness, never resets, never fabricates a handle.
No task context: ordinary `worktree add -b` semantics without task authority.
No fetch. Canonical absolute path returned. Host hook plumbing is Host-owned.

## Protected launch (exported for Runtime)

`ProtectedLauncher` → `admit(handle, repo)` → `launch(admission, request,
policy)` → `ProtectedAttempt { handle (ACTIVE), attempt, worker_channel,
reviewer_channel }`. Admission requires Linux x86_64, effective UID 0, a
delegated cgroup v2 directory with `cgroup.kill`, Landlock ABI >= 3, seccomp
and `protected_hardlinks`/`protected_symlinks`. All other platforms refuse.

- Custody: metadata → controller, no group/other write; ancestors root- or
  controller-owned and not worker-writable (sticky allowed); worktree root
  controller:worker `01770`; `.git` pointer controller-owned; payload → fresh
  worker UID (monotonic, never reused by a launcher).
- Worker: own cgroup, private mount namespace with metadata bind-mounted
  read-only, no groups/capabilities, no-new-privs, close-on-exec FDs, seccomp
  (EPERM: `open_by_handle_at`, `setns`, `unshare`, mount family, `ptrace`,
  `process_vm_*`, `pidfd_getfd`, namespace `clone`; ENOSYS: `clone3`; x32 and
  foreign architectures killed).
- Controller: fixed Git argv only, controller UID with only
  `CAP_DAC_READ_SEARCH` (reads worker `0600` payload without chmod/chown),
  Landlock (repository + worktree full, tool trees read/exec, named `/etc`
  files, `/dev/null`), same seccomp, own cgroup per call.
- Cleanup: existing deadline/grace/reap lifecycle, then `cgroup.kill` and
  `populated 0` proof before reader verification. Unknown cleanup refuses
  success and retains the cgroup and identity.
- Channel: requests carry attempt, task, per-channel secret token and the next
  sequence number; forged, cross-task, replayed and reviewer write requests
  refuse. Operations: identity, status, stage (literal pathspecs), commit
  (`--no-verify`, fixed identity), checkpoint.

Differences from PR185: cgroup placement is done by the child writing
`cgroup.procs` before exec (not `clone3(CLONE_INTO_CGROUP)`); no chroot; the
worker is not Landlocked; all inherited FDs are made close-on-exec.

## Proof status

Local Darwin tests cover all platform-neutral behavior and the refusal on
non-Linux. Linux enforcement code was type-checked/Clippy-checked locally only
by temporarily retargeting its cfg gates; it has NOT been executed. The hosted
diagnostic (`.github/workflows/we-w1-completion-linux.yml`) is prepared and
NOT RUN; it needs A1 inspection and independent static review before
USER-authorized execution. Root/controller compromise, kernel bugs and
non-filesystem Landlock scope are outside admission assumptions.
