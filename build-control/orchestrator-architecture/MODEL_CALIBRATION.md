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

# MODEL_CALIBRATION

## Purpose

Learn from what actually happened here. Vendor benchmarks measure general ability; calibration measures *accepted results in this repository, on this task class*.

## Dimensions

Observations are keyed by `(model, runtime, task_class)`, optionally refined by repository language or subsystem. Aggregating across task classes hides the thing that matters — a model strong at implementation may be weak at review.

## Metrics (§35)

| Metric | Why it matters |
|---|---|
| `first_pass_A4_accept_rate` | The headline signal: did it pass audit without repair? |
| `A4_rejection_rate` | Complement, with finding categories |
| `repair_count_to_accept` | How many attempts to land |
| `task_completion_rate` | Did it produce a reviewable result at all? |
| `test_failure_rate` | Checks failing at handoff |
| `latency` | Wall-clock to handoff |
| `token_usage`, `cost` | Direct spend |
| **`cost_to_accepted_result`** | **The optimisation target** — total spend across attempts and audits, per accepted task |
| `timeout_rate`, `runtime_failure_rate` | Operational reliability |
| `user_override_rate` | How often the user rejected the router's choice |

`cost_to_accepted_result` is the metric the router actually optimises. Per-call price is an input to it, never a substitute.

## Honest statistics (§35)

**Sample size is stored with every aggregate and surfaced with every routing decision.** With small n:

- differences are not treated as meaningful;
- no significance language is produced anywhere;
- prior weight (official + independent evidence) dominates until local evidence accumulates.

Three tasks is an anecdote. The system must say so rather than route on it.

## Bayesian-style blending

```
score = w_local(n) · local_estimate + (1 − w_local(n)) · prior_from_evidence
```

`w_local` rises with sample size. New models are not punished for having no history, and long-serving models are not protected by reputation once evidence contradicts it.

## Contamination controls

- Task difficulty is recorded; hard tasks are not counted against a model as if they were easy.
- Rejections caused by spec ambiguity are excluded once identified through escalation.
- Provider outages count as `runtime_failure`, not quality failure.
- Repair attempts are attributed to the attempting model, not the original.

Without these, calibration would mostly measure which model drew the hardest tasks.

## What calibration is not

Not a public leaderboard, not a benchmark authority, not a claim about general model quality. It is local operational evidence for local routing decisions, and it is presented that way.
