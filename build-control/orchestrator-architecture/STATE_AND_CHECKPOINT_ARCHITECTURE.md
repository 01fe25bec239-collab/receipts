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

# STATE_AND_CHECKPOINT_ARCHITECTURE

## Requirement

Durable local state that survives host restart, Claude restart, Codex restart, provider failure, and A1/A2 executor replacement. Everything the product promises about long-horizon work depends on this layer.

## Store choice — FROZEN (V1.2, Q-03 resolved)

```
MVP_STATE_STORE = SQLite
```

Q-03 was open in V1.1. It is now **decided**, because an architecture cannot be called implementation-ready with an unresolved load-bearing persistence choice.

| Dimension | Frozen decision |
|---|---|
| Scope | **Single local project.** One database file per project. No shared or networked instance in MVP. |
| Transactions | **One transaction per orchestration action.** Append plus any dependent projection update commit together or not at all. |
| Journal mode | **WAL**, with an explicit `busy_timeout`. Readers must not block the single writer. |
| Concurrency boundary | **One writer process** (the orchestrator core). Multiple readers permitted. Concurrent hosts attach read-only; a single-active-binding lease governs write authority. |
| Migration boundary | **Versioned, forward-only migrations**, with the schema version recorded in the store and checked at startup. A version mismatch is a startup error, never a silent upgrade. |
| Replacement interface | All access is behind a **repository interface**. No SQL leaves the state layer. Swapping the backing store is a `BUILD-A2-STATE-CONTEXT` internal change with no effect on any other manager. |

**Why SQLite:** single file, zero administration, transactional, ubiquitous, and adequate for one developer's orchestration write volume. Postgres is right for `PRODUCT_TEAM_MODE` and wrong for a local tool that must install without running a service.

**What would reverse it:** sustained multi-writer contention from concurrent hosts, or a `PRODUCT_TEAM_MODE` requirement. The repository interface exists so that reversal costs one manager's work, not an architecture pass.

Rationale: single-file, zero-administration, transactional, well-supported everywhere, and adequate for the write volume of one developer's orchestration. Alternatives (embedded KV, Postgres) were considered; Postgres is right for `PRODUCT_TEAM_MODE` and wrong for a local tool that must install without a service.

The state layer is behind a repository interface so the backing store can change without touching the layers above — the concrete choice is deliberately reversible.

## Entities (§82)

```
project · hosts · host_sessions
logical_roles · executor_bindings
providers · models · runtime_adapters · model_capabilities · model_observations
goals · goal_evaluations
task_graph · workstreams · tasks · dependencies · task_attempts
workspaces · checkpoints
checks · evidence · reviews · findings · repair_attempts
integrations
context_manifests · context_epochs
quota_states · provider_events
decisions · events
```

Not overengineered: entities exist because a durable question is asked of them. Anything derivable is derived.

## Source-of-truth boundaries

| Fact | Authoritative | Never authoritative |
|---|---|---|
| Code content, SHAs, diffs | **Git repository** | State store copies (references only) |
| Role identity, ownership, bindings | **State store** | Any chat |
| DAG, task state | **State store** | Agent recollection |
| Evidence and reviews | **State store**, bound to SHA | Prose summaries |
| Model capability | **Registry** with provenance | Training memory |
| Availability | **Availability manager** (observed) | Assumption |
| Goal completion | **Goal evaluation record** | Renderer output |

The state store never duplicates git content. Duplicated content diverges, and then two systems disagree about what the code is.

## Append-only where it matters

Events, attempts, reviews, findings, routing decisions, provenance, and bindings are **append-only**. Task state is mutable but every transition is an event. Rejected attempts are never deleted (`A3_A4_REPAIR_LOOP.md`).

Append-only history is what makes an integration decision reconstructible after the fact (I-19).

## Transactions and crash safety

One transaction per orchestration action; WAL mode; a busy timeout for concurrent readers. The store must survive a kill mid-write, because the machine will be closed mid-goal.

Recovery on startup: detect orphaned attempts (expired lease, no handoff), capture workspace state before touching anything, mark roles unbound, and re-derive ready tasks from the DAG rather than trusting a cached view.

## Checkpoints

Attempt-level progress snapshots (`WORKSPACE_RECOVERY.md`): a record in the store plus a workspace snapshot. Frequency is time- or progress-based, bounded so checkpointing does not dominate execution.

## Integrity

Only the orchestrator core writes state. **No worker agent, no host adapter, and no LLM output path has a write path** (I-18). A worker that could edit the state store could mark its own work accepted, which would make every gate above it theatre. Enforced by process boundary and interface, not by instruction.

## Portability

The store is local, per-project, and inspectable. A developer can read their own orchestration history with standard tools — which is also how they can verify the system is telling them the truth.

## Graph and entitlement persistence (V1.3)

New relational tables: `graphs` · `graph_versions` · `graph_nodes` · `graph_edges` · `graph_mutations` · `graph_node_results` · `graph_events`, plus an install-scoped `entitlement_cache`.

**SQLite remains the MVP store.** A graph abstraction does not require a graph database; introducing one would add an operational dependency to a local-first tool for no measured need (ADR-GRAPH-001). WAL, single-writer semantics, one transaction per action, forward-only migrations, crash recovery and the repository abstraction are all unchanged.

**Scope boundary:** graph and project state are **project-scoped**; entitlement is **user/install-scoped** and never written into a repository (§67).
