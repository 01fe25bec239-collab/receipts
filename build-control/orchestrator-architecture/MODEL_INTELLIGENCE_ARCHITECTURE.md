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

# MODEL_INTELLIGENCE_ARCHITECTURE

## The problem

A1 and A2 executors are LLMs. Their training data contains opinions about which models are good — opinions that were already stale when training ended, and staler now. A model released yesterday must be usable **without an architecture change**, and a model that was best last year must not keep being chosen out of habit.

## The hard invariant (I-4, §28)

> **No RUNTIME-A1 or RUNTIME-A2 may select an executor solely from the LLM's internal knowledge of model capability.**

Enforced **structurally, not by instruction**: the router is deterministic code over the registry. An executor may express a preference; a preference with no registry backing is discarded, and the routing record shows what evidence was actually used. Telling a model "don't rely on your memory" is not a control; removing its ability to act on memory is.

## Service responsibilities

1. discover connected providers;
2. discover current models where the provider supports enumeration;
3. maintain the model catalog with freshness timestamps;
4. obtain official capability metadata;
5. determine runtime compatibility (which agent runtimes can drive which models);
6. detect newly released models;
7. record provenance and confidence for every capability claim;
8. track pricing where exposed;
9. track availability and quota state;
10. accumulate local performance observations;
11. expose ranked, filtered candidates to the router.

## Structure

```
┌──────────── MODEL INTELLIGENCE SERVICE ─────────────┐
│  Discovery      provider enumeration, model lists   │
│  Capability     official docs, independent evals    │
│  Compatibility  model × runtime matrix              │
│  Freshness      TTL, epochs, staleness detection    │
│  Calibration    local observations (see MODEL_CALIBRATION) │
│  Availability   quota/health state                  │
└──────────────────────┬──────────────────────────────┘
                       ▼
                  ROUTER (deterministic)
```

## Handling uncertainty

When capability data is stale, missing, contradictory, or newly changed, the system must:

1. refresh; or
2. ask the user; or
3. mark the selection blocked/uncertain.

**Never silently guess.** A silent guess is indistinguishable from working correctly until it routes security-critical work to an uncalibrated model.

## No vendor ranking (§29)

Nothing in the architecture encodes "Anthropic = best coding" or any permanent company ranking. Provider nationality is never a capability or safety input. Rankings are computed from evidence with recorded provenance, and are expected to change.

## No name-dependent branching (§30)

No control-flow branch keys on a model name. Names are configuration; the architecture must keep working when every current name is obsolete. A grep for hard-coded model names in core logic is a CI check.

## Bootstrapping

With an empty registry: enumerate connected providers, fetch official model lists and capability metadata, mark everything `UNASSESSED`, apply user pins if present, and — for the first frontier dispatch — either use officially documented capability or ask the user. The system does not invent a ranking to get started.

## Eligibility filtering precedes candidate generation (V1.3)

```
discover models
   ↓ technical availability
   ↓ POLICY ELIGIBILITY        ← new gate; only VERIFIED_ALLOWED proceeds
   ↓ product capability (entitlement admission)
   ↓ quality floor
   ↓ routing score
```

Lifecycle now distinguishes: `DISCOVERED` → `TECHNICALLY_AVAILABLE` → `POLICY_ELIGIBLE` → `ENTITLEMENT_USABLE` → `ROUTABLE`.

A newly released strong model is not automatically routable. Its **credential path** must also be eligible. Discovering a best model and only then finding its credential path unusable wastes a decision and risks a non-compliant dispatch — so eligibility filters before scoring, not after.
