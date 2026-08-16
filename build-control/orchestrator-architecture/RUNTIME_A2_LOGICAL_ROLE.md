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

# RUNTIME_A2_LOGICAL_ROLE

**A durable engineering workstream manager inside the user's project.** Examples: `RUNTIME-A2-AUTH`, `RUNTIME-A2-PAYMENTS`, `RUNTIME-A2-UI`, `RUNTIME-A2-DATABASE`.

## Granularity

An A2 owns **one coherent engineering workstream** with real code ownership. Not one A2 per task; not one A2 per file type.

A good A2 boundary has: a coherent subsystem, a stable interface with other workstreams, an integration branch it can own, and enough work to justify a durable manager. If a proposed A2 has none of these, the work belongs as tasks under an existing A2.

## Responsibilities

- own one workstream and its integration branch/workspace;
- read the relevant specification, architecture, and interfaces;
- decompose authorised work into **atomic** RUNTIME-A3 tasks;
- state capability requirements to the router (never name a model from memory);
- issue Task Capsules;
- manage intra-workstream dependencies;
- consume A4 findings and dispose of each explicitly;
- issue Repair Capsules within the bounded loop;
- accept A3 results only after required audit;
- coordinate cross-workstream needs **through A1**, never by editing another workstream;
- rehydrate context on mandatory triggers;
- survive executor replacement.

## Persisted identity

```
logical_role {
  role_id, project_id, role_type: 'RUNTIME_A2',
  workstream_id, name, ownership_paths[], integration_branch,
  status, context_manifest_id, current_context_epoch,
  active_binding_id?, binding_history[],
  decisions[], open_findings[], accepted_tasks[]
}
```

## Decomposition discipline

Consolidating managers must not consolidate tasks. A good A3 task has one clear outcome, explicit owned paths, explicit dependencies, independent reviewability, independent revertability, and objective acceptance criteria.

Rejected task shapes: "implement all of auth", "finish the payments workstream". If a reviewer who did not write it cannot audit it in one sitting, it is too large — and an unauditable task defeats the entire A3→A4 loop.

## No documentation-only RUNTIME-A2 (§16)

Never created by default. Reasoning:

- documentation *correctness* is inseparable from the subsystem it describes, so it belongs to that engineering A2;
- documentation *production* is a task, not a standing authority;
- a docs manager would accumulate a durable role with no code ownership and no integration branch — a manager in name only.

Docs work is an economy RUNTIME-A3 task (`ECONOMY_DOCS`) under the owning engineering A2. Architecture or security documentation that encodes a decision routes at `FRONTIER_REASONING`, because the reasoning is the artifact.

```
RUNTIME-A2-PAYMENTS
├── A3: implement webhook handler      FRONTIER_IMPLEMENTATION
├── A3: integration tests              FRONTIER_IMPLEMENTATION
└── A3: update payments README         ECONOMY_DOCS
```

## Boundaries

An A2 may not: write outside its ownership paths; mutate the global DAG; accept work that skipped required audit; overrule a blocking finding on its own authority; or bind more than one active executor.

Cross-workstream needs go to A1 as a dependency request. Solving one by editing another workstream's files is a boundary violation regardless of how correct the edit is — it destroys the ownership model that makes parallel execution safe.

## Failover

Same model as A1. Ownership, branch, decisions, dependencies, A3 children, review history, accepted work, and open issues all persist; only the binding changes.
