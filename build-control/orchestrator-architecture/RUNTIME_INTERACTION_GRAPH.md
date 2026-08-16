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

# RUNTIME_INTERACTION_GRAPH

**Scope: how subsystems communicate when the finished program runs.**

**Bidirectional runtime interaction is allowed and does not imply a cyclic implementation dependency.** A request/response pair between two subsystems is one collaboration, not two build dependencies.

## Interactions

| From | To | Payload / purpose |
|---|---|---|
| `ORCHESTRATION` | `REVIEW-INTEGRATION` | `ReviewRequest` — audit this attempt at this exact SHA |
| `REVIEW-INTEGRATION` | `ORCHESTRATION` | `A4Review` verdict, task state transition |
| `REVIEW-INTEGRATION` | `MODEL-ROUTING` | RoutingRequest for reviewer/repair executor |
| `MODEL-ROUTING` | `REVIEW-INTEGRATION` | RoutingDecision |
| `REVIEW-INTEGRATION` | `MODEL-ROUTING` | `ModelObservation` calibration feedback — **contract owned by MODEL-ROUTING**, consumed by REVIEW-INTEGRATION |
| `ORCHESTRATION` | `MODEL-ROUTING` | RoutingRequest for implementer/manager |
| `MODEL-ROUTING` | `ORCHESTRATION` | RoutingDecision |
| `ORCHESTRATION` | `WORKSPACE-EXECUTION` | provision workspace |
| `WORKSPACE-EXECUTION` | `ORCHESTRATION` | WorkspaceHandle, checkpoint events |
| `RUNTIME-ADAPTERS` | `WORKSPACE-EXECUTION` | execute inside worktree |
| `WORKSPACE-EXECUTION` | `RUNTIME-ADAPTERS` | WorkspaceHandle |
| `MODEL-ROUTING` | `RUNTIME-ADAPTERS` | capability probe, health |
| `RUNTIME-ADAPTERS` | `MODEL-ROUTING` | AvailabilityState, QuotaState |
| `HOST-INTEGRATION` | `ORCHESTRATION` | `NormalizedHostEvent`, `START_GOAL` — **contract owned by HOST-INTEGRATION**, consumed by ORCHESTRATION |
| `ORCHESTRATION` | `HOST-INTEGRATION` | CoreView for rendering |
| `ALL` | `STATE-CONTEXT` | persist / read |
| `STATE-CONTEXT` | `ALL` | durable state, context epochs |

**Derived:** 17 runtime interactions, 17 of them in reciprocal pairs.

## Principal runtime loop

```
HOST-INTEGRATION ──NormalizedHostEvent──▶ ORCHESTRATION
        ▲                                      │
        │ CoreView                             │ RoutingRequest
        │                                      ▼
        │                              MODEL-ROUTING ◀── AvailabilityState ── RUNTIME-ADAPTERS
        │                                      │                                   ▲
        │                                RoutingDecision                            │ execute
        │                                      ▼                                   │
        └───────────────────────────── REVIEW-INTEGRATION ──▶ WORKSPACE-EXECUTION ──┘
                                              │  ▲
                                    ModelObservation (feedback)
                                              ▼
                                       STATE-CONTEXT  (persist / read — all subsystems)
```

`STATE-CONTEXT` is bidirectional with every subsystem at runtime and has **zero** hard build dependencies. That combination is exactly why the three graphs had to be separated: a single matrix would have shown it as maximally coupled when it is in fact the foundation everything else is built on top of.

## Ownership vs data flow

Two edges here are easy to misread, and V1.2.1 did misread them:

| Contract | Flows | Owned by | Consumed by |
|---|---|---|---|
| `ModelObservation` | REVIEW-INTEGRATION → MODEL-ROUTING | **MODEL-ROUTING** | REVIEW-INTEGRATION |
| `NormalizedHostEvent` | HOST-INTEGRATION → ORCHESTRATION | **HOST-INTEGRATION** | ORCHESTRATION |

The first is owned by the *receiver*, the second by the *sender*. Ownership follows who defines the shape, not who moves the bytes.

## Rule

Nothing in this document may be used to justify a build-ordering constraint. Build ordering comes from `BUILD_IMPLEMENTATION_DAG.md` alone.
