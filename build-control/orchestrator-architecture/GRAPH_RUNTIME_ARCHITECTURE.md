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

# GRAPH_RUNTIME_ARCHITECTURE

## Layer model (V1.2.3 layering, graph-native)

```
HOST LAYER        Claude plugin shell │ Codex plugin shell │ headless
                  HostCapabilityDiscovery → NormalizedHostEvent
─────────────────────────────────────────────────────────────────────
GRAPH LAYER       GraphCompiler · ExecutionGraph · GraphScheduler
                  GraphMutation · GraphSnapshot · Goal Evaluator
─────────────────────────────────────────────────────────────────────
ADMISSION LAYER   FeatureAdmission (entitlement) · ProviderPolicyEligibility
                  quality floor · safety state
─────────────────────────────────────────────────────────────────────
DECISION LAYER    Model Intelligence · Router · cost-to-accepted-result
                  availability / quota
─────────────────────────────────────────────────────────────────────
EXECUTION LAYER   Runtime adapters · credential broker · workspaces
─────────────────────────────────────────────────────────────────────
ASSURANCE LAYER   A3→A4 · repair · provenance · integration gate
─────────────────────────────────────────────────────────────────────
STATE LAYER       SQLite · graph persistence · entitlement cache · events
```

**The admission layer is new in V1.3** and sits deliberately *above* routing. A node must be admitted before a runtime is even considered — otherwise a FREE user's Pro request could consume a routing decision, or worse, a provider call, before being refused.

## Shared local core process model

Three options were evaluated (§14):

| | A. On-demand CLI | B. Long-lived daemon | C. Hybrid |
|---|---|---|---|
| Cross-host resume | via state store | via shared process | via state store |
| Concurrent hosts | natural | needs arbitration | natural |
| SQLite writer ownership | one writer per invocation, lease-arbitrated | single writer, simple | lease-arbitrated |
| Crash recovery | trivial — nothing to orphan | needs supervision | moderate |
| Plugin lifecycle | independent | coupled | mostly independent |
| Install/update | simple | service management | moderate |
| Idle resource use | zero | continuous | near zero |
| Stale process cleanup | none needed | required | required |

**Chosen for MVP: A — on-demand local core invocation.**

Reasoning: long-horizon orchestration *sounds* like it needs a daemon, but the durability requirement is already met by the state store, not by process residency. Every V1.2.3 mechanism — durable roles, lease arbitration, checkpointing, crash recovery — exists precisely so that no process needs to stay alive. A daemon would add supervision, stale-process cleanup, idle footprint and update complexity to solve a problem already solved.

§14 explicitly warns against choosing a daemon because "orchestrators use daemons". Option C remains the upgrade path if measured latency or long-poll requirements justify it; the core interface is identical either way.

## Free and Pro use the same core

One binary, one graph engine, one scheduler. The Pro module adds distributed policy implementations that the core loads **only** when admission grants the relevant capability.
