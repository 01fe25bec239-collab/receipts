# CONTRACT-LEDGER-002 — Append-only Event / Projection Invariants

**Version:** 1.0.0  
**Owner:** A2-LEDGER  
**Consumers:** A2-CORE, A2-RUNNER, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M0  
**Depends on:** LEDGER-001 and domain payload contracts

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Freeze storage authority, transaction boundary, projections, concurrency and verify-ledger.

## Storage boundary

`${CLAUDE_PLUGIN_DATA}/receipts/<repoId>/` with `ledger.db`, `logs/`, `diffs/` and broker-owned provider/raw artifacts. SQLite WAL; explicit nonzero busy_timeout; foreign keys enabled.

## Transaction invariant

Each semantic mutation: validate -> begin transaction -> read chain head -> append event(s) -> update all affected projections -> commit. No projection mutation without event. Raw artifact is atomically written before event that references it.

## Projections

events is source of truth. tasks, claims, receipts, reviews, findings, code_evidence, admissions, overrides are rebuildable projections/indexes.

## verify-ledger

Read primary ledger without mutation; validate hash chain; rebuild projections into isolated temporary state; compare canonical logical values; report drift; do not repair in MVP.

## Concurrency

Per `(repoId, recipeKey)` advisory lock prevents duplicate recipe execution. After waiting, second process recomputes fingerprint and checks valid cached evidence before launch.

## Invariants

One writer authority = broker. Stale rows retained. Stored Admission audit-only. Raw logs outside SQLite.

## Failure behavior

Busy timeout -> STORAGE/TEMPORARY no partial commit. Missing referenced raw artifact -> INTEGRITY. Verify drift -> nonzero integrity result.

## Compatibility/versioning

Projection migrations may change tables but preserve event history/hash bytes. Event change follows LEDGER-001.

## Security constraints

No worker direct DB access; filesystem permissions + Claude deny are defense-in-depth.

## Example

Two simultaneous verify requests result in one execution and one cache hit.

## Negative examples

Receipt row committed before event; verify-ledger repairs silently; DB inside repo; delete stale receipt.

## Normative schema

```text
LedgerStore {
  events: append-only ordered LedgerEvent stream
  projections: rebuildable derived tables
  artifacts: broker-owned referenced files (logs/diffs/export support)
}

CommitBoundary = one broker invocation transaction
SourceOfTruth = events
ProjectionAuthority = derived-only
```

## Field semantics

This contract governs persistence relationships rather than adding domain fields. `events` is authoritative history; projections are query accelerators/materialized views; referenced artifacts are integrity-checked by stored digests/refs where their domain contract requires it.

## Required fields

Every committed domain mutation MUST have an append-only event in the same transaction as its projection changes. Ledger storage MUST use the architecture-required external plugin-data location, WAL mode and a configured busy timeout.

## Optional fields

Additional projection tables/indexes MAY be added without changing authority if they remain fully rebuildable from events. They MUST NOT become independent sources of truth.

## Validation rules

`verify-ledger` MUST validate sequence continuity, hash continuity, canonical event hashes, event-kind payload validity, projection rebuild equivalence, and required referenced-artifact presence/integrity where a digest is defined. It MUST report mismatch without repairing the primary ledger.

## Required tests

WAL contention; rollback; rebuild equality; missing artifact; advisory lock/cache hit; read-only verify.
