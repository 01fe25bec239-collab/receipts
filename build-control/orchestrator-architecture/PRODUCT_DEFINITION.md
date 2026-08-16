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

# PRODUCT_DEFINITION

## One-sentence definition

A host-neutral, **graph-engineered coding control plane** with a genuinely useful single-runtime **Free** graph mode and an optional **Pro** distributed multi-runtime orchestration mode — running inside Claude Code and Codex as equal first-class hosts, over one shared local core and one durable ExecutionGraph.

## The central object

```
USER GOAL / SPEC → GraphCompiler → ExecutionGraph → ExecutionPolicy → GraphScheduler
```

The **ExecutionGraph** is the product. It is durable, versioned, auditable, resumable, and identical in FREE and PRO. What changes between tiers is the execution policy — which capabilities are admitted and how nodes are dispatched — never the graph itself.

This is not "a multi-agent router". It is not "graph visualization". It is a graph-native execution control plane where the plan is a first-class artifact bound to exact code states.

## Two tiers, one engine

| | FREE | PRO |
|---|---|---|
| Graph | full — compile, schedule, validate, inspect, resume | identical graph |
| Execution | single eligible runtime | distributed multi-runtime |
| Review | deterministic quality gates | independent A4, cross-provider where eligible |
| Repair | selective retry | automatic bounded A3→A4→repair |
| Routing | current runtime | Model Intelligence, failover, cost-to-accepted-result |
| Roles | GraphCoordinator | durable RUNTIME-A1/A2, ephemeral A3/A4 |

FREE is a real product, not a trial, and never depends on our licensing service being reachable.

## What the customer buys

Graph execution infrastructure, orchestration, model and runtime coordination, review and repair automation, routing intelligence, failure recovery, provenance, cross-host continuity.

**Not inference.** We do not sell, bundle, or resell model credits, and we never proxy customer work through our own provider accounts. The customer brings whatever provider access they are permitted to use; we coordinate it.

```
CUSTOMER ──pays──▶ provider A
         ──pays──▶ provider B
         ──pays──▶ OUR PRODUCT ──▶ orchestration software
```

## The problem being solved

Not "agents lie about tests" — that framing was abandoned in V1.2.3 and stays abandoned. The real problem is **state and continuity**:

> Multiple capable coding agents operate against different code states, sessions, providers, workspaces and quotas. A long-horizon goal outlives any one of them. Something durable must know what the plan is, what changed, what was executed, which exact code state each result applies to, and what remains — and must keep making progress when any executor disappears.

The graph is that durable thing. Chat context is a disposable cache; the repository, the state store and the graph are the source of truth.

## Success criterion

A developer hands the system a `SPEC.md` in either host and gets an inspectable graph that executes to completion — on FREE with one runtime and deterministic gates, or on PRO surviving a rate limit, a review rejection, a manager replacement and a host switch — without re-explaining the project, and without our licensing service being involved in FREE at all.

## What it never claims

It does not prove code correctness. It proves that declared checks and reviews executed against specific, exactly identified code states, and refuses to integrate when that provenance does not hold.
