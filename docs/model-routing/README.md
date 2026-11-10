# Model Routing (Wave 2: MR-W2-1..3, MR-WIRE)

Routing chooses a (provider, model, runtime) triple from registry evidence
only, records why, and fails over on availability limits without losing the
task. It never writes State directly.

## Router (`src/routing/router.rs`)

`route(request, ctx, decision_id, fallback_from)` evaluates every registry
association and keeps every reason a candidate fails:

1. registry candidates only (I-4); caller evidence must bind positionally;
2. request identity constraints (distinct/avoid/pins);
3. lifecycle `ROUTABLE`, required capabilities Confirmed on the runtime path,
   availability `AVAILABLE`/`DEGRADED`, recorded technical status `CONNECTED`,
   provider policy (existing bounded evaluators);
4. quality floor = stricter of request floor and task-class floor. A model
   meets it only with registry capability `quality_tier.<TIER>` (tier ≥ floor)
   established `true` at OFFICIAL/INDEPENDENT/LOCAL_EMPIRICAL confidence.
   UNKNOWN, conflicted, missing, false or USER_DECLARED/UNVERIFIED claims fail;
5. ranking: AVAILABLE > DEGRADED, fresh > stale tier evidence, more Confirmed
   preferred capabilities, stronger tier confidence, canonical id order.

No vendor constant and no cost term: cost components are explicit null
(UNKNOWN) and `estimated_cost_class` is UNKNOWN until the W5 estimator.
`decisive_factor` names the first ranking key that separated the winner;
each alternative's `rejection_reason` lists all its reason codes (axis
prefixed: `AVAILABILITY:`, `AUTH:`, `PROVIDER_POLICY:`,
`SAFETY_OR_POLICY_HOLD:`, `LIFECYCLE:`, `QUALITY_FLOOR_UNMET:`, ...).

No eligible candidate never downgrades: `NO_ELIGIBLE_CANDIDATE`
(`WAIT_RETRY_AFTER` when an otherwise-eligible candidate is only rate/session
limited within the wait budget) or `USER_INPUT_REQUIRED` beyond the budget.

## Availability, backoff, failover (`src/availability/`)

Runtime `FailureClass` → `RATE_LIMITED`/`SESSION_EXHAUSTED`/`PROVIDER_DOWN`
for `retry_after` (used as given) or capped exponential backoff with jitter
(`BackoffPolicy`, config). Attempt n has the ceiling `min(cap, base·2^(n-1))`
and waits `ceil(ceiling/2) ..= ceiling` ("equal jitter": never zero, never past
the cap). The attempt history is per (provider, model): a runtime change does
not reset it and a restart rebuilds it from the persisted records, whose note
records `window_source`, the ceiling and the jitter drawn. Jitter comes from a
`JitterSource` (`RandomJitter` by default, `FixedJitter` for deterministic
tests); the manager clamps every draw to its bound.

`SafetyCheckPending`/`PolicyBlocked` never change availability; a failover
after one is `BLOCKED` (I-12, no provider-shopping). An active limit never
overwrites a SAFETY_CHECK_PENDING, POLICY_BLOCKED or AUTH_REQUIRED base state
in `effective`, so those refusals keep their axis and reason and never become
a WAIT. Retry windows are unsigned end to end: any `u64` window stays active
for its real duration. Other classes are not availability signals. Quota
scope stays UNKNOWN unless a QuotaState with `HEADERS` visibility reports it,
and MODEL scope additionally needs the report to name the observed model
(missing → UNKNOWN; another provider's or model's report is refused).
UNKNOWN scope limits only the exact candidate and marks the provider
DEGRADED. An elapsed window leaves the candidate DEGRADED until newer
evidence. `fail_over` keeps request/task ids, links `fallback_from`, and ASKs
past the backoff attempt or chain bound.

## Persistence (`src/routing/persistence.rs`, `src/registry/persistence.rs`)

`RoutingStatePort` is State's read-only repository plus ONE write callback,
`publish(&Publication)`, where a `Publication` is the records, the event
proposals (with the exact content each digest covers) and the D5 fact of one
outcome. A transaction failure publishes nothing; a success publishes everything.
Interruption or a lost acknowledgement can leave the complete package committed,
so an error alone cannot prove that nothing was written. An exact retry returns
the original receipts (`replayed`) without duplicate writes; conflicting content
under the same logical identity refuses. Routing advances registry/availability
memory only after proposal-bound event receipts matching State’s stored receipts
and an exact durable availability fact/receipt match (complete partition, fact
content, sequence, recorded timestamp and status through that sequence),
or rebuilds it from State.
A runtime probe report is one publication. The composition root implements the port over the
core's `CoreStateWriter`. Every event payload is the exact content its digest
names: a persisted record (`state-query:decision_record/<KIND>/<id>`,
`sha256:` State digest) or, for ROUTING_REQUESTED, the request's canonical
wire JSON (`routing-request:<request_id>`, `sha256:` of those bytes).
`persist_request` publishes it once per request, before the first decision;
the composition's content resolver must resolve that reference to exactly
the admitted request bytes, which the core re-hashes (no State table, no raw
writer).

| Record | State kind | Events |
|---|---|---|
| routing request | none (request body behind `routing-request:<id>`) | ROUTING_REQUESTED |
| RoutingDecision | ROUTING_DECISION | ROUTING_DECIDED + EXECUTOR_SELECTED, or ROUTING_FAILED_NO_CANDIDATE, or ROUTING_DECIDED |
| availability effect | AVAILABILITY_OBSERVATION (`availability-<seq>`) | RATE_LIMIT_OBSERVED / PROVIDER_DEGRADED, + D5 demotion fact |
| registry change | REGISTRY_CAPABILITY (`registry-<seq>`) | MODEL_DISCOVERED / MODEL_LIFECYCLE_CHANGED / REGISTRY_REFRESHED |

Registry structure and lifecycle rows use reserved `receipts.registry.*`
capability names; reload replays them through `ModelIntelligenceService`.
Runtime probe reports become RUNTIME-subject LOCAL_EMPIRICAL
`runtime_probe.<flag>` evidence and never confirm a pair capability.

## N6 exits (`src/intelligence/lifecycle_exits.rs`)

Side-state exits are checked against State's D5 order: pre-demotion,
non-durable (forged), unauthenticated-claim and retry-after evidence refuse.
No authenticated post-demotion producer exists (adopted D5 deferral, after
W3), so no exit can succeed yet; the success type is `Infallible`.

## Codecs (`src/routing/wire/`)

Strict JSON codecs for RoutingRequest, RoutingDecision, AvailabilityState,
QuotaState, ModelCapability, ModelObservation and ProviderPolicyEligibility.
Errors: `Json`, `Shape` (schema shape), `Refused` (schema-valid but outside
the carrier: non-RFC3339 timestamps, `1.0` integers, missing `occurred_at`,
numbers the carrier cannot hold exactly). Absent and null stay distinct;
output keys are sorted. Numbers are admitted on their exact value (raw token,
no `f64` detour) and only then converted: an integer field takes the
canonical spelling of an in-range `u64`/`i64`, an `f64` field takes a number
whose shortest round-trip decimal is the same value, and anything else that
is schema-valid (`1e400`, `2^64`, `9007199254740993`, `1e-400`) is `Refused`
rather than rounded, saturated or reported as malformed. Open capability
values are held to the same rule. Fixtures in `src/routing/wire/fixtures/`;
the numeric expectation tables come from an independent CPython oracle.

## Not in this wave

`PROVIDER_RECOVERED` (no authenticated recovery producer),
`USER_ROUTING_INPUT`/modes/pinning (MR-W5-3), live policy gate (MR-W5-1),
calibration/cost/refresh/discovery (MR-W5-2).

## Core/State library composition

`receipts_e2e::CoreRoutingState::new(writer, clock, requests, project_id)`
implements `RoutingStatePort` using the real
`CoreStateWriter::publish_decision_package`. The composition supplies the
admitted Core writer, trusted clock, fixed project and project-scoped
`CoreContentResolverV1` for admitted requests. Merely attaching request bytes
to a proposal never admits them. The resolver must retain or reconstruct the
exact canonical `encode_routing_request` bytes for retry after restart.

The adapter resolves record references from the typed records in the current
publication, so newly written content needs no stale resolver cache. Core
independently resolves and hashes every event payload before building the State
package. State writes records, events/receipts and the availability fact in one
SQLite transaction and owns the durable retry identity; there is no second
journal. The project-scoped publication key is `routing-v1-<sha256>` of the first
record's reference (the registry/availability append slot or decision id), or
of the request reference for a request-only publication. Event keys are
`routing-event-<zero-based ordinal>`. Payload bytes, payload digests and sibling
counts do not define identity, so altered retries cannot become new operations.
Rebuild the same ordered publication for an exact retry. After a lost
acknowledgement, retry it before assessing a new jittered effect or allocating
a new append slot, or authoritatively reload the committed history.

Real SQLite controls in `crates/receipts-e2e/tests/routing_state.rs` cover late
transaction rollback, request/record content refusals, conflicting retries,
original receipt recovery after reopen, whole probe reports, availability facts
and unchanged memory on unconfirmed success. `sha256_hex` uses the pinned
`sha2` crate; all seven codecs and their exact numeric rules are unchanged.

This is the authorized library adapter. Production composition-root wiring,
deployed authority/isolation, authenticated D5/N6 recovery producers and
independent cumulative A3/Q06 remain separate boundaries.

Receipt validation refuses faulty-port success before advancing memory. It cannot
roll back any writes that the faulty port already committed. Historical fact
receipts remain valid after later facts and reopening; current lifecycle alone
does not establish a receipt’s identity.
