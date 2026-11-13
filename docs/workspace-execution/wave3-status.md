# Wave 3 library boundaries

`persisted_attempt::run_persisted_attempt` is the opt-in Workspace lifecycle
entrypoint. It persists the start checkpoint before launch, captures interval
checkpoints while waiting, prepares explicit cancellation and persists its
bounded snapshot before competing for cancellation, and persists a finish
checkpoint after collection. It uses the existing Workspace runner. The legacy
launch API and Runtime consumers remain compatible.

Each checkpoint binds workspace/attempt identity, HEAD, changed paths and a
content-addressed binary tracked diff. Retained stdout/stderr and diff files are
fsynced before admitted State metadata. Running output describes retained bytes,
not an unretained full stream. Untracked paths are recorded; untracked contents
and ignored files are not captured. Observations are bounded snapshots, not an
atomic filesystem transaction against concurrent worker writes.

Cancellation acceptance is returned explicitly. Timeout or natural completion
can win while persistence runs; captured evidence remains, without claiming
pre-cleanup ordering for that winner. Persistence failure returns an error and
drops the owned attempt, preserving bounded cleanup. External persistence never
runs under controller locks. The wait loop also observes controller closure,
so a fault before any terminal claim returns a collection failure instead of
polling forever. The synchronous caller can block in a slow sink,
but the process controller still enforces its deadline independently.

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

## D4 recovery refusal

`orphan_sweep::detect_recovery_candidates` projects State's startup report of
unfinished attempts, preserving exact identities and rejecting foreign reports.
Unfinished attempts are candidates, not proven orphans: there is no admitted
attempt-to-lease/expected-window association. The exact last-accepted target is
required from trusted acceptance evidence; a recent checkpoint is insufficient.
Missing/invalid targets refuse. Even a valid target always refuses signaling or
reset while durable platform process identity and provisioning physical
continuity are unavailable. The function neither alters attempts nor accepts
partial work. No post-restart signal or destructive reset is implemented.

Timeout, Drop and controller-fault cleanup retain existing bounds and have no
durable evidence acknowledgement barrier. These paths cannot guarantee durable
capture before cleanup. Full D4 execution and durable automatic-cleanup evidence
remain pending A1's shared contracts, as explicitly scoped for this continuation.
