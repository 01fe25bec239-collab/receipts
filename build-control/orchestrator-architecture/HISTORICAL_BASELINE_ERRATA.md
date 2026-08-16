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

# HISTORICAL_BASELINE_ERRATA

**Purpose:** stop an implementer from following obsolete instructions in the committed historical baseline. **No historical file is modified by this package.**

## Authority ladder

```
Historical Receipts architecture   → reference / history only where superseded
Approved ADR-001                   → controlling decision for the OLD Receipts baseline
New orchestrator architecture      → controlling architecture after install and freeze
```

Where the three disagree, the lowest line wins **for the new product**, and the upper lines remain the accurate record of what was decided for the old one.

## E-01 — `Receipts_Final_Architecture.md` §O and §T contain stale worktree guidance

**Location:** §O (Claude hook mapping) and §T item 8 (exact MVP hook list), at `3c70f4d8…`.

**Stale text, verbatim from §O:**

> `WorktreeCreate` … `[VF]` **Any non-zero exit fails worktree creation** — `[DD]` therefore this handler must be trivial, wrapped in a catch-all, and **always exit 0**; a broker bug here would break the user's worktrees

**Why it is wrong twice over:**

| | Says | Reality |
|---|---|---|
| Old Receipts + ADR-001 | install a trivial always-exit-0 handler | ADR-001 (APPROVED) says **install no worktree hook at all** — an always-exit-0 handler cannot satisfy the hook contract, which requires the handler to create the worktree and return its path |
| New orchestrator | — | The orchestrator **does** install `WorktreeCreate`, and the handler must be **correct, fast, path-returning, with a tested fallback** — the opposite of trivial |

So the committed §O text is incorrect under **both** the old and the new architecture. An implementer following it would ship a handler that breaks worktree creation.

**Correct current guidance:** `CLAUDE_HOST_ADAPTER.md` and `WORKSPACE_EXECUTION_ARCHITECTURE.md` in this package.

**Disposition:** recorded here; §O/§T are **not edited**. The historical document stays as committed evidence.

## E-02 — Two historical invariant lists exist

`Receipts_Final_Architecture.md` §C has **10** invariants. `orchestration/01_ARCHITECTURE_AUTHORITY.md` has **17**. Numbers 9, 10 and 11 mean **different things** in the two lists; 12–17 exist only in the second.

Citations of the form *invariant N* in the historical packages follow the **17-item orchestration list**. Mapping table in `REPOSITORY_RECONCILIATION_REPORT.md`.

**This package defines its own invariants as `I-1`…`I-20`** in `NEW_SYSTEM_ARCHITECTURE.md`, deliberately renumbered and prefixed so no reader can confuse them with either historical list.

## E-03 — Filename drift

`orchestration/01_ARCHITECTURE_AUTHORITY.md` names its binding source `Receipts_Final_Architecture(1).md`; the committed file is `Receipts_Final_Architecture.md`. A leftover download suffix. Cosmetic, but recorded so an automated context loader does not fail on it.

## E-04 — Historical invariant 14 is deliberately superseded

Orchestration invariant 14 read: *"Receipts must not expand into a generic multi-agent orchestrator."*

The new product **is** an orchestrator. This is not an accidental violation — it is the reason `ARCHITECTURE_REOPEN_001` exists, and orchestration invariant 17 required exactly that process for a change of this kind. The old invariant remains the correct record of what was decided for the old product.

## Rule for implementers

When the historical baseline and this package disagree about the **new product**, this package governs. When they disagree about **what was historically decided**, the historical baseline and ADR-001 govern. Neither is deleted.
