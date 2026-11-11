# Wave 3 library boundaries

`checkpoint_writer::capture_and_persist` captures real Git checkpoint evidence
then writes through the admitted State sink. Composition must call it at start,
interval, prepared cancellation and finish. It is not an automatic scheduler.
`TrustedAttemptEvidenceSinkV1::persist_finalization` uses State's atomic
finish/finalization operation. `output_store::store_output` fsyncs bounded retained
bytes and their directory before metadata is published. Its directory must be
private to trusted composition. Incomplete writes refuse on retry and need
operator reconciliation. Output digests describe retained bytes only.

`GitBlobMaterializer` binds a trusted project/repository to a canonical local
object store, validates exact commit and blob types and reads immutable objects
through hardened bounded Git commands. It uses separate cat-file invocations
rather than a batch protocol. The existing runner's 1 MiB capture limit applies;
larger blobs refuse before any bytes reach State. State retains source digest
and provenance verification. A real State rehydration witness remains pending.

`remote_publish::publish` applies the D12 mapping with explicit same-name branch
refspecs and an exact local anchor check. It returns remote failure without
changing local history. Inputs, including exact acceptance, must originate in
trusted composition; this module does not authenticate issuers. Local-only
returns before any Git or remote access. Only local file transport is enabled;
production credential-backed transport and durable publication-result recording
remain pending. Retention/pruning is not implemented; published evidence remains.

## Recovery stop

The admitted State setup has no durable process start identity or provisioning
physical continuity record. Current-target checks in recovery preparation cannot
prove either. No post-restart signal or destructive reset is implemented; retain
data and escalate until A1 admits those durable contracts. An exact accepted
commit is required for RESET_TO_LAST_ACCEPTED; a recent checkpoint is insufficient.

Timeout, Drop and controller-fault cleanup retain their existing bounds and have
no durable evidence acknowledgement barrier. These paths cannot guarantee
capture persisted before cleanup. The library helpers do not close that gap.
This package remains partial and requires completion before lane acceptance.

Publication requires composition to exclude concurrent branch writers from the
anchor check through push completion. Without that exclusion a branch can move
between those operations. This helper alone does not guarantee that the pushed
commit remains the accepted anchor. Do not enable automatic publication until
that exclusion and durable result recording are supplied.
