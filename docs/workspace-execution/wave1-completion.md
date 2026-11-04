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
  for each actual Git target before provisioning status, teardown status,
  WorktreeCreate attach and protected admission/controller operations. All includes are refused, including ones inside custody.
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
- One host custody domain: `/var/lib/receipts/protected-custody`, root-owned
  0700 beneath root-owned non-writable ancestors. Trusted host administration
  must preinstall its mode-0600 single-link `lock` and nsfs bind mounts
  `host-pid` and `host-mnt` pinned from the actual host initial namespaces.
  A container's own PID 1 is insufficient provenance. The launcher does not
  provision or infer these anchors: absent, unsafe or mismatched setup refuses.
  Root/administrator replacement of this admission setup is outside the boundary.
  Every privileged parent must share these anchors and filesystem view.
- Before custody mutation or worker identity allocation, a brief shared flock
  critical section reserves both actual common directory and worktree. All four
  resource pairings compare their complete directory object ancestor chains.
  Retained overlapping reservations refuse; unrelated repositories execute
  concurrently after the registry lock is released. Unknown records refuse.
  Linux topology checks admit only ext4/xfs/btrfs/tmpfs and refuse nested mounts, bind-root aliases
  and devices with multiple mount views. Namespace mismatches refuse.
- The shared registry persists paths, object chains, phase, attempt and original
  ownership before transfer; the same-common `receipts-custody.state` marker
  remains an additional persistent guard. Crash/partial state stays reserved.
  Release removes only owned state after gate drain, proven cleanup and successful
  restoration. There is no orphan recovery or force unlock.
- Stable objects (`protected_custody`): both trees are walked and mutated only
  through directory descriptors (`openat(O_NOFOLLOW)`, `fchown`/`fchmod` on the
  object just opened, root identities recorded at admission and re-checked at
  release). Everything that can refuse runs first and read-only: overlapping
  roots, ancestors, mount points strictly inside either root
  (`/proc/self/mountinfo`, so bind mounts count), the `.git` pointer (plain
  single-link regular file), and the link policy of both trees. Refused as
  unsupported, with the external object unchanged: regular files with more than
  one hard link (a possible external alias; Git content is never copied or
  detached), special files, entries on another device, symlinks anywhere in Git
  metadata, nesting deeper than 256 directories. Payload symlinks stay supported
  because they are re-owned in place (`fchownat(AT_SYMLINK_NOFOLLOW)` by name
  inside the verified parent) and never followed or traversed. The same checks
  repeat on each object about to change, so a link created after preflight
  refuses at that object (earlier objects of a transfer already begun stay
  attributed to the retained record). Release applies the same preflight and
  verified-object operations after validating the persisted original owner.
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
- Release barrier: both channels and the release share one lifecycle gate.
  Every controller operation holds it for its whole duration (including its
  reaped Git children); `release_custody` atomically closes admission, waits for
  all admitted operations, and only then restores ownership. Requests that
  arrive during (`custody release in progress`) or after (`custody was
  released`) the release refuse before executing; replay consumption and typed
  errors are unchanged. No lock is held across Git work or restoration. A
  confined controller run whose containment was not proven, or a failed
  restoration, quarantines custody: the record becomes `incomplete`, both
  channels refuse (`custody release incomplete`), later release attempts return
  `CleanupUnknown` and the repository stays reserved.
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
  the captured evidence. Nested checkpoint records (`executed_checks[]`,
  `dirty_diff_ref`, `output_ref`) decode only from JSON objects.
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
no-new-privs and seccomp mode, observed by the probes through system calls
(`getresuid`/`getresgid`/`setfsuid(-1)`/`getgroups`/`capget`/`prctl`) because the controller's Landlock plan grants no `/proc` access and
production access is not widened; only stdio open, observed by the privileged parent through the complete
`/proc/<pid>/fd` directory while every child task is group-stopped. The parent
binds the sole process in its exact attempt cgroup with a pidfd, checks lifetime
and stopped state around observation, then resumes that same process. Missing
observations fail. Controls retain FD 65536 and FD 128 above lowered soft/hard
limits. Capability queries use the admitted kernel capability range; unexpected
query errors preserve errno and cannot terminate a scan as an empty set;
`unshare`/`setns`/`open_by_handle_at`/`pidfd_getfd`/`process_vm_readv` return
`EPERM` where the unfiltered result would differ; root, sibling and own cgroup
`cgroup.procs` cannot be opened for writing; metadata write, replace, rename and
`/proc/self/root` routes are denied while payload edit and worker `0600` creation
work; the controller reads that `0600` file, is denied the outside sentinel
directly and through `/proc/self/root`; reviewer mutation requests refuse on the
enforced channel; a worker-planted embedded repository is never staged and its
filter marker never appears. The worker prepares the embedded repository BEFORE
the filter exists, arms it, proves with a worker-side `git add` that the filter
really runs, then resets the marker; the parent asserts the marker absent at that
baseline and after every controller operation, so a marker can only come from
controller execution. Commit, checkpoint handle, custody release (record deleted
only then), dirty non-force teardown refusal and clean teardown with the task
commit retained. A second hosted test
(`hosted_exclusive_custody_and_linked_target_refusals`) covers, with real
chown: duplicate, linked-worktree, alias and second-launcher admission refusals
that change nothing, safe re-admission after a complete release, and the
external-symlink `.git` pointer and payload hardlink refusals with external
owner/mode unchanged.
`run.sh` records `test_status` and `cleanup_status` separately; PASS/exit 0
requires both 0 (cgroup kill, emptiness, removal, no fixture-identity process,
no fixture directory).
Cleanup enumeration accepts regular `cgroup.*`/controller interface files of the
delegated cgroup as kernel control files; any link (never followed), unexpected
name, non-`receipts-*` directory or non-directory child is reported unowned. `run.sh self-check` is the unprivileged control for that
verdict logic only.

## Proof status

Local Darwin tests cover all platform-neutral behavior and the refusal on
non-Linux, including the reservation (threads and a separate process), the
release barrier (barrier- and condition-coordinated, no sleeps), the descriptor
engine against real symlink/hardlink/replacement fixtures with external
sentinels, and the filter-marker phases with an unprivileged positive control.
Ownership changes to other users, `/proc/self/mountinfo` on a real bind mount,
Landlock and cgroups need the hosted run. Linux enforcement code and the hosted tests were type-checked and
Clippy-checked locally only in a throwaway copy with the Linux cfg gates
retargeted to Darwin (no link, nothing committed from that copy); they have NOT
been executed. The hosted
diagnostic (`.github/workflows/we-w1-completion-linux.yml`) is prepared and
NOT RUN; it needs A1 inspection and independent static review before
USER-authorized execution. Root/controller compromise, kernel bugs and
non-filesystem Landlock scope are outside admission assumptions.

Lifecycle start returns a WorkspaceAttempt facade retaining its own ACTIVE handle.
Only its actual collection can construct WorkspaceCollectionFailure. Orphaning
consumes that opaque carrier and uses its retained handle; caller-built Execution
errors and caller-selected handles supply no provenance. Repeated collection and
capture-only failures cannot supply orphan authority. Execution and Runtime APIs
remain unchanged.

Hosted conflict checks distinguish early Git dubious-ownership refusal from
reservation enforcement. They separately call the production reservation path
using fixture roots recorded before transfer, and require the held attempt in its
refusal. Neither control disables Git ownership safety. Native execution remains
NOT RUN.
