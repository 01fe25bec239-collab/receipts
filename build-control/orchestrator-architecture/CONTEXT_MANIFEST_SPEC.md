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

# CONTEXT_MANIFEST_SPEC

Each durable role (RUNTIME-A1 and every RUNTIME-A2) has an authoritative Context Manifest: the list of sources that constitute its context, with digests.

## Schema

```
ContextManifest {
  manifest_id, role_id, project_id
  epoch                  integer
  sources: [ {
      ref            path | state-query | artifact id
      class          MANDATORY | CONSUMED | REFERENCE
      digest         content hash at last rehydration
      last_read_at
      required_for   [DECOMPOSITION, DISPATCH, ACCEPTANCE, INTEGRATION, EVALUATION]
  } ]
  derived_state: { active_tasks[], accepted_tasks[], open_findings[],
                   dependency_state, current_wave, next_actions[] }
  created_at, last_rehydrated_at
}
```

## Source classes

| Class | Rule |
|---|---|
| `MANDATORY` | Reread in full on every rehydration |
| `CONSUMED` | Reread when a task touches it, or when its digest changed |
| `REFERENCE` | Read on demand only |

Class-based minimisation is what keeps rehydration affordable. Loading everything for every role would make the mandatory triggers too expensive to honour — and a rehydration that is skipped for cost reasons is worse than one that is scoped.

## Typical contents (§60)

Original user spec (always `MANDATORY`) · current architecture · contracts and interfaces the role owns or consumes · ownership boundaries · decision log · DAG slice · branch and SHA state · active and accepted tasks · A4 findings · dependency requests · open issues · current wave · next actions.

## Digest tracking

Every source carries a content digest. On rehydration, digests are recompared: unchanged sources are cheap, changed sources are reread and flagged, missing sources are an error, and new sources required by an epoch change are added.

A changed `MANDATORY` source between two rehydrations is significant: the role has been operating on superseded information, and any decision it made in that window is flagged for review.

## Per-role scoping

RUNTIME-A1's manifest spans the goal, global DAG, workstream registry, and integration state. A RUNTIME-A2's spans its workstream, its contracts, its branch, and its tasks. An A2 does not carry the whole project's context — that is both wasteful and a source of accidental cross-workstream coupling.

## Ownership

`BUILD-A2-STATE-CONTEXT` owns manifests. A role reads its own manifest; it does not rewrite its own charter. Manifest mutation is an orchestrator action recorded as an event.
