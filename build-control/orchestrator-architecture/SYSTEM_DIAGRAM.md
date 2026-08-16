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

# SYSTEM_DIAGRAM

## Whole system

```
                          ┌──────────────┐
                          │     USER     │
                          └──────┬───────┘
                 ┌───────────────┴───────────────┐
                 ▼                               ▼
        ┌────────────────┐              ┌────────────────┐
        │  Claude Code   │              │     Codex      │
        │  (FIRST_CLASS) │              │  (FIRST_CLASS) │
        └───────┬────────┘              └───────┬────────┘
                ▼                               ▼
     ┌─────────────────────┐         ┌─────────────────────┐
     │ ClaudeHostAdapter   │         │  CodexHostAdapter   │
     │ plugin·hooks·skills │         │ plugin·hooks·skills │
     │ commands·subagents  │         │ config·exec·companion│
     │                     │         │ (fallback only)     │
     └──────────┬──────────┘         └──────────┬──────────┘
                └───────── normalized host events ─────────┐
                                    ▼                      │
   ╔══════════════════════════════════════════════════════════════════╗
   ║                     SHARED ORCHESTRATOR CORE                     ║
   ║                                                                  ║
   ║  ┌────────────────────────────────────────────────────────────┐  ║
   ║  │ Goal Orchestrator → RUNTIME-A1 → RUNTIME-A2(s)             │  ║
   ║  │ Global DAG · Scheduler · Capsules · Concurrency · Budgets  │  ║
   ║  └───────────────┬────────────────────────────────────────────┘  ║
   ║                  ▼                                               ║
   ║  ┌────────────────────────────────────────────────────────────┐  ║
   ║  │ Model Intelligence ─► Router ─► routing decision           │  ║
   ║  │ Provider/Model Registry · Capability lifecycle             │  ║
   ║  │ Availability & Quota · Cost-to-acceptance estimator        │  ║
   ║  └───────────────┬────────────────────────────────────────────┘  ║
   ║                  ▼                                               ║
   ║  ┌────────────────────────────────────────────────────────────┐  ║
   ║  │ Runtime Adapters   Credential Broker   Workspace Manager   │  ║
   ║  │ Claude·Codex·Gemini·…      git worktrees · checkpoints     │  ║
   ║  └───────────────┬────────────────────────────────────────────┘  ║
   ║                  ▼                                               ║
   ║  ┌────────────────────────────────────────────────────────────┐  ║
   ║  │ A3→A4 controller · Repair controller · Assurance profiles  │  ║
   ║  │ Security review · Safety interruption · Provenance         │  ║
   ║  │ Integration gate · Global Goal Evaluator                   │  ║
   ║  └───────────────┬────────────────────────────────────────────┘  ║
   ║                  ▼                                               ║
   ║  ┌────────────────────────────────────────────────────────────┐  ║
   ║  │ DURABLE STATE STORE  +  EVENT / AUDIT LOG                  │  ║
   ║  │ roles · bindings · DAG · attempts · reviews · contexts     │  ║
   ║  └────────────────────────────────────────────────────────────┘  ║
   ╚══════════════════════════════════════════════════════════════════╝
                                    │
                 ┌──────────────────┼──────────────────┐
                 ▼                  ▼                  ▼
        ┌────────────────┐ ┌────────────────┐ ┌────────────────┐
        │ RUNTIME-A3 #1  │ │ RUNTIME-A3 #2  │ │ RUNTIME-A3 #3  │
        │ provider P     │ │ provider Q     │ │ provider R     │
        │ worktree wt-1  │ │ worktree wt-2  │ │ worktree wt-3  │
        └───────┬────────┘ └───────┬────────┘ └───────┬────────┘
                ▼                  ▼                  ▼
        ┌────────────────┐ ┌────────────────┐ ┌────────────────┐
        │ RUNTIME-A4 #1  │ │ RUNTIME-A4 #2  │ │ RUNTIME-A4 #3  │
        │ fresh · read-only · audits exact SHA                  │
        └────────────────┴────────────────┴─────────────────────┘
```

## Durable vs ephemeral

```
DURABLE (survive everything)          EPHEMERAL (created, used, destroyed)
───────────────────────────           ──────────────────────────────────
Project / Goal                        RUNTIME-A3 session
RUNTIME-A1 logical role               RUNTIME-A4 session
RUNTIME-A2 logical roles              Executor binding (role ↔ model/session)
Global DAG + tasks                    Host session
Workspaces / branches                 Chat context of any role
Attempts, reviews, findings           Rendered user-facing prose
Context manifests + epochs
Event log
```

A vertical line separates what a crash may destroy from what it may not. Everything in the left column is reconstructible from disk; everything in the right column is expendable by design.

## Git topology (runtime, user's project)

```
main
├── runtime-a2/auth
│   ├── runtime-a3/auth-001        ← ephemeral, one attempt
│   └── runtime-a3/auth-002-r1     ← repair attempt, new session
├── runtime-a2/payments
│   ├── runtime-a3/pay-001
│   └── runtime-a3/pay-002
└── runtime-a2/ui
    └── runtime-a3/ui-001
```

A4 never checks out an A3 worktree; it reads an exact commit.
