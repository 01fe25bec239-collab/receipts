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
- a write pattern does not overlap the node's own adopted ownership;
- a write pattern overlaps another workstream's ownership or a forbidden path;
- the matcher cannot decide a write pattern;
- any text looks credential-shaped.

Other workstreams' owned paths are added to `forbidden_write_paths`. The port only answers overlap, so "inside the workstream" means: overlaps its own ownership and is proven disjoint from every other adopted scope. Full containment would need a subset query the shared matcher does not have.

The credential check is a heuristic (token prefixes, key headers, `key=value` secrets, long mixed-class high-entropy tokens). Passing it does not prove a value is secret-free. The structural rule is that no capsule field is a credential slot.

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
- `persist_mutation_v1` stores one apply-v1 outcome as the next version, moved by State's parent CAS.

Before anything is written, the stored head must equal the outcome's parent in both version and v1 digest, and the mutation id must be unused. A stale or concurrent writer gets a retryable `Conflict` and nothing changes.

Newly attached results are stored first, at the parent version, because State requires them before the snapshot. If the snapshot append then fails (for example, a regressing trusted clock), the head does not move. The result row stays as immutable, unreferenced evidence; an identical retry reuses it, and different content under the same reference refuses.

Reads (`load_current_graph_v1`, `load_graph_version_v1`, `load_graph_history_v1`, `load_node_results_v1`) recompute and compare the v1 digest. On reload, `created_at` is State's trusted time. The digest profile stays v1 and no historical version is rewritten.

Not persisted: the in-memory `GraphResultLedgerV1` attempt and review facts. Rehydrating them is Wave3 resume.

## Codecs (`wire`)

`TaskCapsuleWireV1`, `RepairCapsuleWireV1`, `DispatchAdmissionDecisionWireV1`, `FeatureAdmissionDecisionWireV1`, `FeatureCapabilitySetWireV1` and `GoalEvaluationWireV1` follow the graph codec contract. They check the exact schema shape and keep the JSON verbatim. Conversion to an operational type is fallible, and anything outside the operational domain is `Unsupported(field)`:

- Integers stored as `i64` need a canonical lexeme in range. Lexical integers (repair capsule, catalog version, evaluation epoch) keep any magnitude. `1.0`, `1e0` and `-0` refuse.
- `TaskCapsule.verification_plan` is a plan. Each entry projects to `{source: WORKER_EXECUTION, command, exit_code: <expected>, code_sha: <start_sha>}`. Anything else refuses, including executed-check fields.
- A capsule with no required capability cannot be encoded: the schema needs at least one.
- An absent or null selected provider/runtime reads as none, and none encodes as null.
- `RepairCapsule` conversion takes the parent's quality floor.
- `GoalEvaluation` converts only its deterministic projection. A semantic layer, gaps or convergence refuses.

Tests check accept/reject agreement with the frozen JSON Schemas (via `jsonschema`) on every positive and negative sample.

## Ports and limits

The public consumer-owned ports are `WriteScopePort` (bound to the real Workspace matcher only in `receipts-e2e`) and `CoreContentResolverV1` (spec and event content). They are frozen for later composition.

Router, Executor and Review ports are not added yet: their only caller is the Wave3 goal loop.
