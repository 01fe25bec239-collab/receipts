<!--
Receipts — A2 Manager Initialization Package (5-manager FINAL topology)
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
-->

# A2_INITIALIZATION_INDEX

**Issuing authority:** **A1-BOOTSTRAP** — the temporary bootstrap A1 that designs, freezes, and packages the Receipts multi-agent operating system, and retires on formal authority transfer to `A1-RUNTIME`
**Date:** 2026-08-10
**Package version:** V2 — supersedes the first 5-A2 package
**Repository:** `01fe25bec239-collab/receipts` — `origin` → `https://github.com/01fe25bec239-collab/receipts.git` — integration branch `main`
**Install root:** `build-control/a2/` (see `REPOSITORY_INSTALLATION_MAP.md`)
**Phase:** final long-lived A2 manager structure. **No implementation work is issued by this package.**

## A1 lifecycle — read before using this document

| Role | Responsibility | State now |
|---|---|---|
| **A1-BOOTSTRAP** | Designs, freezes, and packages the multi-agent operating system. Issues this package. | **ACTIVE** |
| **A1-RUNTIME** | Controls implementation: validates A2 integration worktrees, initializes managers, authorizes implementation waves, runs integration gates. | **NOT YET INITIALIZED** |

There is **never** more than one authoritative A1. On formal authority transfer, `A1-BOOTSTRAP` becomes RETIRED and `A1-RUNTIME` becomes ACTIVE. That transfer has **not** occurred and is not performed by this package.

Every reference below to "A1" means **the currently active A1**. `A1-RUNTIME`, `A2`, `A3`, and `A4` are logical roles, not model identities; any capable runtime may execute any of them.

## The three baselines

| Baseline | Meaning | Value |
|---|---|---|
| `CONTRACT_FREEZE_SHA` | Frozen architecture, contracts, ADRs, orchestration foundation. Permanent historical authority. | `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221` |
| `AGENT_SYSTEM_FREEZE_SHA` | The `main` commit holding the complete frozen agent operating system, including everything still to be produced. | `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>` |
| `A2_START_SHA` | The accepted `main` commit a manager's integration branch and worktree are created from. Supplied per manager in its bootstrap handoff. | assigned at initialization |

At initial startup `A2_START_SHA` is expected to equal `AGENT_SYSTEM_FREEZE_SHA`, but the two are **not permanently coupled**: a manager initialized or re-initialized later may legitimately start from a newer accepted `main` commit. **No agent may fabricate any of these values.**

### What V2 corrects

The previous package required each manager to verify `git rev-parse HEAD == CONTRACT_FREEZE_SHA`. That was wrong and is removed. By the time any manager initializes, `main` will have advanced well beyond the contract freeze — it must contain the entire agent operating system, which did not exist at that commit. A manager now verifies HEAD against its supplied `A2_START_SHA`, and verifies `CONTRACT_FREEZE_SHA` as an **ancestor** of HEAD rather than as HEAD itself.

## What this package is

Five long-lived component managers, each with four files: its standalone manager initialization prompt, a `CONTEXT_MANIFEST.md`, an `OWNERSHIP_MANIFEST.md`, and a `FIRST_MANAGER_TASK.md`. Plus three program-level ordering documents, two orchestration overlay documents, and an integrity manifest.

Verify before use, from the package root:

```
sha256sum -c PACKAGE_MANIFEST.sha256      # from the extracted ZIP root
```

## Package contents

| Path | Purpose |
|---|---|
| `A2_INITIALIZATION_INDEX.md` | This file |
| `A2_INITIALIZATION_ORDER.md` | Initialization vs. activation order, and why |
| `A2_FIRST_TASK_ISSUANCE_ORDER.md` | When each manager may first issue A3 work, with gates |
| `A2_CONSOLIDATION_DECISION.md` | 8 → 5 decision record: mapping, reasons, what did not change, control losses and compensations |
| `A2_OWNERSHIP_REMAP.md` | **Authoritative** for manager identity, count, contract ownership, and file ownership |
| `A2_BOOTSTRAP_HANDOFF_TEMPLATE.md` | The only sanctioned way to initialize a manager: fields, placeholder rules, verification checks |
| `REPOSITORY_INSTALLATION_MAP.md` | ZIP → repository path mapping, install procedure, and who may write each installed file |
| `A2_FOUNDATION/` | Core domain + ledger domain |
| `A2_VERIFICATION/` | Deterministic verification execution |
| `A2_CLAUDE_INTEGRATION/` | Claude Code product integration |
| `A2_TRUST/` | Probabilistic review + integrity + security + break-glass |
| `A2_QUALITY_RELEASE/` | Evaluation + documentation + release evidence |
| `PACKAGE_MANIFEST.sha256` | SHA-256 of every packaged file, in **ZIP layout** (`A2_FOUNDATION/…`). Run from the extracted ZIP root. |
| `INSTALL_MANIFEST.sha256` | SHA-256 of the same file contents, in **installed layout** (`foundation/…`). Run from `build-control/a2/` after installation. |

The two manifests exist because the ZIP uses uppercase manager folders and the repository uses lowercase slugs; one manifest cannot describe both layouts. Neither hashes itself or the other. `PACKAGE_MANIFEST.sha256` is not installed into the repository, and running it from `build-control/a2/` proves nothing.

## The five managers

| # | Manager | Merges / carries forward | Contracts owned | Activation |
|---|---|---|---|---|
| 1 | **A2-FOUNDATION** | A2-CORE + A2-LEDGER | 10 | Phase 1 |
| 2 | **A2-VERIFICATION** | A2-RUNNER | 3 | Phase 2 |
| 3 | **A2-CLAUDE-INTEGRATION** | A2-CLAUDE-INTEGRATION | 3 | Phase 3 |
| 4 | **A2-TRUST** | A2-REVIEW + A2-INTEGRITY-SECURITY | 5 | Phase 4 |
| 5 | **A2-QUALITY-RELEASE** | A2-EVALUATION + A2-DOCS-RELEASE | 0 | Phase 5 |

**Exactly five.** A sixth requires a genuine architecture-blocking reason and A1 approval.

## What each manager receives

1. Its own manager initialization file, `CONTEXT_MANIFEST.md`, `OWNERSHIP_MANIFEST.md`, and `FIRST_MANAGER_TASK.md`
2. `Receipts_Final_Architecture.md`
3. The frozen orchestration package — `orchestration/00…15`
4. The frozen contract package — `contracts/**`, `schemas/SCHEMA_PLAN.md`, `architecture-decisions/ARCHITECTURE_DEVIATION_REQUEST_001.md`
5. A completed `A2_BOOTSTRAP_HANDOFF_TEMPLATE.md` from the currently active A1
6. A validated A2 **integration** worktree at the path named in that handoff

Every manager file names the exact architecture sections, contracts, orchestration documents, and repository paths it must inspect before acting, and requires all eight initialization checks to pass first: HEAD equals `a2_start_sha`; the working tree is clean; the remote matches; `CONTRACT_FREEZE_SHA` is an ancestor of HEAD; `AGENT_SYSTEM_FREEZE_SHA` is an ancestor of or equal to `A2_START_SHA`; and branch and worktree identity match the handoff. **Any failure is a hard stop and a report to the active A1** — never a repair, reset, or best-effort continuation.

### Two kinds of worktree, two authorization semantics

| | A2 integration worktree | A3 implementation worktree |
|---|---|---|
| Lifetime | Long-lived, spans the manager's tenure | Short-lived, one bounded task |
| Branch | `a2/<slug>` | `a3/<slug>/<task-id>` |
| Provisioned by | The currently active A1 | The manager, **after** wave authorization |
| Implies implementation authority | **No** | Yes, for that task only |

A manager may hold a fully validated integration worktree and still be forbidden to write a single line of source. These are ordinary development-process Git worktrees; under ADR-001 the **Receipts product** installs no worktree hooks and does not own worktree creation. The two are never conflated.

## Context minimization

The superseded eight-manager package supplied every contract to every manager. This package does not. Each `CONTEXT_MANIFEST.md` classifies all 21 frozen contracts into four classes:

| Class | Meaning |
|---|---|
| **MANDATORY** | Ingest in full at initialization and re-read before issuing any A3 task |
| **CONSUMED** | Directly depended on and binding; load when a task touches it |
| **REFERENCE** | Inspect on demand; do not load for every task |
| **EXCLUDED / FOREIGN OWNERSHIP** | Visible, never yours — do not modify, do not redefine, do not assume ownership |

`EXCLUDED` does not mean invisible. Every manager may read anything in the repository; write authority comes only from `OWNERSHIP_MANIFEST.md`.

**One deliberate exception:** A2-TRUST has no EXCLUDED *reading* class. A security auditor whose scope is narrower than the system cannot produce a valid enforcement-scope audit. Its **write** exclusions are absolute and unchanged.

## Contract ownership integrity

| Check | Result |
|---|---|
| Frozen contracts | 21, all 1.0.0 FROZEN |
| Exactly one owner each | 21 / 21 |
| Ownerless | **0** |
| Multiply owned | **0** |
| Distribution | FOUNDATION 10, VERIFICATION 3, CLAUDE-INTEGRATION 3, TRUST 5, QUALITY-RELEASE 0 |

Ownership means the manager answers for the contract and originates change requests. It does **not** grant edit rights; every contract stays frozen.

## Control losses from consolidation, and compensations

Consolidation is not free. Two merges collapse separations the eight-manager design created on purpose. Both are recorded rather than absorbed.

| Loss | Compensation | Recorded in |
|---|---|---|
| A2-INTEGRITY-SECURITY's sign-off on A2-REVIEW's `OI-003` becomes self-approval inside A2-TRUST | **A1 is the sign-off authority**; the security review must be an A4 session distinct from the review-side specification; the two roles are ledgered separately | `A2_TRUST/A2_TRUST_MANAGER.md` |
| `D-010`'s separation of evaluation from publication is collapsed inside A2-QUALITY-RELEASE | **Internal firewall**: no A3 task both measures and publishes; distinct A4 sessions; numbers enter documents only via an independently reviewed provenance record; A1 audits the seam at IG-7 and RG-8 | `A2_QUALITY_RELEASE/A2_QUALITY_RELEASE_MANAGER.md` |

A2-FOUNDATION's merge carries no equivalent loss: core and ledger were mutually dependent producers, not a producer and its checker.

## Open issues and gaps — preserved and remapped

| Item | Owner | Blocking level |
|---|---|---|
| `OI-001` runtime / library baseline | A2-FOUNDATION | Blocks all A3; A1 approves |
| `OI-002` canonical serialization | A2-FOUNDATION | Blocks all A3; A1 freezes |
| `OI-003` Claude fallback invocation | A2-TRUST + A2-CLAUDE-INTEGRATION; **A1 signs off** | Blocks Claude-fallback A3 |
| `OI-004` permission deny rules | A2-CLAUDE-INTEGRATION + A2-TRUST | Blocks M3 permission A3 |
| `OI-005` recipe-approval UX | A2-VERIFICATION + A2-TRUST | Blocks approval-path A3 |
| `OI-006` name collision check | A2-QUALITY-RELEASE | Release only (RG-9) |
| `OI-007` demo ecosystem / fixtures | A2-QUALITY-RELEASE | M6 only |
| `OI-008` Gemini provider | A2-TRUST | Deferred / optional |
| `OI-009` worktree-hook re-verification | A2-CLAUDE-INTEGRATION | Post-MVP; both hooks stay uninstalled |
| **`GAP-001`** `CONTRACT-ERROR-001` has no file | A2-CLAUDE-INTEGRATION proposes; A2-FOUNDATION positions; **A1 decides** | Before first M0 A3 depending on cross-component typed errors |
| **`GAP-002`** `CONTRACT-PROCESS-001` has no file, yet frozen `CONTRACT_PLUGIN_001.md` cites it by name | A2-VERIFICATION escalates; **A1 decides** | Before first M1 runner A3 |

Neither gap was silently closed, and no manager may invent a missing frozen contract in a task packet.

## ADR-001 — binding on every manager

`architecture-decisions/ARCHITECTURE_DEVIATION_REQUEST_001.md` is **APPROVED**. Receipts installs **no `WorktreeCreate` hook** and **no `WorktreeRemove` hook**, does not own worktree creation, never replaces Claude Code's normal worktree implementation, and observes workspace identity only through `cwd`, repository identity, read-only Git metadata, and normal broker invocation context. Invalidation is lazy at next session start. A2-CLAUDE-INTEGRATION carries a packaging test that fails if either hook ever appears in `hooks/hooks.json`.

## A3 and A4 context principles

**A3** receives one atomic task, the relevant architecture section(s), the required frozen contract(s), the relevant source files, exact file ownership, exact acceptance criteria, and exact test requirements — **not** the full global project context.

**A4** receives the task specification, architecture requirements, frozen contracts consumed, the exact A3 commit, the diff, the tests, and the A3 handoff evidence. A4 independently returns `PASS`, `PASS_WITH_NONBLOCKING_FINDINGS`, or `REJECT`, and **must not be the implementation session**.

**Merging managers must not merge atomic implementation tasks.** Every manager file states this and every proposed A3 decomposition in the first-task deliverables is required to honor it.

## PACKAGE_GENERATION_STATE — now

| Item | State |
|---|---|
| Active A1 | **A1-BOOTSTRAP** |
| `A1-RUNTIME` | **NOT INITIALIZED** |
| Authority transfer | **NOT PERFORMED** |
| `CONTRACT_FREEZE_SHA` | Known |
| `AGENT_SYSTEM_FREEZE_SHA` | **NOT YET ASSIGNED** |
| This package installed / committed / pushed | **NO** |
| A2 integration branches | **NOT CREATED** |
| A2 integration worktrees | **NOT CREATED** |
| A2 managers initialized | **NO** |
| A3 implementation | **BLOCKED** |
| A4 review | **NOT STARTED** |
| Contract freeze | READY / FROZEN, 21 contracts at 1.0.0 |
| MCP | **NOT REQUIRED FOR MVP** (`D-003`) |

## EXPECTED_RUNTIME_INITIALIZATION_STATE — later

| Item | Expected state |
|---|---|
| All orchestration artifacts | Frozen, committed, pushed to `main` |
| `AGENT_SYSTEM_FREEZE_SHA` | Known and recorded |
| `A1-RUNTIME` | Initialized against `AGENT_SYSTEM_FREEZE_SHA` |
| Authority transfer | Formally accepted; `A1-BOOTSTRAP` RETIRED |
| A2 integration branches and worktrees | Created and validated by `A1-RUNTIME` |
| A2 managers | Initialized against their supplied `A2_START_SHA` |
| A3 implementation | **Still gated** — opens only per authorized implementation wave |

The remaining work before that transition — the M0–M7 atomic execution DAG, the A3 implementation protocol, the A4 independent-review protocol, the Git branch/worktree execution protocol, integration waves and gates, the runtime-A1 operating package, and the formal authority-transfer package — is **not** produced by this package and is not authorized by it.
