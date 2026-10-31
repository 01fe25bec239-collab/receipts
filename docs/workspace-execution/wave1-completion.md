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
registered worktree. Provisioning records one versioned file
(`receipts-workspace-binding`, mode 0400, created with `create_new` so a marker
is never overwritten) in the worktree's own Git administrative directory
(`<common-dir>/worktrees/<id>/`): canonical repository and worktree paths, task
branch, ORIGINAL base SHA, workspace ID, task ID and a SHA-256 over them. The
directory is found from the repository side (`worktrees/*/gitdir`), never by
following the payload-writable `.git` pointer. Attach requires the record to be
present, canonical (the digest is proven by re-encoding), a regular non-symlink
file owned by the current effective user or root and not group/other writable,
and every field equal to the request; the branch must still be checked out only
there and the base still an ancestor. A different (even ancestral) base, task
or workspace, a missing/corrupt/unauthenticated record, or a worktree created
without task context refuses with `WorktreeCreateConflict` (field names only);
nothing is adopted, repaired or recreated, current HEAD and dirty data are
untouched and no operational handle is minted. A failed record write rolls the
attempt back (or reports `ProvisionRollbackIncomplete`, retaining the branch
and registration); a crash in between leaves no valid record, so attach and
re-provisioning both refuse.

No task context: ordinary `worktree add -b` semantics without task authority and
without a record; a task-bound worktree is refused, never attached as plain, and
for plain worktrees the original base is verified by ancestry only. No fetch.
Canonical absolute path returned. Host hook plumbing is Host-owned.
Limits: authentication is by custody (owner/mode), not a secret, so anything that
already acts as the owner or root is outside it; after protected custody
transfer the record is controller-owned and attachable only by that identity or
root.

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
  (`--no-verify`, fixed identity), checkpoint. Stage restores the index and
  refuses when Git reports an embedded repository: a tracked gitlink would let
  worker-controlled config (for example a clean filter) run as the controller on
  a later `status`/`diff` (reproduced locally with the hardened Git options);
  the distinct worker UID additionally stops Git from entering such a
  repository. Repositories whose base already tracks initialized submodules are
  not supported under protected launch (their worker-owned worktrees fail
  Git's ownership check). `Checkpoint` returns the `CHECKPOINTED` handle with
  the captured evidence.
- Diagnostics: `ControllerRequest`/`ControllerChannel` (and everything that
  nests them) print `<redacted>` for bearer tokens in ordinary, pretty and
  nested `Debug`; errors never echo request tokens. Explicit protocol access
  (`credentials()`, the public `token` field) is unchanged.
- Custody release: `ProtectedAttempt::attempt()` is a view whose `wait_collect`
  records that deadline/reap/EOF and the cgroup kill/emptiness proof succeeded.
  `release_custody()` (once, Linux, root) returns metadata and payload to the
  pre-admission owner only after that proof and with no process left under the
  worker identity; afterwards both channels refuse. This is what lets ordinary
  non-force teardown run as the repository owner. Modes are not widened again.

Differences from PR185 (a CANDIDATE, not accepted security equivalence; a host
that cannot show every property refuses): the trusted child bootstrap writes
`cgroup.procs` as its first action before any worker-controlled code or fork
(not `clone3(CLONE_INTO_CGROUP)`); no chroot (DAC, the private read-only
metadata bind mount and filtered syscalls instead); the worker is not
Landlocked; all inherited FDs are made close-on-exec; both identities end with
explicit exact capability sets (worker none), cleared ambient set and reset
keep-caps.

## Hosted diagnostic (prepared, NOT RUN)

`hosted_protected_worker_controller_and_cleanup` asserts, inside the real
boundary: exact custody owners/modes/ancestors after admission; worker and
controller real/effective/saved/fs IDs, groups, all five capability sets,
no-new-privs and seccomp mode; only stdio plus the probe's own directory FD open;
`unshare`/`setns`/`open_by_handle_at`/`pidfd_getfd`/`process_vm_readv` return
`EPERM` where the unfiltered result would differ; root, sibling and own cgroup
`cgroup.procs` cannot be opened for writing; metadata write, replace, rename and
`/proc/self/root` routes are denied while payload edit and worker `0600` creation
work; the controller reads that `0600` file, is denied the outside sentinel
directly and through `/proc/self/root`; reviewer mutation requests refuse on the
enforced channel; a worker-planted embedded repository is never staged and its
filter marker never appears; commit, checkpoint handle, custody release, dirty
non-force teardown refusal and clean teardown with the task commit retained.
`run.sh` records `test_status` and `cleanup_status` separately; PASS/exit 0
requires both 0 (cgroup kill, emptiness, removal, no fixture-identity process,
no fixture directory).
Cleanup enumeration accepts regular `cgroup.*`/controller interface files of the
delegated cgroup as kernel control files; any link (never followed), unexpected
name, non-`receipts-*` directory or non-directory child is reported unowned. `run.sh self-check` is the unprivileged control for that
verdict logic only.

## Proof status

Local Darwin tests cover all platform-neutral behavior and the refusal on
non-Linux. Linux enforcement code and the hosted test were type-checked and
Clippy-checked locally only in a throwaway copy with the Linux cfg gates
retargeted to Darwin (no link, nothing committed from that copy); they have NOT
been executed. The hosted
diagnostic (`.github/workflows/we-w1-completion-linux.yml`) is prepared and
NOT RUN; it needs A1 inspection and independent static review before
USER-authorized execution. Root/controller compromise, kernel bugs and
non-filesystem Landlock scope are outside admission assumptions.
