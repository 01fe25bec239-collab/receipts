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
| `EXPAND_REPAIR` | rejected source `node_id` | `{nodes, edges}`: fresh versioned `REPAIR` node(s) plus an `ON_REJECT` control edge from the source |

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
- **Repair entry:** needs an expanded child that has an `ON_REJECT` edge.
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
