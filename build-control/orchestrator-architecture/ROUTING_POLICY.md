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

# ROUTING_POLICY

## Pipeline (§13)

```
ROLE REQUIREMENT → TASK CLASS → QUALITY FLOOR → REQUIRED CAPABILITIES
   → CURRENT AVAILABILITY → USER CONNECTED PROVIDERS → COST/QUOTA POLICY
   → (provider, model, runtime)
```

Hard filters run before optimisation. Cost never removes a candidate that a quality floor requires — the floor is a constraint, not a competing objective.

## Routing request (§37)

```
RoutingRequest {
  role                IMPLEMENTER | REVIEWER | MANAGER | EVALUATOR | RENDERER
  task_class          FRONTIER_IMPLEMENTATION | FRONTIER_REVIEW | FRONTIER_ARCHITECTURE
                    | SECURITY_CRITICAL_CODE | BALANCED_REASONING
                    | ECONOMY_DOCS | ECONOMY_SUMMARY | ECONOMY_STATUS | PRESENTATION_ONLY
  quality_floor       FRONTIER | BALANCED | ECONOMY
  required_capabilities[]     hard filter
  preferred_capabilities[]    soft ranking
  quality_priority / cost_priority
  constraints         { distinct_provider_from?, avoid_providers[], user_pins? }
  context_size_hint, deadline?
}
```

A request states **capabilities**, never "use model X because I remember X is best". The only exception is an explicit user pin.

## Selection

```
candidates = registry.triples()
  .filter(auth CONNECTED)
  .filter(runtime supports required_capabilities)
  .filter(availability ∈ {AVAILABLE, DEGRADED})
  .filter(lifecycle ∈ {ROUTABLE, CALIBRATING*})
  .filter(tier ≥ quality_floor)
  .filter(user policy allows)

if candidates empty → NO_ELIGIBLE_CANDIDATE (never downgrade — I-9)

score = expected_cost_to_accepted_result(candidate, task)     # EXPECTED_COST_TO_ACCEPTED_RESULT.md
        adjusted by quality_priority / cost_priority
        penalised by staleness and DEGRADED availability
```

`CALIBRATING` candidates are eligible only under the conditions in `MODEL_CAPABILITY_LIFECYCLE.md`.

## Modes (§39)

| Mode | Behaviour |
|---|---|
| `AUTO_CURRENT` | Pick the best eligible candidate automatically |
| **`ASK_ON_UNCERTAINTY`** (recommended default) | Route automatically; ask when a new uncalibrated frontier candidate appears, evidence is insufficient, top candidates are within a closeness threshold, a provider change materially alters cost or trust, or failover has large implications |
| `USER_CONTROLLED` | Present the ranking before every critical dispatch |

**Why `ASK_ON_UNCERTAINTY` by default:** `AUTO_CURRENT` silently makes consequential choices on thin evidence — precisely the failure §28 targets. `USER_CONTROLLED` destroys the long-horizon autonomy that is the product. Asking only when genuinely uncertain preserves autonomy where the system knows what it is doing and defers where it does not.

## User pinning (§40)

`prefer_model` · `prefer_provider` · `avoid_provider` · `never_use_model` · `max_cost` · `require_cross_provider_review`.

User policy overrides router preference wherever it is safe and possible. If a pinned model is unavailable, the configured failover policy applies; if a pin would violate a quality floor for security-critical work, the system asks rather than silently overriding either the user or the floor.

## Prohibitions

No hard-coded vendor ranking (§29). No branch keyed on a model name (§30). No selection from training memory (I-4). No silent frontier downgrade (I-9). **No provider-shopping to bypass a safety refusal (I-12)** — enforced in `SAFETY_INTERRUPTION_PROTOCOL.md`.

## Admission and eligibility precede scoring (V1.3)

```
canDispatch =
      product_capability_allowed
   && provider_authenticated
   && provider_available
   && provider_policy_eligible
   && quality_floor_satisfied
   && safety_state_permits
```

Each failure returns its **own** outcome — `LOCKED_REQUIRES_PRO`, `AUTH_REQUIRED`, `PROVIDER_RATE_LIMITED`, `PROVIDER_POLICY_DISALLOWED`, `PROVIDER_POLICY_UNKNOWN`, `NO_ELIGIBLE_RUNTIME`, `SAFETY_CHECK_PENDING`, `POLICY_BLOCKED`, `HUMAN_REQUIRED`. No single boolean replaces them, and a licence problem is never reported as a provider problem or vice versa.

`RoutingRequest` gains `execution_context`; `RoutingDecision` records the `FeatureAdmissionDecision` id and the `ProviderPolicyEligibility` id consulted. **No entitlement token material enters a capsule or a decision record** — references only.
