<!--
Receipts — A2 Initialization / Activation Order
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
-->

# A2_INITIALIZATION_ORDER

**Issuing authority:** A1-BOOTSTRAP
**Date:** 2026-08-10 — package V2
**Semantic baseline:** `CONTRACT_FREEZE_SHA` = `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221`
**System baseline:** `AGENT_SYSTEM_FREEZE_SHA` = `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>`
**Per-manager start point:** `A2_START_SHA`, supplied in each bootstrap handoff

Every reference to "A1" below means **the currently active A1** — `A1-BOOTSTRAP` now, `A1-RUNTIME` after formal authority transfer. Initialization and activation are performed by `A1-RUNTIME`, not by `A1-BOOTSTRAP`; this document defines the order it will follow.

## The distinction that governs this document

**INITIALIZED** — the manager has received a complete bootstrap handoff, has passed all eight initialization checks against its supplied `A2_START_SHA`, has ingested its `MANDATORY` context, has confirmed ownership and contract classification, and has delivered its `FIRST_MANAGER_TASK.md` outputs. It may specify, analyze, propose, and escalate. It holds a validated **A2 integration worktree** provisioned by the active A1.

**ACTIVE FOR IMPLEMENTATION** — the active A1 has authorized the manager's implementation wave and its handoff carries `a3_implementation_authorized: true`. Only then may it create **A3 implementation** branches and worktrees and issue A3 coding work.

Holding a validated integration worktree is **not** implementation authority. The A2 integration worktree is provisioned by the active A1 and exists so the manager can read, specify, and record status. The A3 implementation worktree is created by the manager per bounded task, only after the wave opens.

All five managers may be **initialized** in any order, including in parallel. **Activation** follows dependencies strictly. Later managers performing safe analysis early is encouraged; downstream implementation before upstream acceptance is prohibited.

## Activation phases

| Phase | Manager | Why here | Activation precondition |
|---|---|---|---|
| **1** | **A2-FOUNDATION** | Owns 10 of 21 contracts and the two decisions every other manager waits on. Nothing can be built before the toolchain and the serialization algorithm exist. | A1 approves `OI-001` and freezes `OI-002`; A1 authorizes the M0 wave. |
| **2** | **A2-VERIFICATION** | Every receipt binds a fingerprint and persists through the ledger. | Foundation M0 accepted at IG-6; `DR-003-R` satisfied; `GAP-002` decided. |
| **3** | **A2-CLAUDE-INTEGRATION** | Gates encode admission; admission must exist and be stable first. | Foundation M2 accepted; `DR-004-R` and `DR-005-R` satisfied; `OI-004` frozen; current-doc re-verification accepted. |
| **4** | **A2-TRUST** | Review providers consume claim, evidence, and persistence semantics; integrity signals consume runner facts. | M3 accepted; `DR-006-R`, `DR-007-R`, `DR-008-R`, `DR-009-R` satisfied; `OI-003` frozen with A1 sign-off. |
| **5** | **A2-QUALITY-RELEASE** | Measures a product that must exist first; publishes numbers that must be measured first. | **M5 complete and integrated**; `DR-010-R` satisfied; oracles frozen. |

## Initialization order — for A1's dependency on outputs

Initialization is parallel; the sequence below orders **when A1 needs each manager's first-pass deliverables**, because some of them unblock everyone else.

| Order | Manager | Gating output A1 waits on |
|---|---|---|
| 1 | **A2-FOUNDATION** | `OI-001` proposal; `OI-002` proposal with ≥6 golden digest fixtures; fingerprint specification pack; `GAP-001` consumer position |
| 2 | **A2-TRUST** | Threat model; enforcement-scope audit v0; the deny / fail-direction requirements package for A2-CLAUDE-INTEGRATION |
| 3 | **A2-VERIFICATION** | `GAP-002` escalation; process-safety specification; `OI-005` joint proposal |
| 4 | **A2-CLAUDE-INTEGRATION** | Current-documentation re-verification record; hook set declaration; workspace-observation specification; `OI-004` test plan; `GAP-001` proposal |
| 5 | **A2-QUALITY-RELEASE** | Internal firewall declaration; benchmark task catalogue; metric definitions with explicit denominators; honest scope sentence; `OI-006` collision result |

**A2-TRUST is deliberately second in initialization while being fourth in activation.** Its security requirements are needed by A2-CLAUDE-INTEGRATION and A2-VERIFICATION long before its own implementation wave opens. Security requirements retrofitted are security requirements lost.

**A2-QUALITY-RELEASE is last in both, but must not be late.** Its harness design constrains what the product must expose to be measurable. Initializing it after M5 would produce a product that cannot be evaluated without retrofitting — and retrofitting for measurability is how benchmarks get quietly shaped to flatter the thing they measure.

## Rules governing initialization

1. **Parallel initialization is permitted and expected.** The order above is A1's dependency on outputs, not a start-time constraint.
2. **No manager issues an A3 task during initialization.** First-pass output is specification, proposal, validation, ownership confirmation, blocker resolution, and status. A manager that returns code has failed its first task.
3. **Baseline verification precedes everything.** A manager performs the eight-step initialization check below, in order, before doing anything else. `CONTRACT_FREEZE_SHA` is a **historical semantic baseline only** and is never expected to equal HEAD.

   | Step | Check | Requirement |
   |---:|---|---|
   | 1 | Bootstrap handoff | Complete; no unresolved placeholder in any required field |
   | 2 | `git rev-parse HEAD` | **equals the supplied `A2_START_SHA`** |
   | 3 | `git status --porcelain` | empty |
   | 4 | `git remote get-url <remote_name>` | matches the expected repository and `remote_url` |
   | 5 | `git merge-base --is-ancestor <CONTRACT_FREEZE_SHA> HEAD` | `CONTRACT_FREEZE_SHA` is an **ancestor of** HEAD |
   | 6 | `git merge-base --is-ancestor <AGENT_SYSTEM_FREEZE_SHA> <A2_START_SHA>` | `AGENT_SYSTEM_FREEZE_SHA` is an **ancestor of or equal to** `A2_START_SHA` |
   | 7 | `git rev-parse --abbrev-ref HEAD` | equals the supplied `manager_branch` |
   | 8 | `git rev-parse --show-toplevel` | equals the supplied `manager_worktree_path` |

   Any failure is a hard stop and a report to the currently active A1 — never a repair, reset, re-clone, or best-effort continuation.

   The manager's **A2 integration branch and worktree already exist** at this point: they were created or validated by the active A1 and handed over with the bootstrap handoff. The manager **verifies** that workspace and **MUST NOT create, replace, rebase, rename, or move it.** Holding a validated integration worktree confers no implementation authority.
4. **Cross-manager joint items are joint.** `OI-003` (Trust + Claude-Integration, **A1 signs off**), `OI-004` (Claude-Integration + Trust), and `OI-005` (Verification + Trust) each require both managers' agreement before A1 will freeze them. Unilateral proposals are returned.
5. **Divergence findings escalate immediately.** If any manager's current-documentation re-verification contradicts a frozen contract, it goes to A1 as a finding the same day. It is never adapted around. ADR-001 is the precedent and the standard.
6. **Semantic conflicts stop work.** If a committed orchestration document conflicts with the architecture or a frozen contract about product behavior — not merely about manager identity — the manager stops and raises it. `A2_OWNERSHIP_REMAP.md` wins only on identity, count, and ownership.
7. **IG-0 must stay passing** throughout: architecture authority acknowledged, no unapproved deviation pending, manager ownership fixed.

## Safe work available to not-yet-active managers

Being pre-activation is not idleness. A manager may, before activation: specify interfaces; design fixtures on paper; write test plans and negative-test catalogues; re-verify external documentation; draft schemas as specifications; audit its own contracts for ambiguity; answer inbound dependency requests; and prepare A3 task packets marked `NOT_ISSUED` with their unmet preconditions listed.

A manager may **not**, before activation: create an **A3 implementation** branch; create an **A3 implementation** worktree; create or modify any product source file; issue an A3 coding task; or commit implementation work.

It **may** hold its pre-provisioned **A2 integration** worktree throughout. That workspace exists so the manager can read the repository at `A2_START_SHA`, produce specifications, and record its status files. Its existence is not authorization to implement, and the manager never provisions, replaces, rebases, renames, or moves it.
