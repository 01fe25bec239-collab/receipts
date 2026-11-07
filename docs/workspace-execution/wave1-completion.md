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
  must preinstall its mode-0600 single-link `lock`, an nsfs bind mount
  `host-pid` of the actual host initial PID namespace, and `host-mnt.identity`:
  a root 0600 single-link record (`receipts-host-mnt-identity-v1`, then
  `boot_id=`, `mnt_namespace=<dev>:<ino>`, `end`, nothing else) of the boot and
  the host initial mount namespace. The kernel refuses an nsfs bind of a mount
  namespace inside that same namespace, so the mount namespace is recorded, not
  pinned. The record is an administrative assertion, not a kernel handle or an
  attestation. Admission reads it descriptor-relative without following links,
  bounded and fully parsed, and requires it to name the current boot and the
  mount namespace of both the admitting process and PID 1. A missing,
  malformed, extra, truncated, other-boot, wrong-owner/mode/link or mismatched
  record refuses, as does the obsolete `host-mnt` entry or mount.
  A container's own PID 1 is insufficient provenance. The launcher does not
  provision or infer these anchors: absent, unsafe or mismatched setup refuses.
  (The hosted diagnostic below supplies them from its own disposable VM, as
  administration separate from admission.)
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
  An owner creates the marker, then locks it, then records state; a losing
  contender's diagnosis never lock-probes an empty marker, so it cannot take
  the owner's lock inside that window and leave zero admissions.
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
  files, `/dev/null`), same seccomp, own cgroup per call. Per-call cgroup names
  `receipts-<attempt>-ctl-<n>` come from one counter shared by both channels
  and every request of the admission: overlapping worker/reviewer Git calls
  never collide. An index is consumed even when creation fails and is never
  reused; an existing directory refuses (never adopted or cleaned); exhaustion
  refuses instead of wrapping.
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
`run.sh` records `setup_status`, `test_status`, `cleanup_status`,
`custody_cleanup_status`, `tests_completed`/`tests_total`, the FIRST failed test
(`first_failed_test`, `first_failed_status`), any `interruption` and the
highest-precedence failure as `primary_failure` (setup, then an abnormal end,
then the first failed test). PASS/exit 0 requires verified host setup, ALL
three tests completed with status 0 (`test_status` is the first failing status,
0 only after every test ran, `unknown` before), cgroup kill, emptiness,
removal, no fixture-identity process, no fixture directory, verified removal of
the owned host setup, no interruption, and the final result actually
published. Nothing starts as success: `result.txt` is written `INCOMPLETE`
first (if that fails the run exits 1 before creating anything) and replaced
by one atomic rename of the final PASS/FAIL. A failed write or rename of the
final result exits 1 and leaves the previous `INCOMPLETE`, even when every
test and cleanup passed. SIGTERM/SIGINT/SIGHUP stay live until the command right
after the final rename: one received before that point (even
just after the rename) is recorded (`interruption=`, FAIL republished), the
owned cleanup still runs, and the process exits 128+n; after it they are
ignored, so the exit status matches the published verdict. SIGKILL or machine
loss is NOT handled and does not guarantee cleanup (the VM is disposable):
before the final rename it leaves `INCOMPLETE`; after it the published verdict,
even PASS, stays with a killed (nonzero) process status.
Cleanup enumeration accepts regular `cgroup.*`/controller interface files of the
delegated cgroup as kernel control files; any link (never followed), unexpected
name, non-`receipts-*` directory or non-directory child is reported unowned. `run.sh self-check` is the unprivileged control for that
verdict logic only.

### Hosted host setup (administration, not admission)

Trusted premise, external and only recorded here: the job is the existing
same-repository, exact-head, branch-gated standard GitHub-hosted `ubuntu-24.04`
job running directly on its fresh VM, with no job container, service container
or self-hosted substitution (GitHub documents a fresh VM per job). A container's
PID 1 or an environment flag alone does not establish host origin.

`custody.sh` (sourced by `run.sh hosted`, before the unchanged admission checks)
provides what `production_registry` requires: `/var/lib/receipts` (root `0755`)
and `/var/lib/receipts/protected-custody` (root `0700`) under root-owned,
non-link, non-group/other-writable ancestors; a root `0600` single-link `lock`;
an nsfs bind mount `host-pid` of PID 1's initial PID namespace; and the
root `0600` single-link `host-mnt.identity` record of this boot's
`/proc/sys/kernel/random/boot_id` and PID 1's mount namespace dev:inode,
computed from kernel observations during the read-only preflight and written
by the same exclusive create. No alternate registry, fake pin files, auto-provisioning inside the product or
bypass of its topology/namespace checks exists.

- Context refusal (nothing created): `RUNNER_ENVIRONMENT=github-hosted`,
  `RUNNER_OS=Linux`, `RUNNER_ARCH=X64`, `ImageOS=ubuntu24` (read-only values
  passed through the sanitized `env -i`) and an Ubuntu 24.04 `os-release`; no
  `/.dockerenv` or `/run/.containerenv`; the PID namespace of this process and
  of PID 1 is the kernel's fixed initial PID namespace (inode 4026531836) and
  their user namespace the initial user namespace (4026531837); the mount
  namespace equals PID 1's; the boot id and mount namespace identity are
  well-formed. Contradiction refuses, never guesses.
- Existing/unknown resources refuse untouched: an existing or linked
  `/var/lib/receipts`, any mount at or below it, an unsafe, linked or
  non-root-owned ancestor. Nothing is adopted, overwritten, chmod/chowned or
  unmounted. Objects are created exclusively (`mkdir`, noclobber inside the
  freshly made private directory).
- Verification before any test: owners/modes/link counts, exactly the three
  entries, the record is the object created here and byte-for-byte the current
  boot and the mount namespace of this process and PID 1, exactly one nsfs mount
  (`host-pid`) with nothing stacked or nested, and its dev:inode equal to the PID
  namespace of this process and of PID 1.
  Observed runner/image/namespace/root-mount facts go to `provenance.txt`;
  setup and cleanup events to `diagnostic.log` (`CUSTODY ...`).
- Ownership: intent is recorded BEFORE each side effect (directory, file,
  mount, and the cgroup `mkdir`) as pending, because a command's return status
  does not prove what exists. A create that returns 0 becomes owned with the
  identity observed. One that fails or is interrupted and left NOTHING is
  dropped; one that left something is `uncertain`: it is not provably ours, so it
  is never adopted, removed or unmounted, and the run reports
  `custody_cleanup_status=uncertain` (or an `UNCERTAIN` cgroup) and fails.
- Cleanup has two phases. After the tests and PROVEN worker/controller/cgroup
  cleanup, phase 1 changes nothing: it reconciles pending creations and verifies
  the whole owned setup: directory/file identities and link counts, each mount's
  type and identity, a SUCCESSFUL complete listing of every owned directory (a
  failed listing is a failure, never "empty"), no custody record or unknown
  entry, and no mount below the registry that this run did not create. Only
  then does phase 2 unmount (never `-l`/`-f`/recursive) and remove (`rm`,
  `rmdir`, never recursive) in reverse order, re-checking each object. Any
  mismatch, unreadable state, replaced or aliased lock, unknown mount or record
  keeps EVERY owned resource (anchors and lock) in place with zero unmounts and
  fails; records are never read, deleted or recovered. A placeholder file under
  an owned mount can only be checked after that mount is removed. If
  worker/cgroup cleanup is not proven, custody is retained
  (`custody_cleanup_status=skipped`).
- Failure handling: an aborted setup or run still reaches the EXIT trap and
  keeps its nonzero status. Setup that is refused or fails leaves tests
  `NOT RUN` (`test_status=unknown`, `setup_status=refused|failed`), keeps the
  first failure in `primary_failure`, and reports cleanup separately so cleanup
  success cannot hide it and cleanup failure cannot be a pass. Later test
  results never overwrite the first failed test. The four-file evidence
  allowlist is unchanged.

`run.sh self-check` runs the cgroup verdict controls and the custody controls
with fake mount/namespace/boot operations over a real temporary directory (clean
setup/teardown, each refusal, partial setup, tampered setup, retained records,
foreign/replaced/aliased objects, cleanup failure, abort and the whole hosted
sequence) and the regressions for four review findings: SIGTERM/SIGINT/SIGHUP
at the first test, last test, setup, cgroup removal and custody cleanup (and
SIGKILL) never yield PASS; a failed listing, retained record, replaced or
aliased lock or foreign mount causes zero unmounts; a create that fails or
aborts after creating is retained as uncertain; test results 7,3,0 keep test 1
and status 7. Finalization regressions: a failed final publication or
unwritable final temporary file exits 1 with `INCOMPLETE` retained and owned
cleanup done; a failed `INCOMPLETE` placeholder exits 1 with nothing created;
SIGTERM/SIGINT/SIGHUP just before or just after the final rename exit 128+n
with FAIL and `interruption=`; SIGKILL before the rename leaves `INCOMPLETE`,
after it the published PASS. They never mount or use root.

## Proof status

Local Darwin tests cover all platform-neutral behavior and the refusal on
non-Linux, including the reservation (threads and a separate process), the
release barrier (barrier- and condition-coordinated, no sleeps), the descriptor
engine against real symlink/hardlink/replacement fixtures with external
sentinels, and the filter-marker phases with an unprivileged positive control.
Controller cgroup NAME reservation is covered through the real channel and
confined-run path with overlapping worker/reviewer reservations, using an
unprivileged temporary directory as the cgroup root: a test seam proving names
only, not kernel cgroups or confinement. The host mount identity record parser
and file checks run locally against real files owned by the invoking user
(production requires root); the hosted exclusive-custody test also rewrites the
real record in place (other boot, other namespace, extra line, mode 0644) and
proves production admission refuses each before restoring it.
Ownership changes to other users, `/proc/self/mountinfo` on a real bind mount,
Landlock and cgroups need the hosted run. The host setup likewise needs it:
real nsfs bind mounts, root ownership, the fixed initial-namespace inode
constants, the runner context values and the unchanged product admission
(including its mount-alias check of `/`) are NOT proven by the fake-operation
controls, which only show that unsupported, partial and failed states refuse,
are tracked and are never reported as PASS. Linux enforcement code and the hosted tests were type-checked and
Clippy-checked locally only in a throwaway copy with the Linux cfg gates
retargeted to Darwin (no link, nothing committed from that copy). The later
host mount identity record change in `production_registry` and the hosted
record control were not type-checked locally on Linux; Repository CI compiles
and Clippy-checks them on Linux. The hosted diagnostic
(`.github/workflows/we-w1-completion-linux.yml`) runs only under USER
authority; its results for each exact head are recorded with the PR, not in
this file. Root/controller compromise, kernel bugs and
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
