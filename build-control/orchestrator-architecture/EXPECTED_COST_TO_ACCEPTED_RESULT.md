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

# EXPECTED_COST_TO_ACCEPTED_RESULT

## The objective (§36)

> Optimise **expected cost, time, and compute to an accepted result** — not cheapest invocation.

A cheap model that fails audit twice consumes: its own attempt, an audit, a repair attempt, another audit, a second repair, a third audit — plus wall-clock. A frontier model that passes first time consumes one attempt and one audit. Per-call price ranks them one way; total cost ranks them the other.

## Model

```
E[cost] = C_impl
        + C_review
        + P_reject · ( C_repair + C_review + P_reject₂ · (…) )
```

Bounded by `max_repair_attempts`, so the series terminates. Expanded to the default bound of 3:

```
E[cost] ≈ C_impl + C_rev
        + p·(C_rep + C_rev)
        + p²·(C_rep + C_rev)
        + p³·(escalation cost)
```

where `p = P(reject | model, runtime, task_class)` from calibration.

## Worked illustration

Illustrative units, not measurements — real values come from calibration.

| | Economy model | Frontier model |
|---|---|---|
| `C_impl` | 1 | 8 |
| `C_rev` (frontier reviewer) | 6 | 6 |
| `p` (reject) | 0.55 | 0.15 |
| `C_rep` | 1 | 8 |
| **E[cost]** | **≈ 7 + 0.55·7 + 0.30·7 ≈ 13** | **≈ 14 + 0.15·14 ≈ 16** |

Here the economy model looks competitive — and for a low-risk task it may genuinely be. Now raise task difficulty so `p` rises to 0.8 for the economy model:

```
E[economy] ≈ 7 + 0.8·7 + 0.64·7 + escalation ≈ 20+
E[frontier] ≈ 16
```

The frontier model becomes cheaper *and* faster. This is why `p` must be per `(model, runtime, task_class)` (`MODEL_CALIBRATION.md`) rather than a global constant: the same two models swap ranks as difficulty changes.

Note also that the reviewer cost is constant across both rows — audit is charged regardless, which systematically penalises high-rejection implementers.

## Inputs

`C_impl`, `C_rep`, `C_rev` from pricing (or `UNKNOWN` → tier proxy); `p` from calibration blended with prior evidence and weighted by sample size; latency from observation; availability as a penalty.

## Handling UNKNOWN

Missing pricing does not stop routing. The estimator falls back to cost-tier proxies and marks `estimated_cost_class: UNKNOWN` on the decision. Fabricating a price would corrupt every downstream comparison with an untraceable number.

## Small-sample discipline

With few observations, `p` is dominated by the prior. The estimator never claims precision it does not have, and `RoutingDecision.confidence` reflects it. Optimising hard on n=3 is how a system talks itself into a bad default.

## Interaction with the floor

The floor filters first; this estimator ranks **within** the eligible set. It can never select below the floor, however attractive the arithmetic (I-9).

## Non-cost factors

Deadlines raise latency weight; `distinct_provider: REQUIRED` restricts the set before scoring; a degraded provider is penalised for expected retry cost.
