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

# BUILD-A2-STATE-CONTEXT

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-STATE-CONTEXT` — State, Identity & Context.

## Mission
Implement the durable substrate: the state store, logical-role identity, executor bindings and failover mechanics, context manifests and epochs, the rehydration engine, and the append-only event log.

## Why long-lived
Everything the product promises about surviving rate limits, session loss, and host switches reduces to this layer being correct. It is also the layer with the strongest integrity requirement: **no worker, adapter, or model output may write to it** (I-18).

## Owned subsystem
Durable state store (SQLite for MVP) and repository interface · schema and migrations · transaction and crash safety · logical-role persistence · executor bindings and leases · failover mechanics · context manifests and digest tracking · context epochs · rehydration engine · append-only event log · redaction layer on every persistence path · startup recovery.

## Owned repository paths
`src/state/**` · `src/context/**` · `src/events/**` · owned schemas · **`docs/state-context/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`ActivationState` · `ContextEpoch` · `ContextManifest` · `EntitlementVerifier` · `ExecutorBinding` · `LogicalRole` · `ProductEntitlement`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| — | none |


## Reference-only
All other contracts, since it persists them.

## Forbidden ownership
Orchestration semantics · routing · adapters · workspace/git · review and gates · host adapters. It stores what others define; it never defines meaning.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- **None.** This manager has no hard build dependency and is the DAG source.

**Build wave: W1** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- **None.**


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- Persist/read via `BUILD-A2-STATE-CONTEXT`.


## Expected BUILD-A3 task categories
State store schema and migrations · repository interface · transaction and WAL configuration · crash-safe write path · logical-role persistence · executor binding with lease and expiry · single-active-binding enforcement · append-only event log · redaction layer · context manifest storage with digests · epoch tracking and invalidation · rehydration engine (digest comparison, scoped reread) · startup recovery and orphan detection · state inspection tooling.

## Expected BUILD-A4 review categories
Append-only actually enforced (no update/delete path on events, attempts, reviews, findings, decisions) · **no write path reachable from a worker, adapter, or model output** · crash mid-write leaves a consistent store · lease expiry prevents permanent lock · single-active-binding cannot be violated · redaction covers every persistence path including error messages · rehydration rereads sources rather than replaying summaries · epoch invalidation cannot be skipped.

## Frontier / economy policy
Frontier for the store, transactions, crash safety, bindings, and rehydration. Economy for schema documentation.

## Security responsibility
Owns the integrity boundary of the orchestrator's authority. A defect here — a reachable write path, a leaked credential in an event — invalidates every gate above it. Must pass REVIEW-INTEGRATION's security acceptance tests.

## Integration responsibility
Supplies the persisted provenance the integration gate reconstructs. If the event log cannot answer "why was this accepted", that is a defect in this manager.

## Context requirements
Initial: state architecture, event model, context rehydration, manifest spec, security trust model. Rehydration: on any entity-model change.

## Non-goals
Does not orchestrate · does not route · does not execute · does not review · does not adapt hosts · does not interpret the data it stores.

## First proposed milestone
`M-STATE-1`: state store schema, repository interface, transactional crash-safe writes, append-only event log with redaction, and logical-role + binding persistence with leases. **This milestone unblocks the entire build.**
