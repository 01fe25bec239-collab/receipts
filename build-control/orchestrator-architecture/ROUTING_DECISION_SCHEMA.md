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

# ROUTING_DECISION_SCHEMA

Every dispatch produces a persisted, inspectable decision (§38). Routing that cannot be explained cannot be debugged, trusted, or improved.

```
RoutingDecision {
  decision_id, request_id, task_id, occurred_at

  selected_provider, selected_model, selected_runtime

  selection_reason        structured, not prose:
    { quality_floor_applied, hard_filters_passed[],
      score_components: { expected_implementation_cost, expected_rejection_probability,
                          expected_repair_cost, expected_review_cost, latency_estimate },
      decisive_factor }

  capability_evidence[]   { capability, value, confidence, source_ref, observed_at, sample_size? }
  confidence              HIGH | MEDIUM | LOW
  availability            state + retry_after? at decision time
  estimated_cost_class    LOW | MEDIUM | HIGH | UNKNOWN

  alternative_candidates[]  { provider, model, runtime, score, rejection_reason }

  registry_freshness      { model_list_age, capability_age, calibration_sample_size, stale: bool }

  mode                    AUTO_CURRENT | ASK_ON_UNCERTAINTY | USER_CONTROLLED
  user_involved           boolean
  user_pin_applied?       
  fallback_from?          prior decision_id, if this is a failover
}
```

## Rules

1. **Alternatives are recorded with rejection reasons.** "Why not the cheaper model?" must be answerable months later.
2. **`decisive_factor` is required.** One named component that determined the outcome.
3. **Evidence carries provenance and confidence.** A decision made on `UNVERIFIED` evidence is visible as such.
4. **Freshness is recorded.** A decision on stale data is identifiable in hindsight — important when diagnosing a run of bad routing.
5. **Failover chains are linked** via `fallback_from`, so a sequence of provider failures reads as one story.
6. **Decisions are immutable.** A changed choice is a new decision.

## Surfacing

Available to the user on both hosts (parity row P-05) as a compact summary — selection, decisive factor, confidence, alternatives, freshness — with the full record on request. Rendered by template or economy model; per I-13 the renderer never alters it.

## Uses

Debugging bad outcomes; calibration feedback (decision joined to eventual accept/reject); cost attribution; user trust; and post-hoc audit of whether a security-critical task was routed on adequate evidence.
