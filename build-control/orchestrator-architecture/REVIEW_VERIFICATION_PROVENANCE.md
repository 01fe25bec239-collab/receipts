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

# REVIEW_VERIFICATION_PROVENANCE

## Reframing (§67)

The old product was built on "frontier coding agents regularly lie that tests passed." **That framing is abandoned.** The system must be valuable when workers are competent — and they largely are.

The actual need:

> Multiple capable agents operate against different code states, sessions, providers, and workspaces. Integration requires durable knowledge of what changed, what was executed, what was reviewed, and **which exact code state those results apply to**.

The failure this prevents is not deception. It is **stale, mismatched, or unattributable evidence** — a review of a commit that has since moved, a check run before the last three edits, a result nobody can locate the origin of.

## Evidence sources

| Source | Produced by | Trust posture |
|---|---|---|
| `WORKER_EXECUTION` | The A3 running its verification plan | Accepted under LIGHT/STANDARD (§69) |
| `BROKER_EXECUTION` | The orchestrator re-running deterministic checks | Required under HIGH_ASSURANCE |
| `REVIEW_EXECUTION` | The A4 reproducing acceptance-critical checks | Required under STANDARD+ |
| `GIT_PROVENANCE` | Repository facts: SHAs, diffs, authorship, branch topology | Always authoritative |

`GIT_PROVENANCE` is the backbone: it is the one source no agent can assert, only observe.

## Worker checks are real evidence (§69)

Recorded for each: worker identity, runtime, model, **code SHA**, exact command, exit code, result, timing, output reference.

Accepting this under LIGHT and STANDARD is a deliberate cost decision. Re-running everything independently would roughly double check cost for a benefit that matters mainly in adversarial settings — so independent re-execution is reserved for HIGH_ASSURANCE, where the assurance is worth the compute.

## Exact code-state binding (I-5)

Every piece of evidence is bound to the SHA it was produced against.

```
evidence.code_sha == review.implementation_sha == accepted.sha
```

A review of `abc` never validates `xyz`. At acceptance, the gate verifies the identity chain and that no commit followed the review. This single rule is what makes the assurance chain meaningful; without it, every other control is decorative.

## Staleness

Evidence bound to a superseded code state is **stale** and cannot support acceptance. This is inherited directly from the old architecture, which identified it correctly. Whole-tree binding is used at MVP: prefer false invalidation over false validity — re-running a check is cheap, accepting on stale evidence is not.

## Provenance record

```
Provenance { subject_sha, baseline_sha, task_id, attempt_id,
             worker { runtime, model, provider, binding_id },
             checks[] { source, command, exit_code, result, timing, output_ref },
             review { review_id, reviewer, verdict, reviewed_sha },
             workspace { branch, worktree },
             routing_decision_id, assurance_profile, created_at }
```

Persisted append-only. An integration decision months later is fully reconstructible (I-19).

## Honest limits

The chain proves that **declared checks and reviews were executed against identified code states**. It does not prove correctness, adequacy of the checks, or absence of defects (I-20). No output may imply otherwise.
