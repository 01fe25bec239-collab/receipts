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

# ADR_IMPACT_MATRIX

## ADR-001 — WorktreeCreate Observation Conflict

**Historical status:** APPROVED, 2026-08-09, reconciled across the frozen packages.
**Historical decision:** Receipts installs no `WorktreeCreate` hook and no `WorktreeRemove` hook; Receipts does not own worktree creation; workspace identity is bound observationally.

**New classification: SUPERSEDED (in part).**

### Why it is superseded — precisely

ADR-001's reasoning was:

1. Configuring `WorktreeCreate` **replaces** Claude Code's default git worktree creation and requires the hook to create and return the path. *(Re-confirmed at V1.3.1 as registry claim `C-05`, reviewer-supplied current primary source; see `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`.)*
2. Receipts was an evidence and admission layer with no workspace responsibility.
3. Therefore taking over worktree creation would be scope creep, and an "observational" handler was impossible.

**Fact 1 is unchanged and still true. Premise 2 is no longer true.**

The product's core responsibility now *is* workspace lifecycle: it creates isolated worktrees for parallel A3 attempts, tracks them, checkpoints them, recovers them, and tears them down. The mechanism ADR-001 correctly refused is precisely the mechanism this product needs.

This is a **changed premise, not a reversed analysis**. ADR-001 was right for Receipts and would still be right for Receipts.

### What is retained from ADR-001

| Retained | Where |
|---|---|
| Worktrees are workspace isolation, never security isolation | I-11, `WORKSPACE_EXECUTION_ARCHITECTURE.md`, `SECURITY_TRUST_MODEL.md` |
| Verify host behaviour against current primary documentation before depending on it | `ASSUMPTION_REGISTER.md` methodology |
| Do not install a hook whose worst case is silently taking over behaviour you do not want to own | Applied inversely — we now *do* want to own it, and say so explicitly |
| `WorktreeRemove` has no decision control and cannot block | A-03; the adapter treats it as fire-and-forget |

### What changes

| Item | Old | New |
|---|---|---|
| `WorktreeCreate` hook | Never installed | **Installed by `ClaudeHostAdapter`**, with a correct handler that creates the worktree and returns its path |
| `WorktreeRemove` hook | Never installed | Installed as a cleanup observer |
| Worktree ownership | Claude Code / git | **Orchestrator**, on Claude Code via the hook; elsewhere directly |
| Failure posture | N/A | Handler must be fast and correct; falls back to a plain `git worktree` if the core is unreachable, so the user is never blocked |

### Obligations created by this supersession

1. The `WorktreeCreate` handler is on an interactive path — it is a correctness- and latency-critical component, not a trivial hook.
2. It requires a tested fallback path.
3. `OI-009`-style re-verification of `WorktreeRemove` semantics remains open before implementation.
4. No document may describe the resulting worktrees as sandboxes.

### Committed-repository finding (V1.1)

Verified against the snapshot: `Receipts_Final_Architecture.md` at HEAD **still contains both worktree hooks** in §O and in the §T MVP list, including the instruction that the handler *"must be trivial, wrapped in a catch-all, and always exit 0"*.

ADR-001 refuted exactly that instruction, and the earlier reconciliation pass updated the contracts and orchestration files but was instructed not to modify the architecture document. The divergence is therefore real and present in the baseline.

Three positions, all different:

| Source | `WorktreeCreate` | Handler |
|---|---|---|
| Architecture §O/§T (at HEAD) | installed | trivial, always exit 0 |
| ADR-001 (APPROVED) | **not installed** | n/a |
| **This architecture** | **installed** | **correct, fast, path-returning, with tested fallback** |

The committed §O text is wrong under both the old and the new architecture. Anyone implementing from it would produce a handler that breaks worktree creation. Correct semantics are in `CLAUDE_HOST_ADAPTER.md` and `WORKSPACE_EXECUTION_ARCHITECTURE.md`.

### Historical record

ADR-001 is **not deleted**. It remains at `CONTRACT_FREEZE_SHA` as approved historical evidence, and this matrix is the forward-facing record of its supersession.

## Other ADRs

No other architecture deviation requests existed at `CONTRACT_FREEZE_SHA`. The 13 frozen decisions `D-001`…`D-013` are dispositioned implicitly through `CONTRACT_IMPACT_MATRIX.md` and `OLD_RECEIPTS_REUSE_MATRIX.md`; the ones with continuing force are `D-011` (worktrees are not a security boundary → I-11) and the no-docs-manager principle, which §87 independently mandates.
