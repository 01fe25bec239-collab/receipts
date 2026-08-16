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

# BUILD-A2-RUNTIME-ADAPTERS

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-RUNTIME-ADAPTERS` — Runtime Adapters & Credentials.

## Mission
Implement the layer that actually drives external coding-agent runtimes: the `RuntimeAdapter` interface, per-runtime adapters, capability probing, failure classification, the API/harness path, and the provider credential broker.

## Why long-lived
This is the product's boundary with the outside world, and the fastest-changing code in the repository. External CLI surfaces drift (A-05 and A-15 are `UNVERIFIED` precisely because of it), so a durable owner is needed to absorb churn without letting it leak into routing or orchestration.

## Owned subsystem
`RuntimeAdapter` interface and conformance suite · Claude Code CLI adapter · Codex CLI adapter · Gemini CLI adapter · verify-before-production adapters · API/harness path · capability probing · `classifyFailure` normalisation · credential broker · `PERSONAL_LOCAL_MODE` connection handling.

## Owned repository paths
`src/adapters/runtime/**` · `src/adapters/credentials/**` · owned schemas · **`docs/runtime-adapters/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`Provider` · `RuntimeAdapter`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `WorkspaceHandle` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `TaskCapsule` | `BUILD-A2-ORCHESTRATION` |
| `RepairCapsule` | `BUILD-A2-ORCHESTRATION` |
| `ReviewCapsule` | `BUILD-A2-REVIEW-INTEGRATION` |
| `DispatchAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |


## Reference-only
`ROUTING_DECISION`, `AVAILABILITY_STATE`, `ASSURANCE_PROFILE`

## Forbidden ownership
Routing decisions · registry contents (it supplies probes; MODEL-ROUTING owns the registry) · workspace/git lifecycle · review · host adapters · state internals.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.

**Build wave: W2** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `WorkspaceHandle` — owned by `BUILD-A2-WORKSPACE-EXECUTION`; frozen at M0.
- `TaskCapsule` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `RepairCapsule` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `ReviewCapsule` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `DispatchAdmissionDecision` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-WORKSPACE-EXECUTION` — execute inside worktree
- ↔ `BUILD-A2-MODEL-ROUTING` — AvailabilityState, QuotaState
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories
Adapter interface + conformance suite · Claude Code adapter (`claude -p`, JSON/stream-JSON, flag probing) · Codex adapter (`codex exec`, JSONL, `--output-schema`, sandbox modes) · Gemini adapter · failure classification per runtime · read-only enforcement for reviewers · credential broker with delegated auth · keychain storage · `auth_status`/`health`/`models` implementations · harness path for API-only providers.

## Expected BUILD-A4 review categories
**Read-only enforcement proven against a real runtime, not a fake** · no credential in any log, event, capsule, or error · failure classification never guesses (`UNKNOWN` preserved) · `SAFETY_CHECK_PENDING` never conflated with `RATE_LIMITED` · deprecated flags never used (`--full-auto`) · no shell-string execution · adapter contains no orchestration logic.

## Frontier / economy policy
Frontier for the interface, credential broker, failure classification, and read-only enforcement. Economy for adapter usage documentation.

## Security responsibility
**The highest-risk manager.** Owns credential handling and the process boundary to external agents. Every credential rule in `SECURITY_TRUST_MODEL.md` is implemented here and must pass REVIEW-INTEGRATION's security acceptance tests.

## Integration responsibility
Adding an adapter must require **zero** core changes. If a core file changes to add an adapter, the abstraction is wrong and the task is rejected.

## Context requirements
Initial: adapter interface, credential architecture, both mode documents, `ASSUMPTION_REGISTER` A-01…A-31. Rehydration: mandatory whenever an external CLI fact is re-verified.

## Non-goals
Does not choose models · does not own the registry · does not create worktrees · does not review · does not implement `PRODUCT_TEAM_MODE` in MVP.

## First proposed milestone
`M-ADAPT-1`: adapter interface + conformance suite + Claude Code and Codex adapters passing it, with delegated credential handling and full failure classification.
