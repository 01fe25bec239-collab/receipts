<!--
Receipts — A2 Consolidation Decision (orchestration overlay)
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
-->

# A2_CONSOLIDATION_DECISION

**Issuing authority:** **A1-BOOTSTRAP** — the temporary bootstrap A1 that designs, freezes, and packages the Receipts multi-agent operating system, and retires on formal authority transfer to `A1-RUNTIME`
**Date:** 2026-08-10 — package V2
**Repository:** `01fe25bec239-collab/receipts`
**Remote:** `origin` → `https://github.com/01fe25bec239-collab/receipts`
**Integration branch:** `main`, tracking `origin/main`
**`CONTRACT_FREEZE_SHA`:** `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`
**Working tree at freeze:** CLEAN
**`AGENT_SYSTEM_FREEZE_SHA`:** `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>`

Every reference to "A1" in this document means **the currently active A1** — `A1-BOOTSTRAP` during planning and freezing, `A1-RUNTIME` after formal authority transfer. The consolidation decision itself was made by `A1-BOOTSTRAP` and survives the transfer unchanged, because it is recorded here as a repository artifact rather than held in any agent's memory.

## Status of this document

This is an **orchestration overlay**, not a rewrite. The committed Prompt-1 orchestration package was produced under the earlier eight-manager decomposition and is preserved unchanged as frozen history. Rewriting it to disguise the change would destroy the audit trail that this program exists to demonstrate.

This overlay records what changed, when, why, and on whose authority.

## Previous decomposition — eight long-lived managers

Recorded in `orchestration/00_AGENT1_DECOMPOSITION_AND_INDEX.md` and `orchestration/02_COMPONENT_OWNERSHIP.md`, decision `D-001`:

`A2-CORE`, `A2-LEDGER`, `A2-RUNNER`, `A2-CLAUDE-INTEGRATION`, `A2-REVIEW`, `A2-INTEGRITY-SECURITY`, `A2-EVALUATION`, `A2-DOCS-RELEASE`.

## Final decomposition — five long-lived managers

| # | Manager | Long-lived responsibility |
|---|---|---|
| 1 | **A2-FOUNDATION** | Core domain + ledger domain |
| 2 | **A2-VERIFICATION** | Deterministic verification execution |
| 3 | **A2-CLAUDE-INTEGRATION** | Claude Code product integration |
| 4 | **A2-TRUST** | Probabilistic review + integrity + security + break-glass |
| 5 | **A2-QUALITY-RELEASE** | Evaluation + documentation + release evidence |

No sixth manager exists. Introducing one requires a genuine architecture-blocking reason and A1 approval.

## Old → new manager mapping

| Superseded manager | Final manager | Nature of the move |
|---|---|---|
| A2-CORE | **A2-FOUNDATION** | merged |
| A2-LEDGER | **A2-FOUNDATION** | merged |
| A2-RUNNER | **A2-VERIFICATION** | carried forward, renamed |
| A2-CLAUDE-INTEGRATION | **A2-CLAUDE-INTEGRATION** | carried forward unchanged |
| A2-REVIEW | **A2-TRUST** | merged |
| A2-INTEGRITY-SECURITY | **A2-TRUST** | merged |
| A2-EVALUATION | **A2-QUALITY-RELEASE** | merged |
| A2-DOCS-RELEASE | **A2-QUALITY-RELEASE** | merged |

Every superseded manager maps to exactly one final manager. No responsibility was dropped, and none was assigned twice.

## Why consolidation happened

A2 managers are **long-lived component managers**, not one manager per directory. Parallelism belongs beneath them:

```
A2 (long-lived manager)
 ├── A3 bounded implementation agent ──> A4 independent reviewer
 ├── A3 bounded implementation agent ──> A4 independent reviewer
 └── ...
```

Eight permanent manager sessions produced cross-component bureaucracy that exceeded its own value. Four of the eleven standing dependency requests (`DR-001`, `DR-002`, `DR-011`, and much of `DR-006`/`DR-009`) existed only because a boundary had been drawn where the work was actually continuous — core and ledger, evaluation and publication.

**Fewer A2 managers + smaller A3 tasks + independent A4 reviews** is preferable to eight permanent managers negotiating across seams that add no design value.

## What consolidation explicitly did NOT change

| Unchanged | Evidence |
|---|---|
| Product architecture | `Receipts_Final_Architecture.md` untouched at `CONTRACT_FREEZE_SHA` |
| Frozen contract semantics | All 21 contracts remain 1.0.0 FROZEN; **not one clause, field, enum, authority rule, or failure behavior was modified** |
| Milestone semantics | M0–M7 scope, acceptance evidence, and ordering unchanged |
| Integration and release gates | IG-0…IG-8 and RG-1…RG-10 unchanged |
| Evidence requirements | `orchestration/12_EVIDENCE_REQUIREMENTS.md` unchanged |
| ADR-001 | APPROVED and binding, unchanged |
| A3 granularity | **Preserved.** Merging managers must not merge atomic implementation tasks. |
| A4 independence | **Preserved.** A4 remains a distinct agent and session from A3. |
| MCP decision | `D-003` stands: NOT REQUIRED FOR MVP |
| The 17 architecture invariants | Unchanged |

**This is an ownership remap.** A contract's owner changed for three of the twenty-one contracts' management identity; the contracts themselves did not change at all.

## Control losses introduced by consolidation, and their compensations

Consolidation is not free. Two merges collapse separations that the eight-manager design created deliberately. Both are recorded here and compensated inside the affected manager packages rather than quietly absorbed.

### Loss 1 — security sign-off on review-provider design

The superseded topology had **A2-INTEGRITY-SECURITY sign off** on A2-REVIEW's `OI-003` Claude-session fallback. Both now sit inside A2-TRUST, which would make that sign-off self-approval.

**Compensation.** For `OI-003` and any security sign-off on a review-provider design, **the currently active A1 is the sign-off authority**, and the security review must be performed by an A4 session that did not participate in the review-side specification. A2-TRUST records the two roles separately in its task ledger. A merged manager may not be its own independent reviewer. Recorded in `A2_TRUST/A2_TRUST_MANAGER.md`.

### Loss 2 — separation of measurement from publication

Decision `D-010` deliberately kept A2-DOCS-RELEASE independent from A2-EVALUATION so that prose could never become evidence. A2-QUALITY-RELEASE merges them.

**Compensation — an internal firewall.** No A3 task may both produce a measurement and write publication prose. The A4 reviewing an evaluation result must not be the A4 reviewing the document citing it. A number enters a document only through a provenance record produced by the evaluation side and independently reviewed. A1 audits this seam specifically at gates IG-7 and RG-8. Recorded in `A2_QUALITY_RELEASE/A2_QUALITY_RELEASE_MANAGER.md`.

**A2-FOUNDATION's merge carries no equivalent loss.** Core and ledger were mutually dependent producers, not a producer and its checker, and the two former managers reviewed nothing of each other's that A4 does not review anyway.

## Dependency-request disposition

| DR | Was | Now | Disposition |
|---|---|---|---|
| DR-001 | A2-LEDGER ← A2-CORE | internal to A2-FOUNDATION | `CLOSED-AS-INTERNAL` |
| DR-002 | A2-CORE ← A2-LEDGER | internal to A2-FOUNDATION | `CLOSED-AS-INTERNAL` |
| DR-003 | A2-RUNNER ← A2-CORE + A2-LEDGER | A2-VERIFICATION ← A2-FOUNDATION | OPEN, remapped |
| DR-004 | A2-CLAUDE-INTEGRATION ← A2-CORE | ← A2-FOUNDATION | OPEN, remapped |
| DR-005 | A2-CLAUDE-INTEGRATION ← A2-INTEGRITY-SECURITY | ← A2-TRUST | OPEN, remapped |
| DR-006 | A2-REVIEW ← A2-CORE + A2-LEDGER | A2-TRUST ← A2-FOUNDATION | OPEN, remapped |
| DR-007 | A2-REVIEW ← A2-CLAUDE-INTEGRATION | A2-TRUST ← A2-CLAUDE-INTEGRATION | OPEN, remapped |
| DR-008 | A2-INTEGRITY-SECURITY ← A2-RUNNER | A2-TRUST ← A2-VERIFICATION | OPEN, remapped |
| DR-009 | A2-INTEGRITY-SECURITY ← A2-LEDGER | A2-TRUST ← A2-FOUNDATION | OPEN, remapped |
| DR-010 | A2-EVALUATION ← all product A2s | A2-QUALITY-RELEASE ← all four | OPEN, remapped |
| DR-011 | A2-DOCS-RELEASE ← A2-EVALUATION | internal to A2-QUALITY-RELEASE | `CLOSED-AS-INTERNAL` **as paperwork only** — replaced by the stricter internal firewall above |

`CLOSED-AS-INTERNAL` removes the paperwork, not the work. Each internalized dependency still requires a written interface and its own A3 task.

## Precedence rule

If a committed orchestration document conflicts with `A2_OWNERSHIP_REMAP.md` **only** about manager identity, manager count, or ownership: **`A2_OWNERSHIP_REMAP.md` wins.**

If there is a **semantic product-architecture conflict** — a difference about how the product behaves, what a contract means, or what an invariant requires — **STOP and raise an issue to A1.** This overlay may not silently modify Receipts product architecture, and no manager may reconcile such a conflict on its own authority.

## Authority

Consolidation decided and issued by **A1-BOOTSTRAP** on 2026-08-10, against semantic baseline `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`. It binds `A1-RUNTIME` on succession: a successor A1 inherits this decomposition and may change it only through a recorded decision of the same kind, never by silent reinterpretation. The superseded eight-manager package remains valid **detailed planning input** and retains its component reasoning; it is superseded **for manager topology only** and is never authority.
