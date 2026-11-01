# Orchestration graph: Wave1 behavior

In-memory graph foundation in `receipts-orchestration` (`src/core/**`). Nothing here persists to State, dispatches work or issues authority.

## Apply-v1 (`graph::mutation_apply`)

`apply_graph_mutation_v1(envelope, results, mutation, scope, interface)` executes a frozen `GraphMutation` under the explicit apply-v1 profile:

| op | target | detail |
|---|---|---|
| `ADD_NODE` | `node_id` | `{node: GraphNode}` (new nodes: `PLANNED`, `HUMAN_REQUIRED` or `LOCKED_REQUIRES_PRO`; no `result_ref`) |
| `ADD_EDGE` | `edge_id` | `{edge: GraphEdge}` |
| `SET_NODE_STATE` | `node_id` | `{from, to, cause: {kind, ...}}`; `from` must equal the current state |
| `ATTACH_RESULT` | `node_id` | `{result_ref, result: GraphNodeResult}`; one immutable slot per node |
| `CANCEL_NODE` | `node_id` | absent or `{}` |
| `EXPAND_REPAIR` | retained `REJECTED` `REVIEW` `node_id` | `{nodes, edges}`: one fresh versioned `REPAIR`, one distinct fresh `REVIEW`, the review's `ON_REJECT` edge to the repair and the repair's precedence edge to the new review |

Each outcome returns the next envelope and a `GraphResultLedgerV1` holding the attached results and the admitted attempt identities. Pass both to the next call.

Other schema-valid detail is wire data, not executable. Target identities must agree with the detail, and missing, unrelated or duplicate keys refuse.

What a call checks:
- It runs the existing preflight (target graph, actor role, parent version).
- It checks the caller scope: project, graph, version, actor role and role id. The exact decimal successor is checked too.
- It applies the ordered batch to a private copy.
- It computes the v1 child digest once and compares it with any supplied digest.
- Any refusal returns an error and leaves the inputs unchanged.

Authority comes from `GraphMutationScopeV1`, not from `mutation.actor`:
- `RUNTIME_A2` is limited to its own workstream, for nodes and for both endpoints of an edge.
- `RUNTIME_A1` and `USER` may make global amendments. Only they can accept or integrate, and only `USER` can resolve a human gate.
- `GOAL_EVALUATOR` may only append nodes, plus edges into nodes it appended.
- A3, A4 and the genesis compiler have no successor scope.

This is checked policy over admitted facts. It is not an authenticated production issuer.

## States (`graph::node_state_machine`)

`node_transition_rule` answers every one of the 225 ordered pairs. A pair is either an edge that needs one cause kind (46 edges) or a refusal with a named reason: self-transition, terminal state, verdict revision, locked gate, or not a lifecycle edge. The six older bounded validators stay public and agree with it.

Apply verifies each cause against the graph and the retained ledger:
- **Readiness:** checked through TaskDag.
- **Review verdicts:** a work node (`TASK`, `IMPLEMENTATION`, `REPAIR`, any kind not self-judging) is judged by independent review evidence, not by its own result. See *Review evidence* below. A `REVIEW` node, a deterministic check and the other self-judging kinds are judged by their own attached result, whose outcome must match the verdict.
- **Attempt identity:** `DISPATCH` admits a never-used `attempt_id` for the exact project, graph and node, with the node's known code SHA, and the ledger keeps both. `START`, `SUBMIT` and `ATTACH_RESULT` (its `result.attempt_id`) must name that node's current attempt, in the same batch or in later calls. A missing, different or reused identity refuses; a result's own claim never creates one. A ledger that never saw the dispatch (for example `GraphResultLedgerV1::new()` with a node already `RUNNING`) cannot start, submit or attach for it. A resumed node needs a new attempt; the old one stays in the history and cannot be reused.
- **Repair provenance:** `EXPAND_REPAIR` takes a retained `REJECTED` `REVIEW` and a fresh round of exactly one `REPAIR` and one `REVIEW`. The repair's `parent_node_id` is the one `TASK`/`IMPLEMENTATION`/`REPAIR` node with a precedence edge into the rejected review (the work it judged), not the review. The review's `ON_REJECT` edge goes to the repair, and the repair has a precedence edge to the new review, whose only execution subject is that repair. New nodes must be `PLANNED`, created in the successor version and share one workstream with the subject; supplied `attempt_number`s must match each other and exceed the subject's and old review's. A second expansion of the same review, missing or ambiguous subject linkage, precedence cycles and the old review's reuse all refuse. `REPAIR_ENTRY` checks the same provenance. The old review, its result and its attempts stay as they were.
- **Dependencies:** resuming (`BLOCKED`/`HUMAN_REQUIRED → READY`), `READY → ADMITTED` and `ADMITTED → DISPATCHED` recheck every incoming precedence dependency against the graph as it is at that operation (earlier operations of the batch included), as `PLANNED → READY` does. A resolution or admission reference satisfies none. Unmet or stale evidence refuses as `EvidenceRefused`; absent evidence as `EvidenceUnavailable`. `REQUIRES_INTERFACE` evidence is bound to the current graph version, so each of these steps needs a verifier that can vouch for it.
- **Blocked and human states:** need a recorded reason. They resume only at `READY`, and never when the node already holds an attached result. `HUMAN_REQUIRED` cannot be moved to `BLOCKED`.
- **`LOCKED_REQUIRES_PRO → READY`:** always *unavailable* in Wave1, because no verified capability-admission source exists yet.

`ACCEPTED`, `INTEGRATED` and `CANCELLED` are never reopened. Rejected or passed `REVIEW` verdicts are retained and cannot be cancelled.

## Review evidence and code SHAs

**Code or non-code.** An omitted `code_sha` says nothing: not on a node, not on a result, not on a review record. Every attempt is code or non-code only because trusted core says so, and until then results, verdicts and acceptance for it are *unavailable*, never assumed non-code. No node kind or check command is exempt.
- *Code:* the node's known `code_sha` is bound to the attempt at `DISPATCH`, or trusted core admits the commit with `GraphResultLedgerV1::admit_attempt_code_sha`. The result must carry exactly that SHA and so must the review record. `START`, `SUBMIT` and `ATTACH_RESULT` refuse if the node's SHA is no longer the bound one, in the same batch or across calls.
- *Non-code:* trusted core admits it with `GraphResultLedgerV1::admit_attempt_non_code`, for the exact project, graph, node and current attempt. The result and the review record then carry no SHA, and a SHA claim is refused. No SHA is invented.
- Admission is an owner-local, caller-trusted fact like the scope and the attempt id, not an authenticated issuer. A worker's mutation, result, `review_id` or review record cannot make it. It is made once per attempt: a code attempt (known or admitted) is never downgraded to non-code, non-code never takes a SHA afterwards, and a different SHA refuses. A refused admission leaves the judged result and review as they were. Admission happens between calls, so a SHA-less node is dispatched and started in one call and submits, attaches and is judged in a later one. A new attempt (a resumed node) starts unclassified again. A failed batch changes neither the ledger nor the classification.

**Verdicts.** Self-judging kinds are `REVIEW`, `DETERMINISTIC_CHECK`, `GOAL`, `WORKSTREAM`, `GOAL_EVALUATION`, `ROUTING`, `INTEGRATION` and `HUMAN_GATE`; every other kind, including an unknown one, is a work node. Either way the node's current attempt must already be classified as code or non-code (above); an unclassified one is *unavailable*. `SET_NODE_STATE AWAITING_REVIEW → PASSED | REJECTED` on a work node needs `{kind: REVIEW_VERDICT, result_node_id, review_id}` and review evidence from `apply_graph_mutation_with_evidence_v1` (`ApplyEvidenceV1.review`). The worker's attached result must exist, but neither its `PASS` nor a `review_id` it names proves anything. `ReviewEvidenceVerifier` reads a trusted `ReviewEvidenceSource` and admits the record only if it is bound to this project, graph, subject node, the node's **current admitted attempt**, its bound SHA (or none, only for an attempt admitted as non-code), its `attempt_number` round (or none) and the verdict's outcome; was admitted by the source; and has the content the source stores (SHA-256 recomputed). Each failure is a typed `ReviewEvidenceRefusal` (`GraphOperationError::Review`). A missing record, content or source (including no verifier at all, as with `apply_graph_mutation_v1`) is *unavailable*, never assumed. A review identity judges one attempt once: reuse, an old round's review and a review of another attempt, commit or round refuse. The review identity may not be the subject node or its attempt.

A node already judged `REJECTED` is judged again through its newest repair: the verdict cites the repair, whose own admitted review (and matching state) is the evidence. `PASSED → ACCEPTED` needs the retained `PASS` review of the judged attempt (the newest repair's after a repair), still bound to the same SHA and a classified attempt, in addition to the A1/USER actor rule. Graph data that merely says `PASSED` has no review history and cannot be accepted.

**Not circular.** A graph `REVIEW` node that waits (through `REQUIRES_ACCEPTED` or `REQUIRES_INTEGRATED` edges) for the subject's acceptance can never gate that acceptance, so naming it as the review refuses. A graph review that does not wait for the subject is usable once it holds the matching verdict. Its record is still required from the source.

**Supported path.** `fixtures/graphs/03_repair_expansion.json` is structural: it proves the expansion shape and the absence of a precedence cycle, not that a review waiting for the acceptance it gates could run. The path that runs end to end (see `mutation_review_tests::repair_review_round_completes_without_circular_acceptance`) is: an already accepted source artifact `i1`; its post-acceptance review `r1` runs and rejects (its own result, its own attempt, admitted by core as non-code); `EXPAND_REPAIR` adds repair `i2` (parent `i1`) and fresh review `r2`; `i2` runs, trusted core binds its commit, an independent review from the source judges that attempt, A1 accepts it; only then can `r2` run, as the next round, and `next` follows `r2`'s acceptance. `REPAIR_ENTRY` for an aggregate whose review waits on its acceptance is reachable only from seeded graph data; its tests say so.

**No production source.** Wave1 has no production review-evidence source, issuer or dispatcher. With none supplied every verdict and acceptance that needs one is refused as unavailable. The directory-backed source in the tests proves what the verifier checks, not who produced the review.

## Readiness (`dag::TaskDag`)

TaskDag is a read-only view with three queries: `ready_nodes`, `task_state` and `dependency_status`.

| Edge kind | Satisfied when |
|---|---|
| `REQUIRES_ACCEPTED` | the predecessor is `ACCEPTED` or `INTEGRATED` |
| `REQUIRES_INTEGRATED` | the predecessor is `INTEGRATED` |
| `REQUIRES_INTERFACE` | `InterfaceEvidenceVerifier` admits evidence from a trusted `InterfaceArtifactSource` |

The verifier checks the graph, version, edge, source, consumer, accepted provenance and predecessor code SHA. It also recomputes the SHA-256 of the content. With no source, the dependency is *unavailable*: such nodes are listed in `awaiting_evidence`, never in `ready`.

Control edges never gate readiness. Human-gate nodes, and nodes in any state other than `PLANNED` or `READY`, are never ready.

## Wire codecs (`wire`)

`ExecutionGraphWireV1`, `GraphNodeWireV1`, `GraphMutationWireV1`, `GraphSnapshotWireV1` and `GraphNodeResultWireV1` check the frozen schema shape. They keep the parsed JSON verbatim:
- exact number lexemes, with no f64 step;
- the difference between a missing field and an explicit null;
- open objects.

`GraphEdge` keeps its PR198 codec. Converting to operational types can fail with `Unsupported(field)`, for example for `graph_version: 1.0`, a timestamp that does not parse, or an unknown snapshot state. The wire value is not changed when that happens. Decoding grants no authority.

## Not in Wave1

- State persistence wiring (OR-W2-5).
- The remaining six OR-WIRE schemas.
- The scheduler and admission.
- A capability-admission verifier.
- Any production interface-artifact source or authenticated issuer.
