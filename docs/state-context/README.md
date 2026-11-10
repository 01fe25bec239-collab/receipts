# State: Wave 1 completion notes

State stores what other owners define. It never routes, reviews, runs failover or decides acceptance. This page covers what the Wave 1 completion package added. Source of truth: `src/state/**`.

## Schema version 19

Migration 17 (`decision_records`) adds three tables and immutable-event triggers. Migration 18 (`manifest_successors`) retains immutable historical manifests, seals their children and closes role/ownership duplicate-insert replacement paths. Migrations 1–17 and migration18 SQL are unchanged. Migration19 (`row_identity`) rebuilds the 23 protected tables WITHOUT ROWID, preserving logical keys, values, children, indexes, seals, history and CAS pointer protocols. Hidden SQLite row identities are unavailable; no public rowid API is provided.

| Table | What it holds | Immutability |
| --- | --- | --- |
| `decision_records` | Routing decisions, registry capabilities, model observations, availability observations, review requests, A4 reviews (with their findings), integration requests and integration decisions. Each row keeps the exact supplied JSON bytes, their SHA-256, and extracted identity columns (task, linked request, bound SHA). | No update, delete or replace |
| `provenance_links` | One row per task and link kind: START, IMPLEMENTATION and REVIEW SHAs, the VERDICT and ACCEPTANCE references, and the WORKSTREAM and MAIN SHAs | No update, delete or replace |
| `logical_role_history` | A before/after snapshot of a role's status, binding pointer, epoch and manifest pointer for every change | No update or delete. Rows must start from the role's current row and use the next sequence number. |

Version 17 also adds triggers on existing tables:

- `event` rows cannot be updated or deleted, and an INSERT that names an existing `event_id` aborts however it is spelled (`OR REPLACE`, `REPLACE`, upsert, `OR IGNORE`). A plain INSERT of a new id is the only way a row appears, so append, find and list behave as before. These are direct-SQL backstops, not a defence against a caller who can drop schema objects: open, `StateReader` and explicit migration compare every stored definition with the migration text and refuse a missing, altered or extra trigger on `event` as corruption.
- A role's identity columns and ownership paths cannot be updated or deleted.
- Role rows cannot be deleted.
- An update to a pointer column is refused unless a fresh matching history row exists.

Upgrades:

- `SqliteStateRepository::migrate_existing_to_current` upgrades exact v11–v18 stores, verifying each intermediate postcondition, including exact v17 and v18 intermediate heads. Existing v19 stores are verified idempotently.
- Ordinary `open` refuses every non-current version.
- v1–v10 stores and unknown versions get typed refusals and are never rewritten: `SchemaVersionMismatch` from `open`, and `UNSUPPORTED_SCHEMA` from explicit migration.

## Decision and provenance records

`DecisionRecordV1::parse` validates the whole owner-frozen schema shape of all eight families (`RoutingDecision`, `ModelCapability`, `ModelObservation`, `AvailabilityState`, `ReviewRequest`, `A4Review`, `IntegrationRequest`, `IntegrationDecision`). The shape tables live in `decision_schema.rs`. At every nested level:

- no duplicate decoded keys at any depth;
- no unknown property and every required property present (selection reason, score components, capability evidence and its source reference, alternatives, registry freshness, acceptance criteria, review policy, references, reviewer, findings, dimensions, reproduction checks, included tasks, attestations, check results, provenance entries);
- each property's type, enum, identifier length (1–200), strict 40-hex SHA where the schema has the pattern, `minItems`, and exact integer and range rules.

Open fields stay open: `ModelCapability.value` and `capability_evidence[].value` accept any JSON. Numbers stay raw tokens and are never converted through `f64`: `1.0`, `1e3` and `-0` are integers, huge integers and exponents are admitted, and `expected_rejection_probability` is compared with 0 and 1 exactly. `date-time` is an annotation, so those fields accept any string. Plain-string SHA-like fields (`merge_commit`, `acceptance_sha`, `integration_sha`) are not pattern-checked because the schemas do not say so. Stored records are re-admitted on every read and stored task/link/SHA metadata must match the decoded JSON exactly. Task filtering validates the project/family snapshot before applying the filter, so damaged extracted metadata cannot hide a matching record. Duplicate-key traversal uses a heap worklist, preserving raw values without recursive descent. Thus so damaged nested content fails closed even when the stored digest was recomputed. The `jsonschema` crate is a test-only oracle; the validators are small `serde`/`RawValue` tables with no new dependency.

State stores registry and lifecycle observations as supplied. Lifecycle eligibility and transition policy belong to Model Routing; a stored row is never routing or dispatch authority.

`provenance_chain(project, task)` returns:

- the links that were recorded;
- the links that are missing;
- contradictions with stored records:
  - REVIEW_SHA differs from IMPLEMENTATION_SHA;
  - a verdict or acceptance reference is dangling;
  - the referenced review is for another task, or its reviewed SHA differs;
  - the referenced integration decision's outcome is not `ACCEPT`;
  - an `ACCEPT` decision is the acceptance but the same task's review at the same SHA (the VERDICT_REF) is a `REJECT`;
  - that decision has no provenance entry for the task (absent or foreign-only), or more than one (ambiguous, even if identical);
  - its one entry for the task disagrees with a recorded START, IMPLEMENTATION or REVIEW link, or its `integration_sha` disagrees with MAIN_SHA.

Empty provenance, nonaccept outcomes and `REJECT` reviews are valid, readable, immutable stored documents; a rejected review with no acceptance claimed is ordinary incomplete history, not a contradiction. `PASS` and `PASS_WITH_NONBLOCKING_FINDINGS` are not rejected for carrying nonblocking findings. Contradictions are reported, never used to refuse an append. Unrecorded links are never inferred. A complete and consistent chain is evidence for a gate to evaluate. It is not acceptance and authenticates no issuer.

## Role lifecycle

| API | Rule |
| --- | --- |
| `transition_logical_role_status` | Allowed: ACTIVE↔SUSPENDED and ACTIVE/SUSPENDED→RETIRED. RETIRED is terminal, and a change to the same status is refused. Retirement needs no binding pointer and no unreleased binding. |
| `set_logical_role_active_binding` | Compare-and-set on the current pointer. A new binding must exist, belong to the role and be fully unreleased. A replaced or cleared binding must be fully released. Dangling or partial bindings are refused. |
| `supersede_context_manifest` | Requires the expected epoch and an attached predecessor. Creates a distinct immutable snapshot and its ordered children, CAS-updates the pointer and appends history in one serialized transaction. Same-epoch replacement is allowed; changed epochs must exist in the same project. Failures leave no orphan snapshot. |
| `set_logical_role_context` | Compare-and-set on the epoch and manifest. The epoch cannot go backwards. A changed epoch must exist in the project's history. A manifest must match the role, project and new epoch. A manifest pointer cannot be cleared. |

Lifecycle policy refusals use `RoleChangeRefusal`; manifest identity/shape failures retain their existing typed errors. Refusals write nothing. Legacy `create_context_manifest` remains create-only, refuses a second version for the role and has no pointer/history effects. Historical manifests remain readable by identity. Only the role pointer selects current authority. The legacy create and release binding APIs do not touch role pointers.

## Chronological event query

`list_events_chronological`:

- selects by exact project, subject and type first;
- validates every selected `occurred_at` as `CanonicalTimestampV1` in one snapshot;
- applies an inclusive `since` and an exclusive `until`;
- sorts by time, then binary `event_id`;
- applies `limit` last.

Invalid or reversed bounds are refused, even when the result would be empty. If any selected timestamp is outside the domain, the whole query is refused. Rows outside the selection never affect the result. `list_events` and stored `occurred_at` bytes are unchanged.

## Lease renewal

Public renewal makes its whole decision in one serialized transaction and commits the lease and the accepted trusted time together. The decision checks:

- the binding exists and is unreleased;
- the stored and new deadlines are canonical;
- the clock source is continuous and time has not gone backwards;
- trusted now is before the current deadline;
- the new deadline is strictly later.

Every refusal leaves the watermark and the binding unchanged, including a refusal at a later still-eligible sample. An unsupported stored deadline is refused and never rewritten. Explicit expiry keeps its own existing fence. The independently fenced helper that used to back renewal is `cfg(test)` only and has no production caller.

Two writers on one store serialise on SQLite's write lock. Racing renewals to the same later deadline commit exactly one renewal; the other gets `ExecutorLeaseRenewalRefused`. Renewals to different later deadlines can both succeed in order. Role status, binding and context changes race the same way: one commit and one `StaleExpectation`. A held lock gives a diagnosed `TransactionBeginFailed` with no partial write. Tests: `two_writer_tests.rs`.

## Local authority

- Every one of the 53 event types and every epoch advance has one publisher: the trusted core (`authoritative_event_publisher`).
- Actor kind and id are recorded causal data, not credentials.
- `StateReader::open_existing` opens a current store with SQLite's read-only flag and exposes reads only. Compile-fail doctests prove it has no write methods.
- The legacy writable repository is not secured against arbitrary in-process code.
- The deployed issuer and the conversion of every writer remain next-wave work.

## Wire codecs (`receipts_state::wire`)

There are six codecs: LogicalRole, ExecutorBinding, ContextManifest, ContextEpoch, ProductEntitlement and ActivationState.

What the codecs preserve:

- absent versus `null`, and absent versus `[]`;
- exact number tokens, with no numeric ceiling;
- the open `derived_state` bytes (an object with duplicate decoded keys at any depth is refused, because it has no single identity; the schema oracle would accept it by keeping the last key).

`date-time` is treated as a schema annotation, so dates are not checked as dates.

Conversion to operational carriers can fail. Each failure names the unsupported field. Examples:

- non-canonical decimal epochs such as `1.0`, `1e3` or `-0`;
- `URL` manifest references;
- `derived_state`.

`ProductEntitlementWireV1::to_physical` goes only through the existing entitlement parser. Null optionals, out-of-range or exponent versions and non-canonical dates never become physical or verified values.

Conformance cases are generated in `wire/wire_tests.rs` and checked against the pinned offline `jsonschema` oracle. The frozen corpus has no State fixtures. Known oracle limits:

- the oracle cannot parse numbers beyond `f64`, so only the codec tests those;
- serde refuses lone-surrogate strings.

## Trusted-core write boundary (schema v20)

`TrustedCoreStore` is the only public State writer. Every legacy writable entrypoint (`SqliteStateRepository::open`, `migrate_existing_to_current`, `run_transaction` and all mutators) is crate-private. Ordinary reads, `StateReader` and `ReadOnlyGraphReader` are unchanged.

- `open_exclusive` / `migrate_exclusive` require an absolute path. After resolving system aliases once, the directory chain is walked from `/` with `openat(O_DIRECTORY|O_NOFOLLOW)` on held descriptors: every ancestor must be owned by root or the effective user and not group/other-writable unless it is sticky and the next entry is owned by root or this user (so a shared `/tmp`-style parent is valid, a non-sticky writable one is not). The State directory itself is owned by the effective user with no group/other access. The lock, the database and any pre-existing `-wal`/`-shm`/`-journal` sidecar are examined with `fstatat(AT_SYMLINK_NOFOLLOW)` before anything opens them and must be single-link private regular files; regular files are opened `O_NOFOLLOW|O_NONBLOCK` and identity-compared. After SQLite opens the pathname, the directory and database identities and the sidecars are re-checked. It takes a non-blocking `flock` on `<db>.writer-lock`, never repairs permissions, never touches a refused foreign object and never upgrades on ordinary open. SQLite's own pathname open is safe only because other principals cannot alter the validated chain; same-user code can.
- A successfully acquired writer lock is explicitly unlocked by its acquiring process after the owned SQLite connection closes. Closing only the descriptor is insufficient when an unrelated forked child still holds a copy, even with close-on-exec. Failed acquisition never unlocks; secure-open and initialization failures release an acquired lock, with SQLite closed first where it was opened. No contention retry or lock-file deletion is used. A forked process must not use the inherited SQLite writer; the lock guard also avoids unlocking the parent's live lock from a child destructor. Controls retain a bounded forked child across writer teardown, verify rollback/reopen and live-writer exclusion, and check retained-owner and failed-open cleanup. This reproduces a lock-lifetime defect, not the identity of the holder in the original intermittent suite failures.
- Each write consumes one non-cloneable `StateWriteAdmission`, minted by the store's holder and bound to that live writer instance (a private, process-unique per-open identity) as well as the durable random store identity, one operation family and the project, plus graph/source/compiler/policy where the payload carries them. A copied database keeps the durable identity but never accepts the original's admissions, and an admission minted before a close/reopen is refused by the reopened store. State re-checks equality against the actual payload or the role/binding/attempt it targets; any mismatch writes nothing. The durable identity, also recorded in event receipts, is copyable logical history, not physical provenance; receipts are local unsigned records.
- The Orchestration core's `CoreStateWriter` is the issuer for every State operation family (graph genesis/mutation/node result, attempts, checkpoints, output refs, decision records, provenance links, events, context epochs/manifests/rehydration, roles, executor bindings and availability facts). Its immutable `CoreAdmissionIdentity`, supplied by the trusted composition, names the admitted compiler, policy and, per admitted project, the admitted source revision (a typed `evidence:` reference, not a commit SHA). Subjects come from that context and from the stored records the write targets: genesis cites the admitted source, mutations the stored graph's genesis source, graph-bound writes require the stored graph, and role/binding/attempt writes resolve their stored project. Non-admitted projects, unknown subjects and foreign compiler/policy work are refused before State; State still re-checks the payload. Its public methods are an in-process capability, not exclusion of same-process or same-user code.
- `EventProposalV1` is immutable data any component may build. Before persistence the core resolves the proposal's referenced content through the composition's `CoreContentResolverV1` (scoped to the proposal's admitted project, not arbitrary paths) and verifies the actual bytes against the `sha256:` digest; missing, foreign or mismatched content, or an unverifiable digest, persists nothing. `persist_event_proposal` appends the event (trusted `occurred_at`) and an `event_proposal_receipt` row in one transaction; a replayed proposal or reused event id is refused atomically. All 53 events keep TrustedCore as authoritative publisher; the proposal origin is causal data only.
- Availability facts (D5) are append-only with a gap-free State-assigned sequence per provider/runtime/model partition. Recovery claims and retry-after expiry must follow the exact latest sequence and never clear the last demotion or produce `Available`; no authenticated post-demotion producer exists yet (adopted post-W3 deferral).

Migration 20 adds `state_store_identity`, `event_proposal_receipt` and `availability_fact`, verifying the exact v19 store before and the v20 store after. Migration SQL for v1–v19 is unchanged; the v19 verifier now accepts head 19 or later.

Limits: this is an object-capability boundary inside the trusted core process. It does not authenticate other code in the same address space, and file modes plus a cooperative lock do not exclude a process with the same filesystem authority. Excluding lower-trust principals (separate identity, no State directory access, no inherited descriptors or writer channels) is a deployment requirement. Separate-principal native proof is NOT RUN.

## Recovery and rehydration (ST-W2-1/2)

No new DDL of its own (it was written against schema v20). Everything below uses existing tables, and each action is one serialized transaction that commits rows, role history and its frozen event together, or writes nothing.

| `TrustedCoreStore` method | Admission family | What it does |
| --- | --- | --- |
| `recover_startup` | `ExecutorBinding`, project | Fences one trusted-clock sample against the project watermark. Releases every unreleased binding whose lease deadline is at or before trusted now (`LEASE_EXPIRED` plus `EXECUTOR_RELEASED`). Then reports, from the same snapshot, each role's startup classification and continuation result, plus the unfinished attempts. A repeat scan releases nothing and changes no record. A corrupt deadline, clock regression, foreign clock or busy store refuses the whole scan. |
| `rebind_role` | `ExecutorBinding`, role's project | Expected-current CAS on the role's binding pointer. Optionally releases the still-unreleased current binding with a non-`LEASE_EXPIRED` reason. Creates the successor binding, which must be unreleased, not claim rehydration, be bound no later than now and have a lease after now. Moves the pointer with history and appends `EXECUTOR_BOUND` or `EXECUTOR_REPLACED`. The role must be `ACTIVE` and keeps its identity and status. A RUNTIME_A1 rebind is refused while another A1 role in the project holds an unreleased binding. Returns `RehydrationRequiredV1`. |
| `succeed_changed_context_sources` | `ContextEpoch`, role's project | Takes the role's SUCCEEDED rehydration at its current (latest) epoch and manifest that observed `CHANGED` digests. Advances the project epoch with those changed sources and this role invalidated, inserts an immutable successor manifest carrying the observed digests at the new epoch, CAS-moves the role context with history and appends `CONTEXT_EPOCH_ADVANCED`. |

`continuation_clearance(clock, role)` is a read and writes nothing; a sample behind the watermark is refused. It clears a role only when all of these hold:

- the role is `ACTIVE`;
- its pointer names an unreleased binding;
- trusted now is before that binding's lease deadline;
- no later epoch invalidated the role;
- a SUCCEEDED rehydration with no `CHANGED` source exists for exactly that binding, the role's current manifest and its current epoch, **and it is not superseded by later failed evidence** (see "Rehydration order and continuation invalidation" below).

Every other case is a typed `RecoveryRefusalV1`. "Needs rebind" is `NO_ACTIVE_BINDING`, `BINDING_RELEASED` or `LEASE_EXPIRED` (see `requires_rebind`). Changed digests give `CHANGED_SOURCES_PENDING`. The frozen role status set is unchanged.

`list_unfinished_attempts(project)` lists attempts that have a start row and no finish row. That is all State can prove. Whether an attempt is orphaned, and capturing or recovering its worktree, belong to Workspace. State runs no Git command, kills no process and resets no checkout.

Events use only frozen types, with State-derived canonical references and domain-separated digests. Event ids are ULID-shaped SHA-256 prefixes of the action's subject, so a repeated action cannot mint a second event.

Materializers (`receipts_state::materializers`):

- `ArtifactFileMaterializerV1` reads catalogued artifacts (`artifact_id` → relative path + SHA-256) below one root. Each read walks the path from the held root descriptor with `openat(O_NOFOLLOW)`. It refuses the following:
  - `..`, absolute, empty or backslash components;
  - symlinks at any depth;
  - hard links;
  - non-regular files such as FIFOs and directories;
  - files over the bound;
  - files whose identity, size or timestamps change during the read;
  - SHA-256 mismatches against the catalog.

  Every refusal is a typed source failure, so the attempt fails with no body.
- `MaterializerRegistryV1::ports()` supplies the repository-blob port and the artifact port to `rehydrate_context`. An unplugged port fails with `MATERIALIZER_NOT_REGISTERED`. State queries keep their existing closed registry. Workspace's real `GitBlobMaterializer` is WE-W3-4 and is not implemented here; tests use a fake blob port only.

Missing Core interface: Orchestration's `CoreStateWriter` does not yet forward `recover_startup`, `rebind_role` or `succeed_changed_context_sources`. The State side and its admission checks are complete; Core needs three forwarding methods using its existing `admit`/`admit_role` with the families above.

## Rehydration order and continuation invalidation (schema v22)

Continuation used to pick any matching SUCCEEDED attempt, ordered by caller-chosen attempt id, and ignored FAILED attempts, so an old success could clear a role after a later CHANGED or unreadable reread. Migration 22 (`order_and_seal`) adds `context_rehydration_order`, one immutable row per attempt, assigned by State inside the attempt's own transaction:

- `sequence`: the next per-project commit sequence (1, 2, 3, ...; a unique partial index and an append-only trigger enforce it). Attempt ids, `started_at` and `completed_at` are caller data and never decide order.
- `fence_sequence`: the project's highest sequence at the moment the reread began (read before any materializer runs). It is always strictly behind the attempt's own `sequence`.

`continuation_clearance` and `recover_startup` share one decision over the attempts of exactly the role's project, role, **current binding**, **current manifest** and **current epoch** (attempts for any other binding, manifest, epoch, role or project never matter):

1. an attempt with no order row (not expected after v22; a raw insert) is `DURABLE_INCONSISTENCY`;
2. any attempt that observed a `CHANGED` source makes the scope `CHANGED_SOURCES_PENDING` for this epoch, whatever came later. A later matching reread at the same epoch does not undo it; only `succeed_changed_context_sources` (a new epoch and manifest) followed by a successful reread there clears;
3. otherwise a SUCCEEDED attempt clears only if its `fence_sequence` is at or after the newest FAILED attempt's `sequence` in the scope, i.e. the success began reading after the last failure had committed. A success that was still reading when a newer failure committed (the delayed stale outcome) is kept as honest immutable history but never clears; a fresh reread restores clearance. Among clearing attempts the highest `sequence` is reported.

History is never rewritten or deleted: all attempts stay readable. A failure does not block forever; one fresh verified reread of the exact scope restores clearance.

Upgrade: attempts that exist when a store is migrated to v22 are backfilled at sequence 0, because their relative order was never recorded. A lone upgraded success with no failure beside it still clears. If the scope holds an upgraded failure beside an upgraded success, or any post-upgrade failure, upgraded successes do not clear: the role fails closed (`REHYDRATION_REQUIRED`) until one fresh rehydration succeeds. Upgraded `CHANGED` evidence stays pending.

Concurrency model: `rehydrate_context` reads sources outside any transaction and commits its attempt in one `run_transaction`, with other connections or processes free to commit in between. The fence closes that window: whatever commits after the fence was taken is newer than the reread, whatever the timestamps claim.

## Shared atomic publication and review claims (schema v21)

Migration 21 (`publication_claims`) adds `atomic_publication`, `atomic_publication_availability` and `review_claim`, all `WITHOUT ROWID` with immutability and no-replace triggers like v20. It verifies the exact v20 store before and the v21 store after. Migrations 1–20 SQL is unchanged; v20's own apply now verifies exact v20 (not "current"), and `verify_current` checks the v21 objects too. `migrate_exclusive` upgrades any exact v11–v21 store to the current head (v23) in resumable steps (a failed v21 step leaves the exact v20 store; v21's own apply now verifies exact v21). Every operation below is a typed `TrustedCoreStore` method that consumes one admission. There is no public SQL, transaction closure or generic batch.

### Graph mutation with its results (`GraphMutation` family)

```rust
let publication = GraphMutationPublicationV1::new(
    vec![GraphResultAttachmentV1::new("1", "r1", result)?],   // (version holding the node, reference, result)
    mutation, snapshot,
)?;
let admission = store.admit(graph_subject /* Op::GraphMutation, project, graph, source, compiler, policy */);
match store.publish_graph_mutation_v1(admission, &clock, &publication)? {
    GraphPublicationOutcomeV1::Applied => { /* results, snapshot, history, seal, watermark, head: all written */ }
    GraphPublicationOutcomeV1::Replayed => { /* exact earlier publication; nothing written */ }
}
```

One serialized transaction writes the result rows, snapshot, mutation history, seal, trusted-time watermark and the head CAS. Any failure (clock regression, missing or conflicting result, stale parent, storage error) leaves none of them, including the result rows and the watermark. The admission binds project, graph, source, compiler and policy exactly as `append_graph_mutation_v1`; every result must belong to the mutation's graph. An exact retry (same mutation bytes, same snapshot digest, same result bytes) returns `Replayed` without touching the watermark; the same mutation id with different content is `PublicationFailureCode::ConflictingRetry`. A result row already stored with identical version, node and bytes (left by the older one-row writer) is adopted, never replaced; any different existing row refuses and the whole publication rolls back. The snapshot digest remains a Core-supplied content identity that State does not recompute.

### Decision publication (`DecisionPublication` family)

```rust
let package = DecisionPublicationV1::new(
    "project", "publication-id",
    vec![a4_review_record],                         // DecisionRecordV1 (REJECT included)
    vec![PublicationEventV1::new(proposal, "01ARZ…")?],   // proposals + core-minted ULIDs
    vec![availability_fact],                        // AvailabilityFactV1, State assigns sequences in order
)?
.with_claim_settlement(&ticket, ReviewClaimSettlementV1::completed("REJECT", Some("rev-1"), None)?)?; // optional
let admission = store.admit(project_subject /* Op::DecisionPublication */);
let receipt = store.publish_decision_package_v1(admission, &clock, &package)?; // DecisionPublicationReceiptV1
```

One trusted-clock sample and one serialized transaction write the publication identity, the records, each event with its receipt, the availability facts (each follows the exact latest sequence, including facts earlier in the same package) and the claim settlement. The identity `(project, publication_id)` is bound to the package SHA-256. Outcomes:

- first call: `replayed = false`, with the event receipts and availability sequences;
- exact retry, also after close/reopen: `replayed = true` with the same recorded `recorded_at`, event receipts, availability sequences and settled claim, and no write at all (the watermark does not move);
- the same identity with different content: `ConflictingRetry`, nothing written;
- a record or proposal whose own identity already exists outside a recorded publication (for example a record left by the older non-atomic writer): the original `AlreadyExists` error, the whole package rolled back. State does not "adopt" such partial legacy state; reconciling it needs an explicit disposition.

Records, receipts and facts stay immutable: a stored `REJECT` review can never be replaced by a later package, and a failed or conflicting package leaves no event or watermark change. State checks shape, project and identity; **referenced-content validation stays with the trusted core**, which must resolve and hash every event payload (as `CoreStateWriter::persist_event_proposal` does) before it builds the package.

### Durable review claims (`ReviewClaim` family)

```rust
let admission = store.admit(project_subject /* Op::ReviewClaim */);
match store.claim_review_v1(admission, &clock, &ReviewClaimRequestV1::new(project, claim_id, task_id, Some(sha), &request_sha256)?)? {
    ReviewClaimOutcomeV1::Claimed(ticket) => { /* ONLY here may routing/provider/execution run */ }
    ReviewClaimOutcomeV1::Existing(record) => { /* never execute; record.state says why */ }
}
```

The claim is inserted before routing or execution. `claim_id` must be the deterministic identity of the unit of work (for example the review request or attempt id), never a fresh random value per callback. States:

| State | Meaning | Exits |
| --- | --- | --- |
| `CLAIMED` | unfinished or uncertain (a crash may have lost a provider response) | settle, or explicit abandon |
| `COMPLETED` | recorded disposition (`outcome_kind` token, optional `outcome_ref`, `outcome_sha256`) | none |
| `REFUSED` | retained terminal policy or safety refusal (for example `POLICY_BLOCKED`) | none |
| `ABANDONED` | explicit, authority-named end | none |

- Concurrent or repeated callbacks in one process serialize through the exclusive store (and below it through SQLite `BEGIN IMMEDIATE`); exactly one gets `Claimed`. A restart yields `Existing(Claimed)` with no ticket, so nothing resumes it. There is no TTL, expiry or automatic reopening anywhere; a trigger lets a claim leave `CLAIMED` exactly once.
- `settle_review_claim_v1(admission, clock, &ticket, &settlement)` records `completed(..)` or `refused(..)`. The ticket is non-`Clone` and bound to the live store instance; a ticket from another instance or store is `WrongStore`. Settling again with the identical disposition returns the record; a different one is `ClaimNotUnfinished`. Passing the ticket to `DecisionPublicationV1::with_claim_settlement` settles the claim in the same transaction as the verdict records and events, so they can never be observed apart.
- `abandon_review_claim_v1` is the only way to stop waiting on an unfinished claim. It is never automatic, names the human or policy authority, and runs nothing. An abandoned id stays terminal; deliberately running the work again is a new `claim_id` (for example `<claim>/generation-2`), so every repeated provider call is a separately recorded decision.
- Reusing a `claim_id` with a different task, SHA or request digest is `ConflictingRetry`. `outcome_kind` is an opaque `[A-Z][A-Z0-9_]{0,63}` token that State never interprets.
- `SqliteStateRepository::find_review_claim(project, claim_id)` reads a claim.

### Durable seal and restart publication (`ReviewClaim` + `DecisionPublication`, schema v22)

Review's contract is: claim once, seal the exact observed result and package, then publish atomically. The seal is the smallest typed State boundary for the middle step; it is not a blob store.

```rust
// 1. claim (unchanged): Claimed(ticket) is the ONLY execution permission.
// 2. After the reviewer ran, BEFORE publishing anything:
let package = DecisionPublicationV1::new(project, publication_id, records, events, facts)?
    .with_claim_settlement(&ticket, ReviewClaimSettlementV1::completed("REJECT", Some("rev-1"), None)?)?;
let recovery = ReviewRecoveryDataV1::new("REVIEW_SEALED_V1", sealed_bytes, &sha256_hex_of_bytes)?; // State recomputes the digest
let request = ReviewSealRequestV1::new(&claim_request, &package, recovery)?;
let seal: ReviewSealRecordV1 = store.seal_review_claim_v1(admission /* Op::ReviewClaim */, &clock, &ticket, &request)?;
// 3. publish (live): store.publish_decision_package_v1(admission /* Op::DecisionPublication */, &clock, &package)?
// 3'. publish (after a restart, no ticket):
let claim = match store.claim_review_v1(..)? { Existing(r) => r, .. };            // CLAIMED: never execute
if let Some(seal) = store.reader().find_review_seal(project, claim_id)? {
    let package = rebuild_from(&seal.recovery_data)?.with_sealed_claim(&seal)?;      // Core validates, rebuilds, same stable ids
    store.publish_decision_package_v1(admission, &clock, &package)?;                 // refuses unless package.sha256() == seal.package_sha256
}
```

- **Seal contents.** project, claim id, task, subject SHA and request digest (verified equal to the stored claim, and enforced again by a schema trigger), `publication_id`, `package_sha256` (State's digest of the whole package, which includes the claim settlement), the settlement (`COMPLETED`/`REFUSED`, kind, ref, digest) and the exact recovery bytes (1 byte to 4 MiB) with a kind token and their SHA-256. The package identity and settlement are derived from the `DecisionPublicationV1`, never supplied separately. The seal's own identity digest covers everything except `sealed_at` and is re-verified on every read; any drift is `CorruptStore`.
- **Only the claimant seals.** `seal_review_claim_v1` needs the live, non-`Clone` ticket and the package must have been built with that ticket. After a restart nobody can create the initial seal, so an unsealed claim stays `CLAIMED`/uncertain. A foreign, stale or other-claim ticket is `WrongStore`/`InvalidShape`. The claim must be `CLAIMED`.
- **Reseal.** The identical request returns the stored seal and writes nothing, even after the claim completed. Different recovery bytes, package or disposition are `ConflictingRetry`. A publication id already sealed for another claim, or recorded with another digest, is `ConflictingRetry`.
- **Publish.** A claim that has a seal is completed only by publishing the sealed package: a different package is `SealMismatch`, a plain `settle_review_claim_v1` is `ClaimSealed`, and a restart package built with `with_sealed_claim` for a claim with no seal is `SealNotFound`. The settlement of a restart package comes from the seal, so the caller cannot choose another disposition. Records, events with receipts, availability facts, the publication identity and the claim settlement are still one transaction. Exact retries are replays with the same receipts.
- **Stable ids.** Event ids are part of the package digest. `PublicationEventV1::stable(proposal, publication_id, event_key)` derives the id from the publication id and a caller-owned stable key, so rebuilding the package after a retry or restart cannot mint different ids. (A random id minted per build would change the digest and the publication would be refused.)
- **Terminal and abandoned.** Completed/refused/abandoned claims accept no new seal and no different publication. Abandoning a sealed claim is allowed (it is the trusted decision to stop waiting); the seal stays as history and the sealed package can no longer be published (`ClaimNotUnfinished`).
- **Immutability.** A stored `REJECT` is never replaced: a later sealed `PASS` under the same review id seals fine but its publication fails with the original `AlreadyExists` and writes nothing. A sealed `POLICY_BLOCKED` publishes as a `REFUSED` claim and later callbacks see it.
- **What State does not claim.** An external reviewer execution and SQLite are not one transaction. A crash before the seal commits leaves the claim `CLAIMED` and uncertain forever (until an explicit abandon); State cannot know whether the reviewer ran. Content correctness is the trusted core's job: it validates the recovery bytes before sealing and again before rebuilding a package from them. State only proves the rebuilt package is byte-identical in digest to what was sealed.
- **Abandonment authority.** `abandon_review_claim_v1` still records an `authority_ref` string. State does not authenticate any person or policy behind it; the trusted core must decide who may abandon before it issues the admission.

### Remaining integration requirements

- Orchestration's `CoreStateWriter` also needs `seal_review_claim` (`Op::ReviewClaim`, consuming the live ticket) and a sealed-claim reader; see the seal section.
- Orchestration's `CoreStateWriter` needs forwarders: `publish_graph_mutation` (admit like `append_graph_mutation`, then `publish_graph_mutation_v1`), `publish_decision_package` (verify each event's content through the resolver as `persist_event_proposal` does, check the project is admitted, `admit(Op::DecisionPublication, project, None)`), and `claim_review` / `settle_review_claim` / `abandon_review_claim` (`Op::ReviewClaim`). Core's graph store should call the single graph operation instead of `append_node_result` plus `append_graph_mutation`.
- Routing, Registry, Availability and Review sinks should hand the Core one package per outcome instead of record-then-events, and Review should claim before routing. Event ids must be minted before the package is built; they are part of its digest.
- Provenance links are not part of a package; they stay separate immutable writes.
- Old stores may already hold records without their events (written by the one-write-per-transaction path). This change prevents new ones and does not repair existing ones.

### Repair: evidence and graph replay identity (schema v23)

Continuation checks every success receipt against its immutable manifest: complete ordered source coverage, type/class/expected digest and project/target identity, matching observed digest and a valid reread disposition. Mandatory sources require REREAD; an untouched CONSUMED source may be UNCHANGED and an undemanded REFERENCE may be DEFERRED. Contradictory success evidence counts as a failure for the existing State sequence/fence decision, including upgraded history. A fresh valid reread can recover; CHANGED remains sticky until succession. Startup uses the same decision.

Migration 23 adds immutable `graph_publication_identity`, recording a digest of the complete attachment set (reference, version and canonical result bytes) in the graph publication transaction. Attachment order is immaterial. Replay requires this identity plus the original mutation/snapshot/result checks. Graph v1 digests are unchanged. Historical mutations without this identity refuse retry with ConflictingRetry: membership cannot be reconstructed from unrelated result rows. The migration leaves historical records untouched and creates no inferred identities.

## Bundled SQLite security baseline

The root exact pin is `rusqlite =0.41.0` with `bundled` retained; Cargo.lock resolves `libsqlite3-sys 0.39.1` (SQLite 3.54.1). This replaces the bundled version reported by [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html). `tests::linked_sqlite_meets_the_bundled_security_floor` reads `sqlite_version()` and `sqlite_source_id()` from a real State connection and requires at least SQLite 3.54.0. Migration SQL, schema verification and immutability rules are unchanged. Independent successor review and fresh applicable security qualification are required for the changed dependency graph.
