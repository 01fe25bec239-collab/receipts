# State: Wave 1 completion notes

State stores what other owners define. It never routes, reviews, runs failover or decides acceptance. This page covers what the Wave 1 completion package added. Source of truth: `src/state/**`.

## Schema version 17

Migration 17 (`decision_records`) adds three tables and immutable-event triggers. Migrations 1–16 are unchanged.

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

- `SqliteStateRepository::migrate_existing_to_current` upgrades exact v11–v16 stores, verifying the v16 postcondition before it applies v17.
- Ordinary `open` refuses every non-current version.
- v1–v10 stores and unknown versions get typed refusals and are never rewritten: `SchemaVersionMismatch` from `open`, and `UNSUPPORTED_SCHEMA` from explicit migration.

## Decision and provenance records

`DecisionRecordV1::parse` validates the whole owner-frozen schema shape of all eight families (`RoutingDecision`, `ModelCapability`, `ModelObservation`, `AvailabilityState`, `ReviewRequest`, `A4Review`, `IntegrationRequest`, `IntegrationDecision`). The shape tables live in `decision_schema.rs`. At every nested level:

- no duplicate decoded keys at any depth;
- no unknown property and every required property present (selection reason, score components, capability evidence and its source reference, alternatives, registry freshness, acceptance criteria, review policy, references, reviewer, findings, dimensions, reproduction checks, included tasks, attestations, check results, provenance entries);
- each property's type, enum, identifier length (1–200), strict 40-hex SHA where the schema has the pattern, `minItems`, and exact integer and range rules.

Open fields stay open: `ModelCapability.value` and `capability_evidence[].value` accept any JSON. Numbers stay raw tokens and are never converted through `f64`: `1.0`, `1e3` and `-0` are integers, huge integers and exponents are admitted, and `expected_rejection_probability` is compared with 0 and 1 exactly. `date-time` is an annotation, so those fields accept any string. Plain-string SHA-like fields (`merge_commit`, `acceptance_sha`, `integration_sha`) are not pattern-checked because the schemas do not say so. Stored records are re-admitted on every read, so damaged nested content fails closed even when the stored digest was recomputed. The `jsonschema` crate is a test-only oracle; the validators are small `serde`/`RawValue` tables with no new dependency.

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
| `set_logical_role_context` | Compare-and-set on the epoch and manifest. The epoch cannot go backwards. A changed epoch must exist in the project's history. A manifest must match the role, project and new epoch. A manifest pointer cannot be cleared. |

Every refusal is a typed `RoleChangeRefusal` and writes nothing. The legacy create and release binding APIs do not touch role pointers.

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
