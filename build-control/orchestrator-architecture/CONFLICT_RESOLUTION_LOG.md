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

# CONFLICT_RESOLUTION_LOG

Material conflicts between the historical Receipts architecture, the earlier FAM-era direction, the capability research, and the latest requirements. Nothing here is resolved silently.

---

## C-01 — "Receipts is not an orchestrator" vs "the product is an orchestrator"

**Old:** Receipts architecture invariant 14 rejected an agent-runtime architecture outright; invariant 15 kept `ReviewProvider` to four operations specifically so it could not grow into one.
**New:** §4 defines the product as a multi-agent coding orchestration control plane.
**Resolution:** **New requirements win.** This is the reopen's whole reason. Recorded as `ARCHITECTURE_REOPEN_001`; the old invariants are HISTORICAL_ONLY, not silently deleted.
**Note:** the old invariants were *good engineering* for the old product. They are not being overturned because they were wrong; they are being retired because the product changed.

---

## C-02 — ADR-001 (no worktree hooks) vs orchestrator owning workspaces

**Old:** ADR-001, APPROVED, concluded Receipts must install no `WorktreeCreate`/`WorktreeRemove` hook and must not own worktree creation, because a hook replaces Claude Code's default behaviour and Receipts had no business creating workspaces.
**New:** §63 requires the orchestrator to own workspace isolation; the research (A-03) confirms `WorktreeCreate` replaces default logic and must return a path — which is precisely the capability an orchestrator wants.
**Resolution:** **SUPERSEDED**, with the reasoning preserved. ADR-001's *analysis was correct and remains correct*; only its premise changed. Receipts was an evidence layer with no workspace responsibility, so hooking worktree creation was scope creep. The orchestrator's core job *is* workspace lifecycle, so the same mechanism is now appropriate. Full record in `ADR_IMPACT_MATRIX.md`.
**Retained from ADR-001:** the invariant that a worktree is workspace isolation and never a security sandbox (now I-11), and the discipline of verifying host behaviour against current primary documentation before depending on it.

---

## C-03 — "Agents lie about tests" vs "assume competent workers"

**Old:** the Receipts pitch rested on agents falsely claiming tests passed.
**New:** §67 explicitly forbids that framing; the system must be valuable with competent workers.
**Resolution:** **New wins.** Provenance is reframed from *detecting dishonesty* to *maintaining durable knowledge across sessions, providers, and code states*. Consequence: worker-executed checks are accepted as evidence under LIGHT and STANDARD (§69), and independent re-execution is reserved for HIGH_ASSURANCE. This materially reduces cost and is the single largest behavioural change from the old design.

---

## C-04 — Old five-A2 topology vs new BUILD-A2 topology

**Old:** A2-FOUNDATION, A2-VERIFICATION, A2-CLAUDE-INTEGRATION, A2-TRUST, A2-QUALITY-RELEASE — organised around an evidence pipeline.
**New:** seven BUILD-A2 managers organised around orchestration subsystems.
**Resolution:** old topology **SUPERSEDED**. Reusable elements retained: the A1-BOOTSTRAP → A1-RUNTIME succession model, the three-baseline model (`CONTRACT_FREEZE_SHA` / `AGENT_SYSTEM_FREEZE_SHA` / `A2_START_SHA`), per-manager context minimisation, and the no-docs-manager rule — which the old package reached independently and §87 now mandates.

---

## C-05 — Old A2-QUALITY-RELEASE owned documentation vs §87 no docs BUILD-A2

**Old:** A2-QUALITY-RELEASE owned `docs/**` and `README.md` (with an internal firewall separating measurement from publication).
**New:** §87 forbids a docs-oriented BUILD-A2.
**Resolution:** **New wins.** Documentation correctness belongs to the engineering BUILD-A2 owning the subsystem; docs work is an economy BUILD-A3 task. The old firewall concern does not vanish — it reappears in `SECURITY_TRUST_MODEL.md` as the rule that a renderer may never alter authoritative state (I-13).

---

## C-06 — Claude-first instinct vs hard host parity

**Old direction:** Claude Code as the host, everything else as a client.
**New:** §9 requires Claude Code and Codex to be equally first-class.
**Conflict with research at the time (now historical):** A-14 (**RETIRED at V1.3.1**; historical observation dated 2026-08-13) — at that time Codex had no hook system comparable to Claude Code's, so identical plumbing is impossible.
**Resolution (CURRENT, V1.3.2):** parity is defined **behaviourally, not mechanically**. Both adapters now share the same primary posture:

```
Claude → native plugin / hooks → shared core
Codex  → native plugin / hooks → shared core          (registry C-01, C-02)
Codex supervised / hybrid → compatibility fallback only
```

**[HISTORICAL] Resolution as it stood at V1.2.3–V1.3 (superseded):** at that time Codex was believed to lack a lifecycle-hook system, so CodexHostAdapter was specified to drive `codex exec` plus a companion supervisor. That reasoning was correct on the evidence then available and is preserved as dated history only.

Parity remains behavioural rather than mechanical for a better reason than capability asymmetry: hook coverage varies by version, plugin hooks require trust before running, and hooks can be disabled entirely.
**Residual risk:** recorded in `FAILURE_CRITERIA.md` — if parity cannot be reached without invasive wrappers, that is a pivot signal.

---

## C-07 — "Use the best model" vs "never trust training memory"

**Conflict:** §27–28 require dynamic model intelligence, but the executor doing the routing is itself an LLM whose training data contains model opinions.
**Resolution:** the router is **code, not a prompt**. Model selection is a deterministic function over the Model Intelligence registry. An LLM executor may express a *preference*, but a preference without registry backing is discarded, and the routing decision record shows the evidence used. I-4 is enforced structurally rather than by instruction.

---

## C-08 — Cheapest-model instinct vs frontier quality floor

**Conflict:** §36 optimises cost; §41 forbids downgrading critical work.
**Resolution:** not actually in conflict once the objective is stated correctly — the metric is *expected cost to an accepted result*, which already penalises cheap models that cause repair loops. The frontier floor is a hard constraint applied *before* optimisation, not a competing objective. `EXPECTED_COST_TO_ACCEPTED_RESULT.md` makes this ordering explicit.

---

## C-09 — Subscription convenience vs provider policy

**Conflict:** §50 wants to reuse the developer's existing authenticated CLIs; A-20 records that Anthropic forbids third-party products routing users through subscription credentials.
**Resolution:** two explicitly separated modes. `PERSONAL_LOCAL_MODE` (local, single user, invokes the user's own authenticated CLI, credentials stay in provider tooling) and `PRODUCT_TEAM_MODE` (API keys, enterprise credentials, gateways). The architecture never blurs them, and no code path lets a team deployment reach a personal subscription. Re-verification required before implementation (§51).

---

## C-10 — Defensive security work vs provider safety refusals

**Conflict:** §75 permits trying another provider when a legitimate defensive task is blocked; §76 prohibits provider-shopping to bypass safety.
**Resolution:** the distinguishing factor is **intent and record**, made concrete: retry is permitted only when the task is classified defensive, the capsule is narrowed to reduce exploit-generation detail, the attempt is preserved and logged, and the retry is policy-compatible. Any retry whose purpose is to obtain a refused capability is prohibited, and the event log makes the pattern visible. If no eligible provider completes it: `HUMAN_REQUIRED`, never a false `PASS` (§77). Detailed decision table in `SAFETY_INTERRUPTION_PROTOCOL.md`.

---

## C-11 — Worktree isolation as differentiator vs research finding

**Conflict:** early framing treated worktree isolation as a headline feature; the research found it commoditised across Claude Squad, Conductor, Crystal, vibe-kanban, amux and vendor-native teams.
**Resolution:** **research wins.** Worktree isolation is table stakes and is documented as such in `NON_GOALS.md`. Differentiation claims are restricted to the combination in `PRODUCT_DEFINITION.md`, and `FAILURE_CRITERIA.md` records what would erode it.

---

## C-12 — Old contracts frozen at 1.0.0 vs new domain

**Conflict:** 21 contracts were frozen and the old process treats a frozen contract as changeable only by recorded request.
**Resolution:** the reopen supersedes the *product*, not the *process*. Contracts are not silently mutated; each receives an explicit disposition in `CONTRACT_IMPACT_MATRIX.md` (KEEP / REVISE / SUPERSEDE / RETIRE) with a reason. Versions do not carry over: a revised concept enters the new architecture as a **new** contract at 0.1.0-draft, so nothing inherits a frozen version number it did not earn.

---

## C-13 — Two invariant lists in the committed repository *(added V1.1)*

**Conflict:** `Receipts_Final_Architecture.md` §C defines 10 core invariants; `orchestration/01_ARCHITECTURE_AUTHORITY.md` defines 17. Numbers 9, 10 and 11 mean *different things* in the two lists, and 12–17 exist only in the second.
**Resolution:** both are genuine artifacts of the historical baseline and neither is deleted. This package's `invariant N` citations follow the **17-item orchestration list**, stated explicitly wherever cited. A mapping table is in `RECONCILIATION_REPORT_V1_1.md` R-01.
**Note:** V1 cited correctly but attributed the numbers to the architecture document. Precision defect; no design consequence.

---

## C-14 — Architecture §O/§T vs APPROVED ADR-001 *(added V1.1)*

**Conflict:** at HEAD the architecture still lists `WorktreeCreate`/`WorktreeRemove` in the MVP hook set and instructs that the handler *"must be trivial… and always exit 0"*. ADR-001, APPROVED, says neither hook is installed and that such a handler is impossible.
**Resolution:** ADR-001 wins for the historical product — it is the correction of record, and the earlier pass deliberately did not rewrite the architecture document. For the **new** product the outcome inverts again: `WorktreeCreate` **is** installed, because the orchestrator owns workspace lifecycle.
**Why this matters:** all three positions differ, and the committed §O text is the one that is wrong under *both* the old and the new architecture. A reader implementing from §O would build a trivial always-exit-0 handler, which breaks worktree creation under the new design. `ADR_IMPACT_MATRIX.md` and `CLAUDE_HOST_ADAPTER.md` state the correct handler semantics.

---

## C-15 — Orchestration invariants 16 and 17 undispositioned in V1 *(added V1.1)*

**Conflict:** invariant 16 (no MCP without a concrete unmet requirement) and 17 (external-capability change requires a deviation request) were not explicitly addressed.
**Resolution:** both are satisfied. `MCP_POSITION.md` applies invariant 16's test verbatim — MCP only where hooks/skills/CLI cannot serve, never for symmetry. Invariant 17 is the process this entire reopen followed: `ARCHITECTURE_REOPEN_001` is the deviation request, at maximum scope.

