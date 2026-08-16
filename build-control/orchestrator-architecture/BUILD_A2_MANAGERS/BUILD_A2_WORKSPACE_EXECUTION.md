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

# BUILD-A2-WORKSPACE-EXECUTION

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-WORKSPACE-EXECUTION` — Workspace & Execution.

## Mission
Implement isolated, recoverable execution environments: branch and worktree lifecycle, process execution, sandbox integration, checkpointing, crash and dirty-workspace recovery, write-scope verification, and execution evidence capture.

## Why long-lived
Workspace isolation and recovery are what make parallel agents safe and long-horizon work survivable. The rules here (argv-only execution, no shell strings, post-hoc diff verification, never auto-accept partial work) are easy to erode under convenience pressure and need a durable defender.

## Owned subsystem
Git branch lifecycle · worktree provisioning and teardown · `WorkspaceHandle` · process execution (explicit argv, realpath cwd, allowlisted env, orchestrator-owned timeouts) · output capture and digesting · checkpoints · crash/orphan recovery · write-scope verification · execution evidence capture · remote branch policy.

## Owned repository paths
`src/workspace/**` · `src/execution/**` · owned schemas · **`docs/workspace-execution/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`WorkspaceCheckpoint` · `WorkspaceHandle`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `TaskCapsule` | `BUILD-A2-ORCHESTRATION` |
| `GraphNode` | `BUILD-A2-ORCHESTRATION` |


## Reference-only
`RUNTIME_ADAPTER`, `PROVENANCE`, `ASSURANCE_PROFILE`

## Forbidden ownership
Adapter code · routing · review verdicts and gates · state internals · host adapters (though it owns the *semantics* the `WorktreeCreate` handler must implement).

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.

**Build wave: W2** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `TaskCapsule` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `GraphNode` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-ORCHESTRATION` — WorkspaceHandle, checkpoint events
- ↔ `BUILD-A2-RUNTIME-ADAPTERS` — WorkspaceHandle
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories
Branch lifecycle · worktree provisioning/teardown · `WorkspaceHandle` · pre-flight verification · argv process runner with timeout (terminate → bounded kill) · bounded separate stdout/stderr capture with digests · checkpoint writer · orphan detection · recovery record capture · recovery decision matrix · write-scope diff verification · remote branch policy enforcement.

## Expected BUILD-A4 review categories
**No shell-string execution anywhere, including in fixtures** · timeout leaves no orphaned process · recovery never auto-accepts partial work · capture happens before cleanup · write-scope verification cannot be bypassed · worktree never described or treated as a security sandbox · force-push impossible.

## Frontier / economy policy
Frontier for process execution, timeout/cancellation, and recovery. Economy for workspace documentation.

## Security responsibility
Implements the execution boundary: sandbox integration, environment allowlisting, argv-only launching, and the post-hoc write-scope check that per A-08 is the layer that actually holds. Must pass REVIEW-INTEGRATION's security acceptance tests.

## Integration responsibility
Supplies the evidence that acceptance depends on — exact SHAs, diffs, and check results bound to code state.

## Context requirements
Initial: workspace architecture, recovery, remote policy, `WORKSPACE`/`CHECKPOINT` contracts, security trust model. Rehydration: on adapter interface change, on ADR/worktree semantics change.

## Non-goals
Does not choose models · does not review · does not decide acceptance · does not own host hooks (it owns what the handler must do, not the plumbing).

## First proposed milestone
`M-WORK-1`: branch/worktree lifecycle + `WorkspaceHandle` + argv process runner with timeouts and bounded capture + write-scope verification.
