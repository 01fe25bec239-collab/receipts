# ADR-D11: operational dispatch admission derivation

Status: adopted for the Wave2 preparation lineage under the already-approved D11 principle (A1 disposition 2026-11-08). This is an engineering ADR, not a frozen schema change. Implementation: `src/core/scheduler/dispatch_admission_derivation.rs`.

## Decision

`derive_dispatch_admission` evaluates all six axes from typed evidence and returns either a `DispatchAdmissionDecisionNonTemporalCore` plus every failing axis, or a typed pre-dispatch refusal.

- ALLOW only when all six axes PASS from present, fresh evidence bound to the requesting node, graph and context epoch, and the provider-policy record is VERIFIED_ALLOWED for the same provider as the auth evidence. No PASS by default from missing, unknown, NOT_APPLICABLE or a non-empty reference. The evaluator never emits NOT_APPLICABLE: every axis is evaluated and recorded.
- Primary failure precedence is explicit policy: SAFETY, ENTITLEMENT, PROVIDER_POLICY, PROVIDER_AUTH, PROVIDER_AVAILABILITY, QUALITY_FLOOR (`PRIMARY_FAILURE_PRECEDENCE`). It is not enum or prose order. The decision stores the primary axis and reason; `failures()` keeps all of them. Every DENY means zero dispatch and names no selected provider/runtime.
- ALLOW emits `denial_reason = Some(NONE)`; DENY emits a present non-NONE reason. Existing value constructors and codecs still accept every schema-permitted shape (absent reason, NOT_APPLICABLE); only this evaluator is stricter.

## Reason mapping

| Axis | Evidence | Reason |
|---|---|---|
| SAFETY | fresh Clear | PASS |
| SAFETY | Blocked / HumanRequired (any freshness) | POLICY_BLOCKED / HUMAN_REQUIRED |
| SAFETY | Pending, stale Clear, missing | SAFETY_CHECK_PENDING |
| ENTITLEMENT | fresh ALLOW | PASS |
| ENTITLEMENT | LOCKED_REQUIRES_PRO / ENTITLEMENT_EXPIRED | same reason |
| ENTITLEMENT | ENTITLEMENT_UNKNOWN, stale ALLOW, missing | ENTITLEMENT_UNKNOWN |
| PROVIDER_POLICY | fresh VERIFIED_ALLOWED for the authenticated provider | PASS |
| PROVIDER_POLICY | VERIFIED_DISALLOWED (any freshness) | PROVIDER_POLICY_DISALLOWED |
| PROVIDER_POLICY | NEEDS_REVIEW, UNKNOWN, stale/missing, or a record about another provider | PROVIDER_POLICY_UNKNOWN |
| PROVIDER_AUTH | fresh Authenticated | PASS |
| PROVIDER_AUTH | Unauthenticated, Unknown, stale, missing | AUTH_REQUIRED (technical state; never "wrong credentials") |
| PROVIDER_AVAILABILITY | fresh Available with an admitted runtime | PASS |
| PROVIDER_AVAILABILITY | actual rate limit / proven down | PROVIDER_RATE_LIMITED / PROVIDER_DOWN |
| PROVIDER_AVAILABILITY | Unknown, stale, missing, no admitted runtime | NO_ELIGIBLE_RUNTIME (unknown is never "down") |
| QUALITY_FLOOR | fresh Satisfied | PASS |
| QUALITY_FLOOR | Unsatisfied, stale, missing | QUALITY_FLOOR_UNSATISFIED |

POLICY_BLOCKED here is a safety/product-policy stop, not a renamed provider-policy rejection.

## Typed pre-dispatch refusals

Returned outside the frozen decision, because a decision referencing those facts would be untruthful:

- `EvidenceBindingMismatch { axis }`: evidence bound to another node, graph or context epoch. No stale-epoch enum value is invented and it is not reported as an outage.
- `ContradictorySafetyState`: Clear with an open SafetyInterruption, or a non-Clear state without one.
- `InvalidIdentity`: a mandatory identity or reference outside its frozen domain.

## Proof

`dispatch_admission_derivation_tests.rs`: all-PASS; each single-axis failure; all thirteen non-NONE reasons mapped (plus no-runtime); all 15 pairs in both input orders; all-fail; missing and stale per axis; provider mismatch; binding mismatch per axis and field; contradictory safety; frozen fixtures C/D/E reproduce outcome, axis, reason and selected provider/runtime unchanged.

Not covered: the real evidence producers (FeatureAdmission, provider auth, ProviderPolicyEligibility, AvailabilityState, SafetyInterruption, routing) and their freshness policies; scheduler consumption is OR-W2-1.
