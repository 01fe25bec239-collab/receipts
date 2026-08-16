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

# CROSS_HOST_RESUME

On host entry, `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json` requires host/install-specific cache lookup, freshness verification or re-probe, then mode selection; graph state remains shared and is never reset.

## Requirement

A goal started in Claude Code resumes in Codex, and vice versa, without rebuilding the project.

## Why it is nearly free

Cross-host resume is not a feature bolted on top; it is a **consequence of the layering**. Because no orchestration state lives in a host, and no host adapter contains orchestration logic, switching hosts changes only which shell is attached to the core.

If cross-host resume were hard to implement, that would be evidence the layering had been violated somewhere — so this document doubles as an architectural test.

## What is preserved

| Preserved | Where it lives |
|---|---|
| **ExecutionGraph + graph_version** | state store |
| **Product entitlement** | user/install-scoped, host-neutral |
| Project + goal identity | state store |
| RUNTIME-A1 logical role | state store |
| RUNTIME-A2 logical roles + ownership | state store |
| Global DAG, tasks, dependencies | state store |
| Branches and worktrees | git + state store |
| Accepted decisions, findings, reviews | state store |
| Model observations and calibration | state store |
| Checkpoints | filesystem + state store |
| Context manifests + current epoch | state store + repository |
| Next actions | derived from DAG |

## What is discarded

| Discarded | Why |
|---|---|
| Host session ID | Host-scoped by definition |
| Executor bindings for A1/A2 | Rebound on the new host |
| Any in-flight A3/A4 session | Ephemeral by design (I-2) |
| Chat context | Disposable cache (I-3) |

An in-flight A3 at the moment of a host switch is treated exactly like a crash: `WORKSPACE_RECOVERY.md` applies, its partial work is inspected rather than trusted, and no partial state is silently accepted as complete.

## Flow

```
Claude Code            state store            Codex
─────────────────────────────────────────────────────────
GOAL-17 running   →    persisted        
host unavailable  →    (unchanged)      
                                         user opens Codex
                                         CodexHostAdapter starts
                                         discovers project
                                         binds A1 executor
                                         MANDATORY rehydration
                                         epoch check
                                         resume GOAL-17
```

## Mandatory rehydration on host switch

Host switch is a **mandatory rehydration trigger** (§62). The newly bound executor rereads authoritative artifacts — it never receives a summary written by the previous host's executor. Replaying a summary would let the new executor inherit the old one's misreadings, which is exactly the failure rehydration exists to prevent.

## Concurrent hosts

Two hosts may be open at once. Rules:

1. **One active A1 executor binding per project.** Enforced by a lease in the state store.
2. The second host attaches **read-only** and displays status.
3. The user may explicitly transfer the lease; the previous binding is released and recorded.
4. Lease expiry is time-bounded so a crashed host does not lock a project forever.

Two authoritative A1 executors would produce divergent DAG mutations — this mirrors the "never two authoritative A1 agents" rule the build-control methodology already uses.

## Validation

Scenarios S3 (Claude → Codex) and S4 (Codex → Claude) in `SCENARIO_VALIDATION.md`, plus parity row P-18.

## Graph and entitlement on host switch (V1.3)

A resumed host loads the **same `graph_id` at the same `graph_version`**, verifies the version digest, and resolves the **same user-local entitlement**. No conversion, no new project identity, no second purchase.

Entitlement is install-scoped, not project-scoped, so it never enters a repository. The graph is project-scoped, so it never enters the entitlement store. That boundary is deliberate (§67).
