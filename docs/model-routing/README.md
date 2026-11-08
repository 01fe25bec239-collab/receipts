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
for `retry_after` or capped exponential backoff (`BackoffPolicy`, config, no
jitter). `SafetyCheckPending`/`PolicyBlocked` never change availability; a
failover after one is `BLOCKED` (I-12, no provider-shopping). Other classes
are not availability signals. Quota scope stays UNKNOWN unless a QuotaState
with `HEADERS` visibility reports it; UNKNOWN scope limits only the exact
candidate and marks the provider DEGRADED. An elapsed window leaves the
candidate DEGRADED until newer evidence. `fail_over` keeps request/task ids,
links `fallback_from`, and ASKs past the backoff attempt or chain bound.

## Persistence (`src/routing/persistence.rs`, `src/registry/persistence.rs`)

`RoutingStatePort` is the trusted composition's narrow callbacks (decision
record, event proposal + content, D5 availability fact) plus State's
read-only repository; the composition root implements it over the core's
`CoreStateWriter`. Every event payload is the exact persisted record
(`state-query:decision_record/<KIND>/<id>`, `sha256:` State digest).

| Record | State kind | Events |
|---|---|---|
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
the carrier: non-RFC3339 timestamps, `1.0` integers, missing `occurred_at`).
Absent and null stay distinct; numbers are bounded carriers; output keys are
sorted. Fixtures in `src/routing/wire/fixtures/`.

## Not in this wave

`ROUTING_REQUESTED` (no State record family or public digest helper for the
request body), `PROVIDER_RECOVERED` (no authenticated recovery producer),
`USER_ROUTING_INPUT`/modes/pinning (MR-W5-3), live policy gate (MR-W5-1),
calibration/cost/refresh/discovery (MR-W5-2).
