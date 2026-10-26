# WE029 disposable Linux qualification

Qualification source only. Original WE029 product implementation and MR020 stay
stopped. This does not install a launcher, implement mediation, establish product
lifecycle compatibility, or grant an assurance PASS. New privileged source needs
independent static review before publication. A2 inspects the exact commit; USER
starts fresh A4 static review; only after static PASS may A2 update existing PR185
for one authorized corrected hosted run. A3 must not push, open a PR or dispatch it.

The workflow and recorder admit only `pull_request: opened` or `reopened`, run
attempt 1, for existing PR185 from this repository's qualification branch into
this repository's `main`. Synchronization and manual reruns do not run
qualification. This stateless gate does not prevent repeated reopen events;
A2's execution ledger controls the one authorized corrected run, after exact-SHA
static review and acceptance, using exactly one close/reopen of PR185. No second
PR or unrestricted retry is authorized. It uses standard `ubuntu-24.04` x64 with a ten
minute job bound. Preparation verifies the event, exact head, baseline ancestry,
clean checkout, regular source modes and every changed path against the two-path
allowlist. `ci.yml` is preserved. Checkout credentials are not persisted. The only
permissions are `contents: read`; no environment, secret, OIDC, deployment or
worker-supplied privileged command is used.

## Fixed probes

`run.py prepare` runs unprivileged, records provenance and compiles `probe.c` using
the image's GCC. `run.py hosted` requires Linux x86_64/root, an attributable prepared
binary/source set and the exact image identity. Its administrator actions are only
for this disposable hosted VM. Do not execute it with sudo on a workstation.

The supervisor creates only a random `/tmp/we029-*` fixture and a matching private
cgroup under `/sys/fs/cgroup`. It refuses occupied numeric identities 61001/61002,
unsafe hardlink configuration, unavailable cgroup v2, unsupported Landlock, failed
mount/identity/capability/FD setup, unavailable file handles, or failed evidence
checks. There is no fallback. Every subprocess has a deadline, core dumps are
disabled, and each stdout/stderr stream is limited to 128 KiB. Hitting a limit is a
failure rather than accepted truncation. The total raw receipt limit is 1 MiB.

The actual synthetic repository is initialized once on `we029-task`. Metadata,
its admitted include and enabled `config.worktree` are controller-owned and private.
The worktree root is controller-owned, worker-group, sticky 01770; its parent is
controller-owned 0755. Existing ordinary payload files belong to the worker.
No proxy repository or branch switch is used.

| Case | Required observations |
| --- | --- |
| preflight | Confined controller observes root, branch, HEAD and porcelain cleanliness before worker edits. |
| worker | Real/effective/saved worker UID/GID, empty groups and all capability sets, inherited NNP before and after exec; inability to regain UID0. Parent/descendant write, open-truncate, truncate, chmod, unlink/rmdir, rename, exchange, hardlink and redirection probes against metadata/ancestors. Protected metadata inode/mode/owner/content equality is checked afterward. An inherited writable metadata FD is closed; write/truncate/chmod attempts verify EBADF. Proc-root metadata access and namespace changes are denied. Root, own and actual sibling cgroup migration opens are denied in parent and descendant. Root-level edit/delete and new worker0600 payload succeed. |
| lifecycle | Leader exits while double-fork/setsid descendant holds a pipe. Leader reap, populated=1 and non-EOF are observed first. Then `cgroup.kill`, populated=0, EOF and adopted-child reaps are separately observed. |
| owned-failure | Same owned tree, with an injected leader exit72. Cleanup still proves cgroup emptiness, EOF and reaps; exit72 is retained as an expected failure, never rewritten to exit0. |
| controller | Same confined controller UID/capabilities reads worker0600 without chmod, while a real visible outside sentinel and proc-root/retained-directory-FD routes are denied. A real outside file handle must first open/read under host privileged control and then under the same controller UID/capabilities before confinement. Its confined open must return the explicit seccomp EPERM. Unsupported handles refuse the case. The same confined controller then stages/commits the actual edit/delete/private payload and observes the checkpoint SHA/status/diff. An independent fixed host observer checks SHA, parent, branch, clean status, exact change set, private bytes and unchanged repository inodes. |

All cases use `clone3(CLONE_INTO_CGROUP)` and verify the child's exact cgroup before
identity setup. The validated nonnegative cgroup FD is converted directly to the
Linux field type `__u64`, preserving its value and all strict compiler checks.
There is no fork-then-migrate launch. Bounding capabilities are
dropped while privileged, followed by supplementary group removal, setresgid/uid,
explicit capability sets and verification. The runtime capability range must
match the compile-time range; unknown new capabilities refuse setup. NNP is set
in the privileged parent and inherited. The controller retains only
CAP_DAC_READ_SEARCH in effective/permitted/inheritable/bounding/ambient sets;
ordinary exec retains and verifies it. Workers retain none.

## Controller confinement and syscall policy — review required

Each child receives a private mount namespace/chroot containing the synthetic
repository, explicitly copied Git/probe binaries and their `ldd`-identified shared
libraries, one `/dev/null` device, proc and a read-only view of cgroups. Mounts are
made private before setup. No host service sockets or writable host mounts are
provided. Deliberately retained directory and real host mount-namespace FDs are used solely for the controller
bypass tests and closed before Git runs; all other privileged FDs are closed.

The controller additionally requires Landlock ABI >=3. Its filesystem rules
handle all ABI3 rights (through TRUNCATE), allowing the repository tree, read/exec
of fixed tool/library trees, and read/write of `/dev/null`. The outside sentinel
remains visible to `stat` but an actual read must be denied with the same UID and
CAP_DAC_READ_SEARCH as the positive worker0600 read. This is a meaningful denial,
not a missing-path witness. Landlock's known non-filesystem/metadata limitations
are not claimed covered by this candidate.

Both identities install an explicit seccomp filter before synthetic worker code
or controller Git. Architecture must be AUDIT_ARCH_X86_64; alternate ABI/x32 calls
kill the process. The following calls return EPERM:

- `open_by_handle_at` (CAP_DAC_READ_SEARCH's handle bypass), `setns`, `unshare`;
- `mount`, `umount2`, `pivot_root`, `chroot`, `open_tree`, `move_mount`, `fsopen`,
  `fsmount`, `mount_setattr`;
- `ptrace`, `process_vm_readv`, `process_vm_writev`, `pidfd_getfd`.

Other syscalls remain available to this fixed fixture. This is an explicit small
candidate policy, not a general sandbox claim. It is installed before any fork;
children inherit it. Clone3 is used by the privileged supervisor before filtering.
No arbitrary shell, executable, Git arguments, config, ref or request from a worker
is accepted by the controller. Git commands in the probe are fixed at compile time.

## Failure, cleanup and evidence

A C setup/assertion failure exits70 with expression/line/errno. The supervisor
stops further cases, writes refusal evidence, kills only owned cgroups, waits at
most five seconds per emptiness/reap phase and records receipts. It becomes a
subreaper so failed C supervisors cannot orphan owned descendants. Unknown cleanup
retains the fixture/cgroup, refuses success and stops; there are no retries or UID
reuse. Mount namespaces disappear with their owned processes. Fixture deletion
happens only after known cleanup. The deliberate owned-failure case is distinct
from an unexpected qualification failure.

Only `provenance.json`, `results.json`, `raw.jsonl` and their `SHA256SUMS` are uploaded
for three days, including preparation/qualification failures. No whole environment,
fixture, checkout, host file, credential or private operating record is archived.
`raw.jsonl` preserves exact subprocess stdout/stderr in base64 with byte counts,
hashes, argv, cwd, exits and timeout flags. Probe receipts are read only after the supervisor cleanup/reap callback; unknown writer quiescence is recorded as incomplete failure evidence. For C-invoked Git, COMMAND and REAP
receipts bracket complete unmodified Git stdout; stderr remains the original raw
probe stderr stream. This is synthetic evidence, not the product's evidence ABI.
`results.json` names each completed/refused case; cases after a refusal are NOT RUN.
The actual event action, repository, PR number, run ID and attempt are recorded
in `provenance.json`, including compilation failures after admission. The image
OS/version, kernel/architecture, filesystem, tool versions, exact
checkout/harness SHA, source/binary hashes and cleanup receipts bind the evidence.

Local static preparation: `python3 .github/scripts/we029-linux-qualification/run.py
self-check` verifies opened/reopened admission, wrong-PR/repository/branch/head/event/attempt
refusal and accurate event receipts, allowlist rejection, NUL preservation, failed-command receipts,
timeout cleanup, output-limit/unknown-cleanup refusal and evidence sealing without Linux setup. Linux execution is
NOT RUN until the separately authorized hosted run. Do not run Rust suites or
scanners for untouched product files.

Primary references: [clone3](https://man7.org/linux/man-pages/man2/clone3.2.html),
[cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html),
[Landlock](https://docs.kernel.org/userspace-api/landlock.html),
[capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html),
[file handles](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html),
[seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html),
[NNP](https://docs.kernel.org/userspace-api/no_new_privs.html).
Action pins were verified against upstream
[checkout commit](https://github.com/actions/checkout/commit/11d5960a326750d5838078e36cf38b85af677262)
and [upload-artifact v4.6.2](https://github.com/actions/upload-artifact/commit/ea165f8d65b6e75b540449e92b4886f43607fa02).
