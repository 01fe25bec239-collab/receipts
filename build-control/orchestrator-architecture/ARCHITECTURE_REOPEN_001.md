<!--
MultiAgent Orchestrator Architecture — HISTORICAL SNAPSHOT
DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT
SNAPSHOT: V1.3
Issued by: BUILD-A1-BOOTSTRAP
Status: PRESERVED HISTORICAL RECORD — NOT current architectural truth.
The original reopen record.
This document records what was believed at the time it was written. Where it
disagrees with a CURRENT_NORMATIVE document, the current document governs.
It contributes NO current evidence assertion to normative validation.
-->

# ARCHITECTURE_REOPEN_001

**Type:** Formal architecture reopen
**Raised by:** BUILD-A1-BOOTSTRAP
**Date:** 2026-08-13
**Status:** OPEN — candidate for external review
**Reason:** **Product-goal mismatch discovered before implementation.**

## What is being reopened

The historical product, *Receipts*, was defined around the pipeline `CLAIM → EVIDENCE → POLICY → ADMISSION`. Its architecture explicitly and repeatedly stated that Receipts was **not** a multi-agent orchestrator — invariant 14 rejected an agent-runtime architecture, and invariant 15 constrained `ReviewProvider` to four operations precisely to prevent it drifting into one.

The product now desired is materially different: a host-parity, multi-provider **coding orchestration control plane**. Evidence and provenance survive, but as an internal integration subsystem rather than the product thesis.

This is not a refinement of the old architecture. A design whose central invariant is "we are not an orchestrator" cannot be incrementally reshaped into an orchestrator. Attempting that would produce an architecture that contradicts its own frozen invariants — which is exactly the failure the deviation protocol exists to prevent. So the product definition is reopened formally, and the old architecture is preserved as historical evidence rather than quietly reinterpreted.

## Timing

Reopening happens **before any implementation**. No product source code, no dependency, no runtime branch, no worktree, and no A3 task ever existed. The cost of this reopen is therefore documentation and planning only. That is the cheapest moment this could possibly have happened, and it is worth stating plainly: the previous three phases produced a rigorous, internally consistent plan for the wrong product.

## What is NOT being discarded

The historical work retains substantial value:

- the discipline of exact code-state binding and staleness;
- the separation of deterministic from probabilistic evidence;
- append-only event history with a hash chain;
- the A1/A2/A3/A4 build-control methodology itself, which is being reused as **BUILD-control** and simultaneously promoted into the **product runtime**;
- the honesty constraints on claims, enforcement scope, and measurement.

See `OLD_RECEIPTS_REUSE_MATRIX.md` for the item-by-item disposition and `ADR_IMPACT_MATRIX.md` for ADR-001.

## Artifact classification

| Artifact | Classification | Note |
|---|---|---|
| `Receipts_Final_Architecture.md` | **HISTORICAL_ONLY** | Preserved at `CONTRACT_FREEZE_SHA`. Not authority for the new product. Specific mechanisms are reused via the reuse matrix, not by reference. |
| 21 frozen contracts (1.0.0) | **Mixed — see `CONTRACT_IMPACT_MATRIX.md`** | Roughly: fingerprint/evidence/ledger concepts REVISED; runner/plugin/policy/review contracts SUPERSEDED or RETIRED. |
| `ARCHITECTURE_DEVIATION_REQUEST_001` (ADR-001) | **SUPERSEDED (in part)** | Its *conclusion* is reversed by a changed product boundary; its *underlying reasoning* is retained. See `ADR_IMPACT_MATRIX.md`. |
| 16 orchestration control files (`orchestration/00`–`15`) | **REUSED** | The build-control methodology is sound and is carried forward for BUILD-control. Content referring to Receipts product semantics is HISTORICAL_ONLY. |
| Five-A2 definition package (`build-control/a2/**` @ `A2_DEFINITION_SHA`) | **SUPERSEDED** | Superseded by `BUILD_A2_DECOMPOSITION.md`. The A1-BOOTSTRAP → A1-RUNTIME succession model, the three-baseline model, and the context-minimisation model are **REUSED**. |
| Execution-control package (partially drafted, uncommitted) | **RETIRED** | Was being written against the old task DAG. Its protocol shapes (A3 capsule, A4 verdicts, repair loop, integration gate) are **REUSED** and promoted into product runtime specifications. |
| `schemas/SCHEMA_PLAN.md` | **REVISED** | Dialect and canonicalisation approach REUSED; the schema set itself is replaced. |
| `MANIFEST.sha256` files, git history | **STILL_AUTHORITATIVE** | Repository history is evidence. Nothing is deleted or rewritten. |

## Constraints on this reopen

1. **No history deletion.** Nothing at `CONTRACT_FREEZE_SHA` or `A2_DEFINITION_SHA` is removed or rewritten.
2. **No silent reinterpretation.** Where the new architecture contradicts the old, `CONFLICT_RESOLUTION_LOG.md` records the contradiction and the resolution.
3. **No fabricated freeze.** `NEW_ARCHITECTURE_FREEZE_SHA` is **NOT ASSIGNED** and will not be assigned by this package.
4. **No implementation authorisation.** This package defines BUILD-A2 managers; it does not initialise them.
5. **No fabricated external facts.** Volatile provider facts are labelled per `ASSUMPTION_REGISTER.md`.

## Disposition requested

External review of this candidate package, then either: acceptance with a subsequent freeze commit, or a further reopen. BUILD-A1-BOOTSTRAP does not self-approve an architecture reopen of this magnitude.
