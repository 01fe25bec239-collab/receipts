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

# NEW_SYSTEM_ARCHITECTURE

## 0. Terminology — two namespaces, never conflated

This architecture describes two distinct hierarchies. Confusing them is the single easiest way to misread every other document in this package.

| Namespace | What it is | Lives where |
|---|---|---|
| **RUNTIME-A1 / A2 / A3 / A4** | Roles the **finished product** creates inside the **user's** project (e.g. `RUNTIME-A2-AUTH`) | Product runtime |
| **BUILD-A1 / A2 / A3 / A4** | Roles **we** use to implement this orchestrator repository (e.g. `BUILD-A2-ORCHESTRATION`) | This repository's build control |

`RUNTIME-A2-AUTH` is a manager of the user's authentication feature. `BUILD-A2-RUNTIME-ADAPTERS` is a manager of our adapter source code. They are different systems with a coincidentally similar shape — the shape is reused deliberately, because we are dogfooding the methodology we are shipping.

Every prefixed term in this package is explicit. Unprefixed "A2" appears nowhere normative.

## 1. Layer model

```
┌──────────────────────────────────────────────────────────────────────┐
│ HOST LAYER            Claude Code  │  Codex                          │
│                       ClaudeHostAdapter │ CodexHostAdapter           │
│                       ── normalized host events ──                   │
├──────────────────────────────────────────────────────────────────────┤
│ ORCHESTRATION LAYER   Goal Orchestrator · RUNTIME-A1 engine          │
│                       RUNTIME-A2 engine · Global DAG · Scheduler     │
│                       Capsule factory · Concurrency · Budgets        │
│                       Global Goal Evaluator                          │
├──────────────────────────────────────────────────────────────────────┤
│ DECISION LAYER        Model Intelligence Service · Provider Registry │
│                       Model Registry · Capability lifecycle          │
│                       Router · Cost-to-acceptance estimator          │
│                       Availability & Quota manager                   │
├──────────────────────────────────────────────────────────────────────┤
│ EXECUTION LAYER       Runtime Adapters (Claude/Codex/Gemini/…)       │
│                       Credential Broker · Workspace Manager          │
│                       Git/worktree lifecycle · Process runner        │
│                       Checkpoints · Workspace recovery               │
├──────────────────────────────────────────────────────────────────────┤
│ ASSURANCE LAYER       A3→A4 controller · Review protocol             │
│                       Repair controller · Assurance profiles         │
│                       Security review pipeline · Safety interruption │
│                       Provenance · Integration gate                  │
├──────────────────────────────────────────────────────────────────────┤
│ STATE LAYER           Durable store · Logical role identity          │
│                       Executor bindings · Context manifests/epochs   │
│                       Event & audit log                              │
└──────────────────────────────────────────────────────────────────────┘
```

Dependencies point downward. The State layer depends on nothing above it; the Host layer depends on everything and is depended on by nothing. That ordering is what makes cross-host resume possible: swapping the top layer changes no state below it.

## 2. Control flow — one goal, end to end

```
user: START_GOAL(spec)
   │
   ├─ HostAdapter normalises → USER_GOAL_SUBMITTED
   │
   ▼
Goal Orchestrator ── persists Goal ──► State
   │
   ▼
RUNTIME-A1 (durable logical role; executor bound on demand)
   │  1. rehydrate context from repository + state (never from chat memory)
   │  2. inspect repository
   │  3. decompose → workstreams
   │  4. build Global DAG (cycle-checked)
   │  5. create RUNTIME-A2 logical roles
   │
   ▼
Scheduler ── eligible tasks ──► Router ── routing decision ──► Adapter
   │                               ▲
   │                               └── Model Intelligence (freshness-gated)
   ▼
RUNTIME-A2 (durable) → Task Capsule → Workspace Manager → worktree
   │
   ▼
RUNTIME-A3 (ephemeral, fresh session) ─ implement ─ checks ─ commit ─ handoff ─ TERMINATE
   │
   ▼   implementation SHA frozen
RUNTIME-A4 (ephemeral, fresh session) ─ audit exactly that SHA ─ verdict ─ TERMINATE
   │
   ├─ PASS ──────────────────► A2 acceptance gate ─► A2 integration branch
   │
   └─ REJECT ─► findings ─► A2 ─► Repair Capsule ─► fresh RUNTIME-A3 ─► loop (bounded)
                                       │
                                       └─ threshold exceeded ─► A2 escalation ─► A1 ─► HUMAN_REQUIRED
   │
   ▼
A1 integration gate ─► main
   │
   ▼
GLOBAL GOAL EVALUATOR (against original spec)
   │
   ├─ INCOMPLETE ─► mutate DAG ─► continue
   ├─ BLOCKED / HUMAN_REQUIRED ─► surface to user
   └─ COMPLETE ─► render (economy/template) ─► done
```

## 3. Load-bearing invariants

These are the statements the rest of the architecture is not permitted to contradict. Each is testable. They are **this product's** invariants (`I-n`), newly defined here — distinct from the historical Receipts invariants, which survive only where the reuse matrix says so.

> **Invariant numbering note (V1.1).** References of the form *invariant N* in this package follow the **17-item list in `orchestration/01_ARCHITECTURE_AUTHORITY.md`**, not the 10-item list in `Receipts_Final_Architecture.md` §C. Both exist in the committed repository; see `RECONCILIATION_REPORT_V1_1.md` R-01 for the mapping.


| # | Invariant |
|---|---|
| I-1 | A logical role (RUNTIME-A1, RUNTIME-A2) is **not** an LLM session. Identity, ownership, branch, decisions, and history survive executor replacement. |
| I-2 | RUNTIME-A3 and RUNTIME-A4 are **ephemeral**. A fresh session per implementation attempt and per audit. Resuming a long prior A3 conversation is not the default path. |
| I-3 | Chat context is a disposable cache. The repository plus the durable state store is the source of truth. |
| I-4 | No executor may be selected **solely** from an LLM's internal knowledge of model capability. Every material dispatch consults current Model Intelligence. |
| I-5 | A review is bound to an **exact** implementation SHA. A review of `abc` never validates `xyz`. |
| I-6 | A4 must be a fresh session and must not modify the implementation it reviews. |
| I-7 | Repair loops are **bounded** and escalate. No infinite autonomous loop. |
| I-8 | A3 cannot spawn A3. Discovered work becomes a `SUBTASK_REQUEST`; only A2/A1 mutate the authoritative DAG. |
| I-9 | Frontier-floor work is never silently downgraded to an economy model. If no eligible frontier executor exists: WAIT, BLOCK, ASK, or HUMAN_REQUIRED. |
| I-10 | Routing optimises **expected cost to an accepted result**, not cheapest invocation. |
| I-11 | Git worktrees are workspace isolation, **not** security isolation. |
| I-12 | The system must **never** provider-shop to circumvent a provider safety restriction. |
| I-13 | The economy renderer may present authoritative state but may never alter it. |
| I-14 | Task-level PASS ≠ goal COMPLETE. Only the Global Goal Evaluator decides completion, against the original specification. |
| I-15 | Claude Code and Codex have **behavioural parity**; implementation plumbing may differ. |
| I-16 | There is exactly **one** orchestrator core. Host adapters are shells. |
| I-17 | Model and provider names are configuration. No architectural branch depends on a specific model name. |
| I-18 | Worker agents cannot silently mutate orchestrator authority or state. |
| I-19 | Every integration decision is reconstructible from persisted events and provenance. |
| I-20 | The system never claims to prove code correctness. |

## 4. Source-of-truth boundaries

| Fact | Authoritative home | Never authoritative |
|---|---|---|
| Project/goal identity, DAG, task state | Durable state store | Any chat transcript |
| Code content and history | Git repository | Agent recollection |
| Which SHA was reviewed | Review record + git | A4's prose summary |
| Model capability | Model Intelligence registry (with provenance + freshness) | Executor training memory |
| Provider availability/quota | Availability manager (observed) | Assumption or vendor marketing |
| Integration decision | Integration gate record | An agent saying "done" |
| Goal completion | Global Goal Evaluator record | A renderer's summary |

## 5. Why this shape and not a simpler one

A simpler design — one session per project, agents spawned as subprocesses, state in the conversation — fails at exactly the moments this product exists for: the session ends, the provider rate-limits, the context compacts, the host changes. Every piece of durable machinery here (state store, logical roles, context epochs, capsules, provenance) is present because one of those events would otherwise destroy the project. `FAILURE_CRITERIA.md` records what would falsify that judgement.
