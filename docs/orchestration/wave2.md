# Orchestration: Wave2 behavior

All of this is library code in `receipts-orchestration` (`src/core/**`). Nothing here dispatches, routes or runs a worker; the Wave3 goal loop composes these pieces.

## Scheduling (`scheduler::schedule_dispatch_v1`)

One deterministic round over one graph version. Each candidate gets exactly one decision: `Admitted`, `Denied` (D11 DENY), `Deferred` (a named axis) or `Refused`.

Checks, in order. The first failing one is the named axis:

1. **Refusals.** Duplicate candidate, unknown node, node without workstream, empty write scope, admission request bound to another node or graph, request bound to a context epoch other than the trusted current one (`StaleContextEpoch`), node not ready per `TaskDag` (unavailable interface evidence included).
2. **D11.** `derive_dispatch_admission`. Typed evidence refusals stay refusals. DENY means zero dispatch. Missing or stale evidence never becomes ALLOW.
3. **Budgets.** Remaining worker starts, then cost. Under a cost cap, an unknown estimate defers; it is never treated as free.
4. **Concurrency.** Global, then the node's workstream, then the D11-selected provider. A workstream or provider with no configured limit defers.
5. **Write collision.** Against every in-flight writer, then every writer admitted earlier in the round, through `WriteScopePort`. Only proven disjointness admits. An overlap defers with its witness; an undetermined answer (invalid pattern, matcher resource limit) defers.

Scheduling is pure. A round reads one snapshot and returns decisions; it is not a durable dispatch reservation and does not account actual spend. Two rounds over the same snapshot can admit the same node, and the budget is not carried between rounds. The composition must record the admission, count it as in flight, and update remaining budget before the next round. Nothing here is cross-round safe on its own.

Priority among candidates that pass step 1: longest downstream precedence chain, then most transitive dependents, then oldest `ready_sequence`, then highest `user_priority`, then node id. Capacity used by an earlier admission is gone for later ones, so a full limit defers everything behind it (backpressure) instead of failing it.

## Collision resolution (`scheduler::resolve_write_collision_v1`)

A collision must exist first: proven-disjoint scopes refuse with `NoCollision`.

- `Defer`: no change.
- `Serialize { first, second }`: one `REQUIRES_ACCEPTED` edge from first to second.
- `CanonicalWriter { ownership }`: the writer is the workstream that provably owns the overlap witness, when the other provably does not. No witness, both or neither owning, or undecidable ownership refuses.

The edge is a real `GraphMutation` executed by apply-v1 under the caller's `GraphMutationScopeV1`. So the scope decides who may add it (`RUNTIME_A2` only within its workstream), and cycles refuse. The caller persists the returned mutation.

## Capsules (`capsules::build_task_capsule_v1`, `build_repair_capsule_v1`)

Graph-owned fields come from the graph and trusted context, never the requester:

- task id, workstream, attempt number;
- dependencies (incoming precedence edges);
- required capabilities, branch `runtime-a3/<id>`, worktree;
- the context epoch, read as State's latest for the project. The request must have been prepared against that same epoch.

A capsule is refused, never built, when:

- the node is not ready, or is of the wrong kind;
- a write pattern starts with a wildcard;
- a write pattern is not proven to lie inside the node's own adopted ownership;
- a write pattern overlaps another workstream's ownership or a forbidden path;
- the matcher cannot decide a write pattern;
- any text the capsule would emit looks credential-shaped.

Other workstreams' owned paths are added to `forbidden_write_paths`. The port only answers overlap, which cannot show containment: with own `src/core/**`, another owner `docs/**` and allowed `src/*/safe/**`, the pattern overlaps its owner and is disjoint from the other owner, yet it reaches `src/unowned/safe/x.rs`. So a pattern is built only when `capsules::containment` proves it lies inside the node's own ownership, and the port then proves it disjoint from every other ownership and forbidden path.

The containment proof reuses the Workspace grammar (`WritePathPattern::parse` decides validity: whole-component `**`, `*` inside a component, no `?[]{}`). It is structural and sufficient, not complete. A union counts only when one owned pattern contains the whole pattern, and a `*` in the requested pattern is absorbed only by a `*` in the owner. Anything not shown contained is `OutsideWorkstream`; an invalid or oversized (over 1 KiB) pattern is `WriteScopeUndetermined`. Tests check every claimed containment against the real matcher over a bounded exhaustive path set. A repair must also pass this check; being a subset of the original's patterns is not enough on its own.

The credential check is a heuristic (token prefixes, key headers, `key=value` secrets, long mixed-class high-entropy tokens). Passing it does not prove a value is secret-free. The structural rule is that no capsule field is a credential slot.

The check covers every free-text value either capsule emits, whatever its source: requester text, values derived from the graph (task, workstream and dependency ids, branch, required capabilities), values read from the original capsule (a repair re-screens its objective, criteria and traps, because `TaskCapsule` is a public type and its builder is not proof), reference digests, criterion and finding ids, and the final `forbidden_write_paths` (requester paths, original traps and every other workstream's owned paths). Numeric, enum, boolean, timestamp and lowercase-hex fields are validated structurally by the capsule constructors and cannot hold a recognized token shape.

The screen runs before any scope check. A refusal therefore does not echo a credential-shaped pattern: it is `CredentialShapedValue` naming the field family, not the value. Patterns, matcher witnesses and State messages that do reach a refusal payload (for example an overlap witness the screen could not see) are replaced with `<withheld: credential-shaped>` when they look credential-shaped, so `Display` and `Debug` of `CapsuleBuildError` carry no recognized token. Ordinary scope refusals keep their typed payloads. A credential-shaped value now takes precedence over a scope refusal for the same request.

A repair (N4) is built only for a ready `REPAIR` node such that:

- its `parent_node_id` chain reaches the original task through repairs;
- a `REJECTED` `REVIEW` that judged its parent hands off to it through `ON_REJECT`;
- its attempt number is between 2 and the bound.

It keeps the original's objective, criteria, workstream, traps and floor (never lower). Its write paths must be a subset of the original's patterns, and every listed blocking finding must be marked blocking. `current_sha` is the judged node's bound code SHA and must agree with the attempt the ledger admitted.

## Compiler (`compiler::compile_graph_v1`)

A pure, deterministic function. The decomposition proposal is input data, so no model is called. The spec is resolved through the composition's admitted-content resolver and kept byte-for-byte. `compiled_from` is `[spec_ref, "sha256:<hex of exact bytes>"]`.

The output is a version-1 envelope plus the State genesis with the admitted compiler, the project's admitted source and `INITIAL_COMPILATION`.

**Canonical shape.**

- Node ids come from the proposal.
- Edges are numbered `e1..eN` in topological order of their target, then their source, with ties broken by node id.
- `HUMAN_GATE` nodes start `HUMAN_REQUIRED`.
- A node with a `LOCKED_REQUIRES_PRO` capability starts locked, with a fixed visible reason.
- A capability with no supplied outcome, or an unknown or expired one, refuses. Nothing is inferred from a live catalog.

Fixtures `01`, `02`, `04`, `06` and `07` compile to their exact JSON, `compiled_from` aside.

**Coherence.**

- Identical owned scopes are merged into the smallest workstream id ("too granular").
- A workstream with no owned paths, or a node naming an unproposed workstream, refuses ("no ownership").
- Overlapping or undecidable owned scopes refuse ("re-cut the boundary").

**Criteria.** At least one acceptance criterion is required. A `DERIVED` criterion, or a deterministic one with no check command, needs explicit user confirmation; the refusal lists each one.

## Persistence (`graph::store`)

All writes go through `CoreStateWriter`, the core issuer.

- `persist_compiled_graph_v1` stores the genesis.
- `persist_mutation_v1` stores one apply-v1 outcome as the next version through State's single atomic graph publication. `persist_mutation_outcome_v1` does the same and also says whether State wrote it (`Applied`) or recognized an exact retry (`Replayed`).

One State transaction writes the newly attached results, the snapshot, the mutation history, the seal, the trusted-time watermark and the head move. A failure (for example a regressing trusted clock) leaves none of them: no orphan result rows, so changed content under the same reference is never blocked by an earlier failure.

Before anything is written, the stored head must equal the outcome's parent in both version and v1 digest. A stale or concurrent writer gets a retryable `Conflict` and nothing changes. A mutation id already in the history is checked first:

- **Same stored mutation:** an exact retry. The head check is skipped (the head has moved on) and State decides. It replays only when the recorded attachment membership (every reference, version and result body, order immaterial) matches; then nothing is written.
- **Different content under that id:** `MutationExists`, nothing written.
- **Different attachment set, or a legacy mutation with no recorded membership:** `ConflictingRetry`. Membership is never inferred from stored result rows, so a mutation written by the older non-atomic writers refuses an exact retry. Repairing such history needs an explicit disposition.

A result row already stored under a reference must be exactly the attached result (State adopts an identical orphan left by the old writer; any other row is `ResultConflict` and nothing else moves). A missing result in the outcome's ledger is `MissingResult`, refused before State.

The old per-result and per-mutation writers (`append_node_result`, `append_graph_mutation`) stay on `CoreStateWriter` only as documented legacy paths. New work does not use them.

Reads (`load_current_graph_v1`, `load_graph_version_v1`, `load_graph_history_v1`, `load_node_results_v1`) recompute and compare the v1 digest. On reload, `created_at` is State's trusted time. The digest profile stays v1 and no historical version is rewritten.

Not persisted: the in-memory `GraphResultLedgerV1` attempt and review facts. Rehydrating them is Wave3 resume.

## State integration (`orchestration`)

Everything below is a method of `CoreStateWriter`. Core builds each State package itself from inputs it validates; it never forwards an opaque State package. Admissions are minted from the actual project, role and operation context, never by State and never from a payload value. A project the composition did not admit is `ProjectNotAdmitted` and a stored role or claim of an unknown subject is refused before State.

**Recovery.** `recover_startup(clock, project, authority)`, `rebind_role(clock, request)` and `succeed_changed_context_sources(clock, request)` forward to State's recovery operations. The project of a role comes from the stored role. State's refusals (wrong expected binding, unknown attempt, repeated succession, malformed authority) pass through as `CoreStateWriteError::State` with nothing written.

**Decision publication.** `publish_decision_package(clock, content, publication_key, &CorePublicationInputV1)` takes typed records, events (each with a caller-owned `event_key`) and availability facts. Core checks the project is admitted, that every item belongs to it, and resolves and hashes EVERY event payload through `CoreContentResolverV1` before building anything. The publication id is `publication_id_v1(project, key)` and each event id derives from it and the event key, so a retry or a rebuild after a restart is the same package. One State transaction writes the records, events with receipts, availability facts and the publication identity. An exact retry is `replayed = true` with the recorded receipts; the same key with other content is `ConflictingRetry`; a record or event identity that already exists outside a recorded publication refuses and rolls back; a stored `REJECT` is never replaced.

**Review claim → seal → publish.** A review is one `CoreReviewUnitV1` (project, task, optional 40-hex SHA, 64-hex request digest, `generation`). Its claim id is a pure function of those fields.

1. `claim_review` returns `Acquired` exactly once per unit. The live State ticket stays inside the writer (not `Clone`, gone after a restart). Anything else is never executed: `InFlight` (claimed, no seal: the earlier run may have reached the reviewer), `Sealed` (publish from it, also for a finished claim, where publication replays), `Terminal` (finished without a seal) or `Abandoned`.
2. `seal_review(clock, content, codec, unit, recovery_bytes)` seals the exact bytes BEFORE anything is published. Only the writer that acquired the claim can seal. The bytes are decoded by the composition's `CoreRecoveryCodecV1`, which returns the typed package inputs and the claim disposition. Core checks the project, resolves and hashes every payload, builds the State package, and seals only if all of it holds. State stores the bytes, their digest and the package identity. An identical reseal returns the stored seal; other bytes or another package are `ConflictingRetry`.
3. `publish_sealed_review(clock, content, codec, unit)` reads the seal from State, decodes the sealed bytes again, requires the same task, subject, request, publication id, kind and disposition, rebuilds the package and requires its digest to equal the sealed one. It then publishes records, events, availability facts and the claim settlement in one transaction. It takes no package from the caller, so the same call finishes a live run and a run recovered after a restart, and it runs no reviewer.

`sealed_review(unit)` reads a seal and its claim without writing. `settle_review_refusal` settles the live claim as a terminal refusal with nothing to publish (for example a policy block before any execution); it refuses a completed settlement, and State refuses plain settlement once a seal exists, so plain settlement can never discard review records or events. `abandon_review_claim(clock, unit, grant)` ends an unfinished claim without re-running anything. Re-running needs a new `generation`.

**Abandonment authority.** `CoreAdmissionIdentity::with_abandonment_authority(project, authority_ref)` admits the authority, and `abandonment_grant(project, claim_id)` mints a single-use `CoreAbandonmentGrantV1` only from that admitted identity object (or a clone of it). The writer refuses a grant minted by another identity object (even naming the same authority), for another claim or project, or when its own identity admits no authority. The authority recorded on the claim is the admitted one, never a caller string. This is an object capability held by the trusted composition, not an authentication service: it does not identify a person.

**Recovery codec.** The sealed bytes belong to the reviewer, which Core must not depend on, so the composition supplies `CoreRecoveryCodecV1 { kind, decode }`. `decode` returns typed values (never a validity flag) and must reject corrupt or foreign bytes; Core still checks the project and the rebuilt package digest.

Limits:

- A crash between an external execution and the seal commit is uncertain by design; nothing re-runs it.
- The ticket map is in memory. A claim whose writer is gone can be published from its seal but not sealed or settled.
- Event payloads must be resolvable by the composition's resolver when a sealed package is rebuilt; if not, publication refuses (`ContentUnavailable`) and the seal stays for a later retry.
- Direct SQL claim transitions do not carry the sealed-publication guarantee. Same-process and same-user access, separate-principal deployment and bundled SQLite assurance remain outside this package.

## Codecs (`wire`)

`TaskCapsuleWireV1`, `RepairCapsuleWireV1`, `DispatchAdmissionDecisionWireV1`, `FeatureAdmissionDecisionWireV1`, `FeatureCapabilitySetWireV1` and `GoalEvaluationWireV1` follow the graph codec contract. They check the exact schema shape and keep the JSON verbatim. Conversion to an operational type is fallible, and anything outside the operational domain is `Unsupported(field)`:

- Integers stored as `i64` need a canonical lexeme in range. Lexical integers (repair capsule, catalog version, evaluation epoch) keep any magnitude. `1.0`, `1e0` and `-0` refuse.
- `TaskCapsule.verification_plan` is a plan. Each entry projects to `{source: WORKER_EXECUTION, command, exit_code: <expected>, code_sha: <start_sha>}`. Anything else refuses, including executed-check fields.
- `cost_policy.max_cost` converts only when the `f64` holds the wire number exactly. `9007199254740993` (would round), `1e-400` (would underflow) and `1e400` refuse; `2.5`, `9007199254740992` and `0.1` convert. A very long spelling of an exactly representable value can refuse, which is the safe side.
- A capsule with no required capability, or with an empty verification plan (allowed for DOCS and INVESTIGATION operationally, but the schema needs one entry), cannot be encoded. The encoder also checks its own output against the schema rules, so a successful encode always decodes.
- `verification_plan` entries are expectations, never execution receipts. `exit_code` is the expected exit and `code_sha` is the start SHA; no result, timing or output evidence exists in a plan, and nothing may promote an entry into executed evidence.
- Wire wrappers keep absence versus null. Operational conversion is not lossless for the selected provider and runtime: an absent and a null value both become none, and re-encoding emits null. Do not describe that round trip as lossless. (`upgrade_info` and `max_cost` keep the distinction.)
- `RepairCapsule` conversion takes the parent's quality floor.
- `GoalEvaluation` converts only its deterministic projection. A semantic layer, gaps or convergence refuses.

Tests check accept/reject agreement with the frozen JSON Schemas (via `jsonschema`) on every positive and negative sample.

## Ports and limits

The public consumer-owned ports are `WriteScopePort` (bound to the real Workspace matcher only in `receipts-e2e`) and `CoreContentResolverV1` (spec and event content). They are frozen for later composition.

Router, Executor and Review ports are not added yet: their only caller is the Wave3 goal loop. `CoreRecoveryCodecV1` is the one reviewer-owned composition port, supplied by the trusted composition.
