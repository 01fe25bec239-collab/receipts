<!--
MultiAgent Orchestrator Architecture — HISTORICAL SNAPSHOT
DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT
SNAPSHOT: V1.3
Issued by: BUILD-A1-BOOTSTRAP
Status: PRESERVED HISTORICAL RECORD — NOT current architectural truth.
Records the V1.3 reopen as raised, including the then-unresolved §7 research gap that V1.3.1 closed.
This document records what was believed at the time it was written. Where it
disagrees with a CURRENT_NORMATIVE document, the current document governs.
It contributes NO current evidence assertion to normative validation.
-->

# ARCHITECTURE_REOPEN_002 — GRAPH + COMMERCIAL

**Type:** Controlled structural reopen of an accepted, **not yet installed** candidate
**Baseline:** V1.2.3 · **Date:** 2026-08-15 · **Status:** CANDIDATE

## Why this is a reopen and not a redesign

V1.2.3 was structurally sound and passed independent review on every gate. Four new load-bearing requirements arrived **before installation**, so no freeze needs rolling back and no history is disturbed. `NEW_ARCHITECTURE_FREEZE_SHA` was never assigned.

The four:

1. **ExecutionGraph becomes first-class.** The DAG was an internal scheduling structure; it becomes the authoritative, durable, versioned product artifact.
2. **FREE and PRO tiers** must exist without two orchestration engines.
3. **Product entitlement** becomes a first-class concern, strictly separate from provider authentication.
4. **Host integration** must be re-evaluated against current plugin capabilities and provider policy.

Everything else in V1.2.3 is preserved. The seven BUILD-A2 managers, the hard DAG, the durable/ephemeral role split, exact-SHA review, Model Intelligence, safety handling — all carried forward.

## What actually changed, structurally

| Change | Nature |
|---|---|
| `TaskDag` → `ExecutionGraph` | **Promotion.** Same scheduling semantics, now a durable versioned artifact with an audit trail |
| One engine, two policies | **New.** `GraphExecutionPolicy` selects FREE single-runtime or PRO distributed dispatch over the *same* graph |
| Product entitlement | **New concern.** Four-way separation: entitlement / technical auth / policy eligibility / availability |
| Host posture | **De-hardcoded.** Capability discovery replaces the fixed "Claude embedded, Codex supervised" split |

**No product capability was removed.** The V1.2.3 distributed orchestrator is now the PRO execution policy.

## The commercial thesis, stated plainly

The product sells **orchestration infrastructure**, not inference. The customer brings their own provider access; we coordinate it. Our Pro price is not a token bundle, and managed inference is explicitly deferred.

That constraint drives the entire entitlement design: our licensing service never needs to see repository content, prompts, graph nodes, diffs, or provider credentials — because it is licensing software, not brokering compute.

## The honest limitation

§7 required re-verifying current Anthropic and OpenAI facts. **That research could not be performed this pass.** Rather than guess, the architecture was designed so the unverified facts are **not load-bearing**:

- host posture is *discovered*, not assumed, so the Codex-hooks question no longer changes the design;
- provider policy is a *gate that defaults to conservative*, so the product works with zero subscription paths enabled.

Full disclosure in `SOURCE_VERIFICATION_MATRIX_V1_3.md`. This is the first thing an independent reviewer should close.

## Artifact classification

| V1.2.3 artifact | V1.3 status |
|---|---|
| Seven BUILD-A2 managers | **UNCHANGED** |
| Hard build DAG (7 nodes / 10 edges / 0 cycles) | **UNCHANGED** |
| Runtime role model | **REVISED** — policy-dependent instantiation |
| `TaskDag` | **SUPERSEDED** by `ExecutionGraph`, retained as a reduced compatibility view |
| Codex supervised-only host design | **SUPERSEDED** by capability discovery |
| A-14 ("Codex has no hooks") | **REVISED** — historical observation, no longer an architectural constant |
| `PERSONAL_LOCAL_MODE` / `PRODUCT_TEAM_MODE` | **REVISED** — subsumed by credential modes plus policy eligibility |
| Provenance, review, repair, safety, security | **UNCHANGED** |
| MVP scope | **REVISED** — must now prove a FREE and a PRO vertical slice |

Full matrix: `V1_2_3_TO_V1_3_IMPACT_MATRIX.md`.

## Disposition requested

Independent review of this candidate, then installation and freeze — or a further correction pass. BUILD-A1-BOOTSTRAP does not self-approve a reopen of this size, and does not declare it freeze-ready.
