# State: Wave 1 completion notes

State stores what other owners define. It never routes, reviews, runs failover or decides acceptance. This page covers what the Wave 1 completion package added. Source of truth: `src/state/**`.

## Schema version 17

Migration 17 (`decision_records`) adds three tables. Migrations 1–16 are unchanged.

| Table | What it holds | Immutability |
| --- | --- | --- |
| `decision_records` | Routing decisions, registry capabilities, model observations, availability observations, review requests, A4 reviews (with their findings), integration requests and integration decisions. Each row keeps the exact supplied JSON bytes, their SHA-256, and extracted identity columns (task, linked request, bound SHA). | No update, delete or replace |
| `provenance_links` | One row per task and link kind: START, IMPLEMENTATION and REVIEW SHAs, the VERDICT and ACCEPTANCE references, and the WORKSTREAM and MAIN SHAs | No update, delete or replace |
| `logical_role_history` | A before/after snapshot of a role's status, binding pointer, epoch and manifest pointer for every change | No update or delete. Rows must start from the role's current row and use the next sequence number. |

Version 17 also adds triggers on existing role tables:

- A role's identity columns and ownership paths cannot be updated or deleted.
- Role rows cannot be deleted.
- An update to a pointer column is refused unless a fresh matching history row exists.

Upgrades:

- `SqliteStateRepository::migrate_existing_to_current` upgrades exact v11–v16 stores, verifying the v16 postcondition before it applies v17.
- Ordinary `open` refuses every non-current version.
- v1–v10 stores and unknown versions get typed refusals and are never rewritten: `SchemaVersionMismatch` from `open`, and `UNSUPPORTED_SCHEMA` from explicit migration.

## Decision and provenance records

`DecisionRecordV1::parse` checks the owner-frozen top level of each schema:

- an object with no duplicate decoded keys at any depth;
- no unknown property;
- every required property present;
- each property's type, enum, identifier length, SHA pattern, integer minimum or `minItems`.

Nested contents beyond duplicate keys are the producing owner's codec obligation. Numbers stay raw tokens.

`provenance_chain(project, task)` returns:

- the links that were recorded;
- the links that are missing;
- contradictions with stored records:
  - REVIEW_SHA differs from IMPLEMENTATION_SHA;
  - a verdict or acceptance reference is dangling;
  - the referenced review is for another task, or its reviewed SHA differs;
  - the integration decision's provenance entry or `integration_sha` disagrees with a link.

A complete and consistent chain is evidence for a gate to evaluate. It is not acceptance.

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

Every refusal leaves the watermark and the binding unchanged. An unsupported stored deadline is refused and never rewritten. Explicit expiry keeps its own existing fence.

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
- the open `derived_state` bytes.

`date-time` is treated as a schema annotation, so dates are not checked as dates.

Conversion to operational carriers can fail. Each failure names the unsupported field. Examples:

- non-canonical decimal epochs such as `1.0`, `1e3` or `-0`;
- `URL` manifest references;
- `derived_state`.

`ProductEntitlementWireV1::to_physical` goes only through the existing entitlement parser. Null optionals, out-of-range or exponent versions and non-canonical dates never become physical or verified values.

Conformance cases are generated in `wire/wire_tests.rs` and checked against the pinned offline `jsonschema` oracle. The frozen corpus has no State fixtures. Known oracle limits:

- the oracle cannot parse numbers beyond `f64`, so only the codec tests those;
- serde refuses lone-surrogate strings.
