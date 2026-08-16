<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# GRAPH_MUTATION_PROTOCOL

## Rule

> Every load-bearing change to the plan produces an identifiable new graph version with a recorded actor and reason.

The Global Goal Evaluator may **append** work. It may not silently rewrite the plan. That distinction is the whole point of versioning the graph: a user must be able to ask "why is this node here?" and get an answer.

## Mutation record

`mutation_id` · `graph_id` · `parent_version` · `resulting_version` · `actor{role, role_id}` · `reason` · `operations[]` · `resulting_digest` · `created_at`

Operations: `ADD_NODE` · `ADD_EDGE` · `SET_NODE_STATE` · `ATTACH_RESULT` · `CANCEL_NODE` · `EXPAND_REPAIR`.

Notably absent: `DELETE_NODE` and `REWRITE_EDGE`. **The graph is append-only in structure.** A node that should not have existed is `CANCEL_NODE`-ed, not deleted — cancellation is history, deletion is amnesia.

## Repair expands, it does not loop

A rejected review must not create a cyclic precedence edge back to the implementation. Instead the graph **expands**:

```
Implementation-1 ──PRECEDENCE──▶ Review-1
                                    │ CONTROL: ON_REJECT
                                    ▼
                              Repair-2  ──PRECEDENCE──▶ Review-2 ──▶ Integration
```

Precedence edges point forward only. `ON_REJECT` is a **control** edge — it records why the new nodes exist; it is not a scheduling prerequisite. Fixture `03_repair_expansion.json` proves this: precedence cycles = 0 despite the loop being conceptually present.

This is the graph-level version of the V1.2.3 rule that a repair gets a new task revision rather than reopening the parent. Every rejected attempt and its review remain permanently in the graph.

## Who may mutate

| Actor | Permitted |
|---|---|
| `GRAPH_COMPILER` | Initial construction |
| `RUNTIME_A1` | Add workstreams, resolve cross-workstream structure, integrate |
| `RUNTIME_A2` | Expand its own workstream, expand repairs within bounds |
| `GOAL_EVALUATOR` | **Append** gap-closing work only |
| `USER` | Explicit amendment |
| `RUNTIME_A3` / `RUNTIME_A4` | **Never.** A3 raises a `SubtaskRequest`; A4 returns a verdict |

An implementer that could mutate the plan could widen its own scope. That is precisely the control the ephemeral-worker design exists to keep.

## Concurrency

Mutations are serialised by the single-writer state layer. A mutation carries `parent_version`; if the graph has advanced, the mutation is rejected and recomputed against the current version rather than blindly applied.

## Digest

Each version carries a content digest over its canonical node and edge set, so a resumed host can confirm it is looking at the same plan rather than trusting a version integer alone.
