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

# INTEGRATION_GATE_ARCHITECTURE

## Principle (§78)

> The strongest gate is: **may this exact result enter its parent integration branch?**

Not "is the agent finished?" and never conversational wording such as "done". Integration is where provenance is checked, because it is the last point at which a bad result can be stopped cheaply.

## Two gates

```
RUNTIME-A3 result ──[A2 acceptance gate]──► runtime-a2/<workstream>
runtime-a2/<workstream> ──[A1 integration gate]──► main
```

## A2 acceptance gate

| # | Check |
|---|---|
| 1 | `review_sha == implementation_sha`, and that commit exists on the task branch |
| 2 | **No commit added after review** — `git log <review_sha>..<branch>` empty |
| 3 | Every acceptance criterion met; deterministic ones re-run, not read |
| 4 | Required A4 verdict present and passing; blocking findings closed |
| 5 | Required checks present at the correct SHA per the assurance profile |
| 6 | Diff entirely within `allowed_write_paths` |
| 7 | Dependencies still accepted; none reverted while this was in flight |
| 8 | Code-state freshness — evidence not stale against the merge target |
| 9 | Component suite passes on the **post-merge** tree, not the task branch alone |

Check 2 is the one most easily skipped and most damaging to skip: a single commit after review means the accepted code is not the reviewed code.

## A1 integration gate

Every included task `ACCEPTED`; every code task has a passing independent review; SHA identity chain intact for each; workstream branch clean and containing exactly the claimed merges; cross-workstream dependencies satisfied; interface compatibility across workstreams; cross-component tests on the merged tree; no unauthorised writes; no unresolved blocking finding; complete provenance chain.

## Provenance requirement

```
task_id → start_sha → implementation_sha → review_sha → verdict
        → acceptance → workstream_sha → main_sha
```

A missing or unverifiable link fails the gate. This chain is what allows someone who trusts none of the agents to check the result later — the property the product exists to provide, so the product must hold itself to it.

## Outcomes (§78)

| Outcome | Meaning | Next |
|---|---|---|
| `ACCEPT` | All conditions met | Merge preserving task provenance; never squash away boundaries |
| `REPAIR` | Fixable defect identified | Bounded repair task |
| `BLOCKED` | Cannot be judged — dependency, budget, or unresolved question | Record blocker and owner |
| `HUMAN_REQUIRED` | Judgement or authority beyond the system | Surface with the exact question |

No partial merge. No "merge now, fix after".

## What the gate never does

Integrate to keep a schedule. Waive an invariant or a required check. Accept evidence it cannot reproduce. **Implement the fix itself** — a defect returns to the owning A2 as a bounded task, because the moment the gate starts fixing things, nothing downstream has an independent check.

## Honesty (I-20)

The gate does not claim the code is correct. It claims the declared checks and reviews were executed against this exact code state and that the required conditions held.
