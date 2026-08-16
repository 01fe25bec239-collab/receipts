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

# ENTITLEMENT_ADMISSION_PROTOCOL

## Enforcement lives in shared core, never in host UI

```
Host command / node dispatch
        │
        ▼
  Core FeatureAdmission
        │
        ├── ALLOW  → execute
        └── DENY   → zero dispatch, zero state mutation
```

A Claude skill that hides a Pro command while a direct core call executes it is **not** enforcement. Both hosts pass through the same admission logic, and the parity conformance suite includes a bypass attempt (§111).

## Two decisions, not one (V1.3.1)

V1.3's `FeatureAdmissionDecision` carried `UNAVAILABLE_PROVIDER`, `UNAVAILABLE_POLICY`, `UNAVAILABLE_HOST`, `UNAVAILABLE_RUNTIME` and `BLOCKED_SAFETY` — which contradicted the architecture's own rule that those are separate axes. A single decision type that can express a provider failure *is* the axis collapse the design forbids.

Corrected:

```
FeatureAdmissionDecision   answers ONLY: may OUR product capability execute?
                           outcomes: ALLOW | LOCKED_REQUIRES_PRO
                                   | ENTITLEMENT_UNKNOWN | ENTITLEMENT_EXPIRED

DispatchAdmissionDecision  composes, by reference:
                             FeatureAdmissionDecision
                             provider connection state
                             ProviderPolicyEligibility
                             AvailabilityState
                             safety state
                             quality-floor result
                           returns ALLOW | DENY + exactly one failing_axis
```

`DispatchAdmissionDecision` holds **references**, not copies — duplicating full records would create two sources of truth for the same fact.

Enforced invariants: `FEATURE_ADMISSION_PROVIDER_OUTCOMES = 0`, `FEATURE_ADMISSION_SAFETY_OUTCOMES = 0`. **Every provider dispatch must consume a `DispatchAdmissionDecision`.**

## Full dispatch gate

```
canDispatch =
      product_capability_allowed      (FeatureAdmissionDecision)
   && provider_authenticated          (technical status)
   && provider_available              (availability / quota)
   && provider_policy_eligible        (ProviderPolicyEligibility)
   && quality_floor_satisfied         (routing)
   && safety_state_permits            (safety)
```

No single boolean replaces these, and each failure returns its **own** distinct outcome.

## Distinct failure vocabulary

`LOCKED_REQUIRES_PRO` · `AUTH_REQUIRED` · `PROVIDER_RATE_LIMITED` · `PROVIDER_POLICY_DISALLOWED` · `PROVIDER_POLICY_UNKNOWN` · `NO_ELIGIBLE_RUNTIME` · `SAFETY_CHECK_PENDING` · `POLICY_BLOCKED` · `HUMAN_REQUIRED` · `ENTITLEMENT_UNKNOWN`

A user must always know **what** prevented execution. Returning "upgrade to Pro" for a rate limit, or "auth required" for a policy refusal, is a defect.

## FREE user invokes a Pro capability

1. resolve entitlement **before** execution;
2. return `LOCKED_REQUIRES_PRO` deterministically;
3. explain the feature;
4. give activation instructions;
5. **create no Pro dispatch**;
6. **consume no paid provider execution**;
7. **mutate no authoritative state** as though it ran.

The node is marked `LOCKED_REQUIRES_PRO` in the graph — visible, explained, never dispatched. Verified by scenario S-FREE-PRO-ATTEMPT: `provider dispatch count = 0`, `graph corruption = 0`.

## Offline behaviour

| Situation | Behaviour |
|---|---|
| FREE, service unreachable | **Fully functional.** FREE never depends on our licensing service |
| PRO, valid cached entitlement | Bounded offline grace per `offline_grace_until` |
| PRO, grace expired | No new Pro dispatch; FREE remains functional; history readable |
| Service unreachable, no cache | `ENTITLEMENT_UNKNOWN` — **not** silently `FREE` |

## Expiry mid-run

- an already-running Pro node may finish or checkpoint — killing it risks workspace corruption, which is a worse outcome than a slightly late gate;
- no **new** Pro-only dispatch begins;
- accepted evidence and history remain readable;
- the graph is never corrupted;
- the user gets an explicit entitlement state, not a silent stall.

## Admission is recorded

Every `FeatureAdmissionDecision` is persisted with capability, outcome, entitlement state, reason and `dispatch_permitted`. `dispatch_permitted: false` is the machine-checkable guarantee that no provider call and no state mutation occurred.
