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

Apply verifies each cause against the graph:
- **Readiness:** checked through TaskDag.
- **Review verdicts:** the subject's own attached result, or its newest repair child's. A result from a rejected round is never reused.
- **Attempt identity:** `DISPATCH` admits a never-used `attempt_id` for the exact project, graph and node and the ledger keeps it. `START`, `SUBMIT` and `ATTACH_RESULT` (its `result.attempt_id`) must name that node's current attempt, in the same batch or in later calls. A missing, different or reused identity refuses; a result's own claim never creates one. A ledger that never saw the dispatch (for example `GraphResultLedgerV1::new()` with a node already `RUNNING`) cannot start, submit or attach for it. A resumed node needs a new attempt; the old one stays in the history and cannot be reused.
- **Repair provenance:** `EXPAND_REPAIR` takes a retained `REJECTED` `REVIEW` and a fresh round of exactly one `REPAIR` and one `REVIEW`. The repair's `parent_node_id` is the one `TASK`/`IMPLEMENTATION`/`REPAIR` node with a precedence edge into the rejected review (the work it judged), not the review. The review's `ON_REJECT` edge goes to the repair, and the repair has a precedence edge to the new review, whose only execution subject is that repair. New nodes must be `PLANNED`, created in the successor version and share one workstream with the subject; supplied `attempt_number`s must match each other and exceed the subject's and old review's. A second expansion of the same review, missing or ambiguous subject linkage, precedence cycles and the old review's reuse all refuse. `REPAIR_ENTRY` checks the same provenance. The old review, its result and its attempts stay as they were.
- **Dependencies:** resuming (`BLOCKED`/`HUMAN_REQUIRED → READY`), `READY → ADMITTED` and `ADMITTED → DISPATCHED` recheck every incoming precedence dependency against the graph as it is at that operation (earlier operations of the batch included), as `PLANNED → READY` does. A resolution or admission reference satisfies none. Unmet or stale evidence refuses as `EvidenceRefused`; absent evidence as `EvidenceUnavailable`. `REQUIRES_INTERFACE` evidence is bound to the current graph version, so each of these steps needs a verifier that can vouch for it.
- **Blocked and human states:** need a recorded reason. They resume only at `READY`, and never when the node already holds an attached result. `HUMAN_REQUIRED` cannot be moved to `BLOCKED`.
- **`LOCKED_REQUIRES_PRO → READY`:** always *unavailable* in Wave1, because no verified capability-admission source exists yet.

`ACCEPTED`, `INTEGRATED` and `CANCELLED` are never reopened. Rejected or passed `REVIEW` verdicts are retained and cannot be cancelled.

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
