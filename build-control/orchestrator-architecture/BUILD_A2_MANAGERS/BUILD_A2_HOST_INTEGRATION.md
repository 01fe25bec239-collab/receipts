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

# BUILD-A2-HOST-INTEGRATION

This manager owns HostCapabilityReport and the single freshness authority, `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`; it creates no second capability contract.

**Namespace:** BUILD-control.

## Identity
`BUILD-A2-HOST-INTEGRATION` — Host Integration & Parity.

## Mission
Implement both first-class hosts and prove they are equivalent: `ClaudeHostAdapter`, `CodexHostAdapter`, the headless adapter, the normalized host event bridge, the goal UX on both hosts, and the parity conformance suite that gates release.

## Why long-lived
Host parity is a standing commitment, not a one-time port. Both host surfaces evolve independently, and someone must continuously prove that a capability added on one exists on the other. This manager holds release-blocking authority on parity.

## Owned subsystem
`HostAdapter` interface · ClaudeHostAdapter (plugin manifest, hooks, skills, commands, `WorktreeCreate` handler, fallback path) · CodexHostAdapter — **native plugin shell is primary** (`.codex-plugin/plugin.json`, hooks, skills), including hook trust/enablement/admin-policy detection and `HostCapabilityReport` probing/mode-selection; supervised/hybrid compatibility fallback (companion process, `codex exec` driving, config/`AGENTS.md`, optional MCP bridge) selected only when the native path cannot safely operate · headless adapter · normalized host event bridge with `OBSERVED`/`INFERRED` marking · `START_GOAL` UX on both hosts · view rendering (template and economy) · parity conformance suite.

`codex exec` remains a worker/runtime dispatch mechanism used whenever a Codex *worker* is dispatched (see RUNTIME-ADAPTERS); it is never itself the primary Codex host-integration path — no future implementer reading only this charter may conclude Codex is supervisor-primary.

## Owned repository paths
`src/hosts/**` · `plugins/claude/**` · `plugins/codex/**` (native, primary) · `integrations/codex-fallback/**` (compatibility fallback) · `tests/parity/**` · owned schemas · **`docs/host-integration/**`** (this manager's documentation directory — and no other part of `docs/`).

## Owned contracts

**NORMATIVE — generated from the canonical ownership map** (`CONTRACT_CONSUMPTION_GRAPH.md`). This is the single authoritative owned-contract list for this manager.

`HostAdapter` · `HostCapabilityReport` · `HostParity` · `NormalizedHostEvent`

This manager never lists any of the above as a consumed dependency — using one's own contract is not a dependency.

### [HISTORICAL] V1.2 ownership snapshot — NON-NORMATIVE

Retained for provenance only. Superseded by the normative list above; do not use for implementation authority.

—


## Consumed contracts

Externally owned only.

| Contract | Owner |
|---|---|
| `RoutingDecision` | `BUILD-A2-MODEL-ROUTING` |
| `IntegrationDecision` | `BUILD-A2-REVIEW-INTEGRATION` |
| `A4Review` | `BUILD-A2-REVIEW-INTEGRATION` |
| `GoalEvaluation` | `BUILD-A2-ORCHESTRATION` |
| `LogicalRole` | `BUILD-A2-STATE-CONTEXT` |
| `WorkspaceHandle` | `BUILD-A2-WORKSPACE-EXECUTION` |
| `ExecutionGraph` | `BUILD-A2-ORCHESTRATION` |
| `GraphSnapshot` | `BUILD-A2-ORCHESTRATION` |
| `FeatureCapabilitySet` | `BUILD-A2-ORCHESTRATION` |
| `FeatureAdmissionDecision` | `BUILD-A2-ORCHESTRATION` |
| `ProductEntitlement` | `BUILD-A2-STATE-CONTEXT` |
| `ActivationState` | `BUILD-A2-STATE-CONTEXT` |


## Reference-only
`TASK_CAPSULE`, `PROVENANCE`

## Forbidden ownership
**Any orchestration logic.** No DAG, routing, role lifecycle, repair, or gate logic may live in an adapter. Also forbidden: state internals, adapter-to-provider worker code (that is RUNTIME-ADAPTERS), workspace internals.

## HARD_BUILD_DEPENDENCIES

Concrete implementation of another manager is required before this one can be implemented. These edges form the acyclic `BUILD_IMPLEMENTATION_DAG`.

- `BUILD-A2-STATE-CONTEXT` — **concrete implementation required.** Needs the real state repository; nothing durable can be stubbed honestly.
- `BUILD-A2-ORCHESTRATION` — **concrete implementation required.** Needs real core operations to drive from a host surface.

**Build wave: W3** of 3.

## FROZEN_CONTRACT_DEPENDENCIES

Owned elsewhere, frozen at M0. Identical to *Consumed contracts* by construction.

- `RoutingDecision` — owned by `BUILD-A2-MODEL-ROUTING`; frozen at M0.
- `IntegrationDecision` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `A4Review` — owned by `BUILD-A2-REVIEW-INTEGRATION`; frozen at M0.
- `GoalEvaluation` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `LogicalRole` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `WorkspaceHandle` — owned by `BUILD-A2-WORKSPACE-EXECUTION`; frozen at M0.
- `ExecutionGraph` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `GraphSnapshot` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `FeatureCapabilitySet` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `FeatureAdmissionDecision` — owned by `BUILD-A2-ORCHESTRATION`; frozen at M0.
- `ProductEntitlement` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.
- `ActivationState` — owned by `BUILD-A2-STATE-CONTEXT`; frozen at M0.


## RUNTIME_INTERACTIONS

How this manager collaborates at run time. **Bidirectional interaction here does not imply a build dependency.**

- ↔ `BUILD-A2-ORCHESTRATION` — NormalizedHostEvent, START_GOAL
- ↔ `BUILD-A2-STATE-CONTEXT` — persist and read durable state.


## Expected BUILD-A3 task categories
`HostAdapter` interface · host detection with explicit override · Claude plugin packaging (manifest, hooks, skills, command) · Claude hook → normalized event mapping · **`WorktreeCreate` handler with tested fallback** · Claude flag probing at install · **Codex native plugin shell** (`.codex-plugin/plugin.json`, `hooks/hooks.json`, skills) and Codex hook → normalized event mapping · Codex hook trust/enablement/admin-policy detection · `HostCapabilityReport` probing and mode selection (EMBEDDED/HYBRID/SUPERVISED) · supervised/hybrid compatibility fallback (companion process, `codex exec` driving and JSONL parsing) with `INFERRED` marking · `START_GOAL` on both hosts · headless adapter · template renderer (zero-model) · economy renderer with no write path · parity conformance suite with drivers for both hosts.

## Expected BUILD-A4 review categories
**Dependency-direction test: core does not depend on any adapter** · no orchestration logic in adapters · every parity row tested on both hosts with no skips · `INFERRED` events correctly marked · `WorktreeCreate` handler is correct, fast, and falls back cleanly · renderer cannot mutate authoritative state (I-13) · no claim of enforcement or capability the host does not provide · no credential handling in adapters.

## Frontier / economy policy
Frontier for the event bridge, the `WorktreeCreate` handler, and the parity suite. Economy for installation documentation.

## Security responsibility
The host bridge is a trust boundary: host input is untrusted, and adapters must not become a path for host content to reach state or commands. Renderer restriction is enforced here.

## Integration responsibility
**Holds release-blocking authority on host parity.** May block a release when a parity row fails, and may not resolve a gap by narrowing the capability set without a recorded decision.

## Context requirements
Initial: host parity contract, both adapter documents, normalized events, cross-host resume, `ASSUMPTION_REGISTER` A-01…A-15. Rehydration: **mandatory whenever a host capability fact is re-verified** — this manager depends on the most volatile external surfaces in the product.

## Non-goals
Does not orchestrate · does not route · does not implement worker runtime adapters · does not own state · does not decide acceptance.

## First proposed milestone
`M-HOST-1`: `HostAdapter` interface + normalized event bridge + `START_GOAL` on both hosts + parity rows P-01…P-06 passing on both.
