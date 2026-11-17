# Wave 3 library boundaries

`persisted_attempt::run_persisted_attempt` is the opt-in Workspace lifecycle
entrypoint. It persists the start checkpoint before launch, captures interval
checkpoints while waiting, competes for explicit cancellation, acknowledges
pre-signal evidence through the controller barrier, and persists a finish
checkpoint after collection. It uses the existing Workspace runner. The legacy
launch API and Runtime consumers remain compatible.

Each checkpoint binds workspace/attempt identity, HEAD, changed paths and a
content-addressed binary tracked diff. Retained stdout/stderr and diff files are
fsynced before admitted State metadata. Running output describes retained bytes,
not an unretained full stream. Untracked paths are recorded; untracked contents
and ignored files are not captured. Observations are bounded snapshots, not an
atomic filesystem transaction against concurrent worker writes.

Cancellation acceptance is returned explicitly. After a winning cancellation
or timeout, the controller requests a bounded output snapshot and waits at most
one second for trusted composition to acknowledge durable evidence before
signalling. Natural-completion signalling, post-spawn setup failure, Drop and
controller fault/unwind use the same single-use barrier. The lifecycle lock is
released while waiting. A cancellation accepted before persistence remains the
terminal cause even if the child exits during that wait.

`start_live_process_attempt_with_cleanup_evidence` and
`start_workspace_attempt_with_cleanup_evidence` expose this opt-in channel to
trusted supervisors. Retain `CleanupEvidenceReport` to observe evidence status
even after a failed start or Drop. Its receiver must be serviced independently
of collection, including during startup. The persisted driver services it in its
normal polling loop and writes a PRE_TERMINATION checkpoint before acknowledging.
An acknowledgement is a trusted assertion of durability, not a grant to signal
another process, reset a workspace or accept work.

Missing, failed or late acknowledgement permits bounded emergency cleanup of
the original owned process group. `CleanupEvidenceStatus` reports assurance
separately from process success; `PersistedAttemptError::EvidenceFailure` retains
both the persistence/acknowledgement failure and the actual collection result.
Late durable evidence is retained but cannot retroactively establish pre-action
ordering. The synchronous driver can still block inside an arbitrary slow sink;
the owning controller's additional wait remains bounded independently. Setup
failures before a handle is returned cannot be serviced by the polling driver
and use the emergency path. Legacy launch APIs do not request this barrier.

`TrustedAttemptEvidenceSinkV1::persist_verified_finalization` compares fresh
mechanical finalization evidence with the exact persisted checkpoint and binds
the attempt, workspace, final SHA, scope, checks and output refs through State's
atomic finish/finalization operation. Exact replay is supported; mechanical
success is not review acceptance. The real-State lifecycle fixture reopens the
checkpoint/output metadata and successful finish after this operation.

Output storage requires a private directory owned by trusted composition.
Incomplete file writes refuse on retry and require operator reconciliation.
The driver does not automatically retry a persistence error or relaunch a child.

## Immutable context

`GitBlobMaterializer` implements State's actual repository snapshot port. It
binds a trusted project/repository to a canonical local object store, validates
exact commit and blob types and reads through hardened Git commands. Separate
cat-file calls reuse the existing 1 MiB capture bound; larger blobs refuse before
any bytes reach State. State retains digest and provenance validation. Tests run
actual admitted State rehydration over real Git objects, including unknown-object
and oversize failure after another source was read; failed outcomes expose no
partial context and survive reopen.

## D12 publication and retention

Publication applies the adopted mapping, exact same-name branch refspecs and
non-force pushes. A per-ref Git lock spans the local anchor check through push,
refusing competing publication and cooperating Git branch updates. The lock is
removed only while its filesystem identity still matches the owned file. This
does not exclude hostile filesystem mutation by a principal with repository
write authority. A stale lock after a process crash refuses publication and
requires operator reconciliation; it is never blindly removed. Trusted composition supplies configuration, branch class and
exact acceptance; this module does not authenticate those issuers.

Publication preflight also refuses repository/worktree command, signing,
proxy, recursive-push and transport-policy configuration overrides. In the
publication-related configuration families, only named-remote URL/fetch mappings
are admitted. The hardened builder's fixed command-scope settings remain trusted.
Configuration keys are inspected through the bounded Git runner without reading
or logging their values. PUSH_ALWAYS with PUSH_ALL_CHECKPOINTS admits Checkpoint
branches only; Task and Workstream still require an eligible acceptance-gated mode.

Local-only returns before Git or remote access. Remote failure leaves local
progress intact. Tests use owned local remotes and verify rejected checkpoint
retention, non-fast-forward refusal and exact destination/commit publication.
Explicit remote deletion now consumes a single-use authorization issued by the
trusted State holder after user direction. State evaluates complete ref history
and retains unresolved and PUSH_ALWAYS evidence. Expected-SHA readback plus a
receive-side lease refuses moved refs. Reconciliation never repeats deletion.
Local branches remain retained. HTTPS and SSH use explicit trusted configuration
and credential-free endpoint identities. See [the D12 contract](d12-remote-policy.md)
for trust requirements, replay behavior and controlled-fixture qualification
limits. Historical SQL stays unchanged; v26 adds a version barrier against old writers.

## Production D4 composition

The trusted launcher calls `receipts_cli::workspace_recovery::spawn_supervisor`.
The binary's internal `__workspace-supervisor` entry authenticates a fresh,
private bootstrap, opens the exclusive Core writer, provisions a new task
workspace and persists provisioning identity before accepting commands. It
never adopts an existing workspace or reconstructs a process from a persisted
PID. Original process ownership and pinned worktree/Git directories remain in
the surviving supervisor when a controller exits or disconnects.

Supervisor bootstrap is one fixed 32-byte capability frame, authenticated before
provisioning. Readiness does not require stdin EOF. Trailing stdin bytes are
outside that frame, remain unread, and must never be interpreted as another
command. Commands continue to use the authenticated local socket. Incomplete
bootstrap retains the existing ten-second owner deadline and owned-child
kill/reap behavior.

`SupervisorClient` implements Review's actual `ReviewRecordSink` over bounded,
authenticated local frames. ReviewController and AcceptanceGate use this port;
the supervisor claims, seals and publishes through Core, rebuilding State's
stored seal. There is no receipts-e2e dependency. Mechanical finalization uses
fresh owning terminal evidence and the exact persisted checkpoint, then Core's
atomic finalization forwarder. Only an actually published ACCEPT for that
finished attempt can bind the exact physical workspace, commit and tree.
Publication without a matching physical anchor remains fail-closed.

Launch writes durable intent, attempt start, recovery admission (once an accepted
baseline exists), a start checkpoint and an attempt-bound Pending handoff before
returning launch success. Failure after durable attempt registration quarantines
further launch even if checkpoint storage is later restored. The supervisor holds
the original process handle;
worker output, a caller-supplied SHA and socket access alone are not recovery
authority. Trusted closure records ClosedWithoutHandoff. Verified successful
finalization records Eligible and cannot later be rewritten as closed.

`prepare_recovery`, `prepare_reset` and `recover` support controller reconnect
across durable prepared states. Recovery requires expired lease and execution
window, closed handoff and current exclusive State action authority. A fresh
State guard remains held through each recovery signal or physical reset and
readback. Pre-action checkpoint/output evidence is acknowledged in State; while
the action guard borrows the writer, cleanup output is fsynced locally before
the process controller's acknowledgement. Verified cleanup precedes stable
quiescent evidence. Post-reset evidence is persisted under the same exclusive
writer before State atomically records resolution and releases reservations.
Only then can stopping the supervisor release physical custody.

Reset uses the State-bound last accepted target, never a recent checkpoint.
HEAD and index tree must match, and physical tracked bytes and execute bits are
compared with accepted blobs. Index stat caches and assume-unchanged flags cannot
replace readback. All untracked and ignored paths are retained; obstructions
refuse reset and retain prepared custody. A prepared reset whose physical result
already matches is resolved through readback without repeating the action.
Recovery never writes a successful finish or acceptance for the discarded attempt.

The supervisor services the cleanup acknowledgement barrier for automatic
completion/timeout and uses the same durable local snapshot path during explicit
recovery or shutdown. It fsyncs the attempt-bound stdout/stderr artifacts and
receipt before acknowledging. Full Git checkpoints remain at launch,
finalization and recovery boundaries, outside the one-second cleanup snapshot
window; recovery action guards still require fresh acknowledged State evidence. Failed evidence preserves owned-process emergency cleanup
and reports assurance separately. An unresolved admission, failed assurance or
lost supervisor retains quarantine/reservations; no stale marker is removed to
permit adoption. Existing protected custody is refused before provisioning.

The startup `orphan_sweep` remains detection-only: an unfinished attempt or
persisted PID cannot substitute for the surviving owner's original custody.
Full goal commands and general product orchestration remain outside this entry.

### Qualification limits

Executed production-entry proof is local macOS arm64. The same supported Unix
primitives compile for Linux x86_64/aarch64, but new Linux execution qualification
is not claimed. Other hosts refuse the internal entry. Tracked symlinks and
submodules are unsupported reset targets. Existing Git capture bounds apply
(including the per-blob capture limit), and physical readback/preflight has an
aggregate time budget between bounded Git operations. Oversize or ambiguous
inputs refuse. Local socket path limits apply at startup before provisioning.

This is Workspace isolation, not a security sandbox. Same-user hostile mutation
is outside the existing Core/custody boundary; deployment must separate
lower-trust principals from State and supervisor credentials. This plain-workspace
path never adopts protected-custody workspaces or asserts protected-launch,
live-provider or whole-platform confinement qualification. Existing stores must
retain the supervisor clock contract; a different clock source is refused.
