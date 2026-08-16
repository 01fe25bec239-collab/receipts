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

# BUILD_A2_DECOMPOSITION

## Namespace

These are **BUILD-A2** managers implementing *this orchestrator repository*. They are not the RUNTIME-A2 workstreams the finished product creates in a user's project.

## Result: 7 managers

| ID | Name | Owns |
|---|---|---|
| `BUILD-A2-ORCHESTRATION` | Orchestration Core | Goal orchestrator, logical-role engine, DAG, scheduler, capsules, concurrency, subtask control, budgets, goal evaluator |
| `BUILD-A2-MODEL-ROUTING` | Model Intelligence & Routing | Model Intelligence Service, registries, capability lifecycle, refresh, calibration, router, cost-to-acceptance, availability/quota |
| `BUILD-A2-RUNTIME-ADAPTERS` | Runtime Adapters & Credentials | Agent runtime adapters, credential broker, failure classification, harness path |
| `BUILD-A2-WORKSPACE-EXECUTION` | Workspace & Execution | Git/branch/worktree lifecycle, process execution, sandbox integration, checkpoints, crash recovery, execution evidence capture |
| `BUILD-A2-REVIEW-INTEGRATION` | Review, Assurance & Integration | A3→A4 controller, review protocol, repair controller, assurance profiles, provenance, integration gate, security review, safety interruption |
| `BUILD-A2-STATE-CONTEXT` | State, Identity & Context | Durable store, logical identity persistence, executor bindings, failover mechanics, context manifests/epochs, rehydration, event log |
| `BUILD-A2-HOST-INTEGRATION` | Host Integration & Parity | ClaudeHostAdapter, CodexHostAdapter, headless adapter, parity contract + conformance suite, normalized events, goal UX |

## Analysis performed (§86)

The hypothesis proposed six-to-seven managers. Each boundary was tested rather than copied.

### Merges considered and rejected

| Candidate merge | Rejected because |
|---|---|
| MODEL-ROUTING + RUNTIME-ADAPTERS | Tempting — the router picks what the adapters run. But routing is policy-and-evidence code with almost no I/O, while adapters are I/O-heavy integration code against volatile external CLIs. They change for entirely different reasons and at entirely different rates. Merging would put the most-churning code in the same manager as the most-invariant code. |
| ORCHESTRATION + STATE-CONTEXT | Orchestration is the biggest consumer of state, so the coupling is real. But durable role identity, context epochs, and rehydration form a distinct load-bearing subsystem, and ORCHESTRATION is already the largest manager. Merging produces one manager owning roughly half the system. |
| WORKSPACE-EXECUTION + RUNTIME-ADAPTERS | Both sit at the "execution edge". But workspace/git is host- and provider-neutral, while adapters are provider-specific. The interface between them (`WorkspaceHandle`) is narrow and clean — a good sign the split is real. |
| HOST-INTEGRATION into ORCHESTRATION | Host parity is a distinct deliverable with two adapters plus a conformance suite that gates release. It needs an owner who can block on parity failure. |
| REVIEW-INTEGRATION split into REVIEW + SECURITY | Security review is a *type* of review sharing the same dispatch, findings, and gate machinery. Splitting would duplicate the loop. |

### Splits considered and rejected

| Candidate split | Rejected because |
|---|---|
| ORCHESTRATION → scheduler / goal-evaluator | The evaluator's inputs are the DAG and integration state; splitting creates a chatty boundary over shared data. |
| MODEL-ROUTING → intelligence / router | The router is the only consumer of the registry; the boundary would be internal. |

### The one genuinely close call

**MODEL-ROUTING vs RUNTIME-ADAPTERS.** Kept separate primarily on *rate of change*: adapter code must track external CLI drift (A-05, A-15 are `UNVERIFIED` precisely because these surfaces move), while routing policy should be stable. A single manager owning both would have its stable policy work continually interrupted by adapter breakage. Revisit if adapter work turns out smaller than expected.

## Why 7, not 5

Five would require two of the rejected merges. Seven is within the §85 range of 5–7, and each manager owns substantial runtime code with a defensible boundary. The count is not maximised — it is the smallest topology where no manager owns two subsystems that change for unrelated reasons.

## No documentation-only BUILD-A2 (§87)

**None exists, and none is proposed.**

Documentation belongs to the engineering manager that owns the subsystem it describes, and is produced as economy BUILD-A3 tasks. Reasons: documentation correctness is inseparable from the code it describes; a docs manager would own no runtime code and no integration boundary, making it a manager in name only; and separating docs from code is how documentation drifts.

Architecture and security documentation that encodes a decision routes at frontier quality, because there the reasoning *is* the artifact.

## Safe parallelism

```
Wave 1   STATE-CONTEXT ──────────► foundation for everything
Wave 2   ORCHESTRATION  ·  WORKSPACE-EXECUTION  ·  RUNTIME-ADAPTERS   (parallel)
Wave 3   MODEL-ROUTING  ·  REVIEW-INTEGRATION                          (parallel)
Wave 4   HOST-INTEGRATION ─────► parity suite gates release
```

Dependencies in `BUILD_A2_DEPENDENCY_MATRIX.md`. No two managers share a write path (`BUILD_A2_OWNERSHIP_MATRIX.md`).

## Status

**DEFINED ONLY.** Not initialized, not authorized to implement.
