<!--
MultiAgent Orchestrator Architecture — HISTORICAL SNAPSHOT
DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT
SNAPSHOT: V1.1
Issued by: BUILD-A1-BOOTSTRAP
Status: PRESERVED HISTORICAL RECORD — NOT current architectural truth.
Working reconciliation record. Canonical statement: REPOSITORY_RECONCILIATION_REPORT.md.
This document records what was believed at the time it was written. Where it
disagrees with a CURRENT_NORMATIVE document, the current document governs.
It contributes NO current evidence assertion to normative validation.
-->

# RECONCILIATION_REPORT_V1_1

## Scope and evidence

V1 was written **without access to the committed repository**. It was built from artifacts generated earlier in the same working session plus the capability research pass. `REQUIREMENTS_TRACEABILITY_MATRIX` recorded this as the §3 gap.

That gap is now closed against two supplied artifacts, and **nothing outside them was consulted**:

| Artifact | What it establishes |
|---|---|
| `receipts-snapshot.zip` | `git archive HEAD` — exact tracked contents at HEAD. 74 files. |
| `git-state.txt` | Root, branch, remote, clean tree, HEAD, origin/main, ancestry, commits, inventory. |

**No direct GitHub or local-machine access was used or is claimed.** `git ls-remote` and the GitHub API were attempted earlier and both failed (private repo, no connector).

## Git state — verified

| Fact | Value | Status |
|---|---|---|
| HEAD | `3c70f4d8bac1732058de50b383f0485ab4632de9` | VERIFIED |
| HEAD == origin/main | yes | VERIFIED |
| Working tree | clean (empty porcelain) | VERIFIED |
| `CONTRACT_FREEZE_SHA` `2d2dbc2c…` ancestor of HEAD | yes (exit 0) | VERIFIED |
| `A2_DEFINITION_SHA` `3c70f4d8…` | **equals HEAD** | VERIFIED |
| Commits | 3: architecture → contracts/orchestration → five-manager A2 | VERIFIED |
| Tracked files | 74 | VERIFIED |

`A2_DEFINITION_SHA` being *identical* to HEAD confirms no commit followed the A2 freeze. The baseline V1 assumed is the baseline that exists.

## Findings

### R-01 — Two invariant lists exist. V1 cited the right one by the wrong name. **CORRECTED**

`Receipts_Final_Architecture.md` §C contains **10** core invariants.
`orchestration/01_ARCHITECTURE_AUTHORITY.md` contains **17** non-negotiable invariants — an expanded derivation by the A1 control package.

V1 (and the earlier frozen packages) cited invariant numbers 11–17: worktree-not-a-sandbox as 12, CLI-not-daemon as 13, no-generic-orchestrator as 14, `ReviewProvider`-stays-small as 15. **Those numbers are correct for the 17-item orchestration list and do not exist in the 10-item architecture list.**

So the citations were substantively right and referentially imprecise: V1 attributed them to "architecture invariants". Severity: **precision defect, not a correctness defect** — no design decision changes. Corrected in `ASSUMPTION_REGISTER`, `CONFLICT_RESOLUTION_LOG` (C-13), and `ADR_IMPACT_MATRIX`.

Mapping for reviewers:

| Concept | Architecture §C | Orchestration 01 |
|---|---|---|
| Broker-only ledger writes | 6 | 6 |
| Approved recipes only | 7 | 7 |
| Human override recorded | 8 | 8 |
| `ADMITTED_WITH_OVERRIDE` never proof | (inside 8) | **9** |
| Enforcement scope honest | **9** | **10** |
| Model/provider is configuration | **10** | **11** |
| Worktree ≠ security | — (§S) | **12** |
| CLI not daemon | — (§Q.1) | **13** |
| Not a generic orchestrator | — | **14** |
| `ReviewProvider` stays small | — | **15** |
| MCP not introduced | — | **16** |
| Deviation required for external change | — | **17** |

### R-02 — The architecture document still contradicts APPROVED ADR-001. **NEW, MATERIAL**

At HEAD, `Receipts_Final_Architecture.md` **still lists `WorktreeCreate` and `WorktreeRemove`**:

- §O hook mapping — including verbatim: *"this handler must be trivial, wrapped in a catch-all, and **always exit 0**"*
- §T item 8 — both events inside the MVP installed hook set

ADR-001 is `APPROVED` and states the exact opposite: neither hook is installed, and the always-exit-0 handler is impossible because a configured `WorktreeCreate` replaces default git worktree creation and must return a path.

The earlier reconciliation pass updated the contracts and the orchestration control files but was instructed not to modify the architecture document, so the divergence was left in place. It is real and it is in the committed baseline.

**Impact on this package:** `ADR_IMPACT_MATRIX.md` is *strengthened*, not weakened. The new architecture reinstalls `WorktreeCreate` — but for the opposite reason and with opposite handler semantics. Anyone reading §O at HEAD would implement a trivial always-exit-0 handler, which is exactly wrong under both ADR-001 and this package. Recorded as C-14.

### R-03 — Citations verified correct. **NO CHANGE**

| V1 claim | Verified |
|---|---|
| Section letters §C, §G, §I.2, §L, §M, §O, §P, §Q, §T, §V, §W, §Y, §Z | All present with the cited meanings |
| 22 MVP `ExecutionReceipt` fields | Exactly 22 in §I.2 |
| Four MVP claim types | `IMPLEMENTED`, `TESTED`, `LINT_CLEAN`, `REVIEWED` (§T.4) |
| 21 contracts + index, all `1.0.0 FROZEN` | Confirmed for all 21 |
| Decisions `D-001`…`D-013` | All present in `09_DECISION_LOG.md` |
| Receipt proves / does not prove | §I.3 matches `REVIEW_VERIFICATION_PROVENANCE.md` |
| Codex `--sandbox read-only --json`, `claude -p` fallback | §T.7 matches |
| Whole-tree staleness, LIGHT/STANDARD, HIGH_ASSURANCE config-only | §T.5, §T.6 match |

### R-04 — The installed A2 package is byte-identical to what this session produced. **VERIFIED**

All **28** files under `build-control/a2/` compare byte-identical to the V3 package generated earlier. `INSTALL_MANIFEST.sha256` is installed; `PACKAGE_MANIFEST.sha256` correctly is not. Directory renames to slugs were applied as specified.

No drift. The V1 assumptions about the A2 baseline were accurate.

### R-05 — Filename drift in a control document. **MINOR**

`orchestration/01_ARCHITECTURE_AUTHORITY.md` names its binding source `Receipts_Final_Architecture(1).md`. The committed file is `Receipts_Final_Architecture.md` — a leftover download suffix. Cosmetic; noted so a future automated context loader does not fail on it.

### R-06 — Invariants 16 and 17 were not addressed in V1. **CORRECTED**

Orchestration invariant **16** (MCP not introduced unless a concrete requirement cannot be met by hooks/skills/CLI) and **17** (external-capability-driven change requires a deviation request) were not explicitly dispositioned in V1.

Both are satisfied and now recorded: `MCP_POSITION.md` applies exactly the invariant-16 test; the reopen itself is the invariant-17 process, executed at the largest possible scale.

## Facts that could NOT be established from the supplied evidence

Named precisely rather than guessed, per instruction:

| Missing fact | Why it matters | How to establish |
|---|---|---|
| Installed CLI versions and `--help` output for `claude` / `codex` | A-05 and A-15 remain `UNVERIFIED`; adapters would be built against assumed flags | Run `claude --help`, `codex --help` on the machine and supply output |
| Which providers are authenticated, and under which plan | Determines whether `PERSONAL_LOCAL_MODE` is viable as designed | Supply `auth status` output per CLI |
| OS / Node version / machine specs | Q-03 (SQLite), sandbox availability, concurrency defaults | Supply `uname -a`, `node --version` |
| The target `SPEC.md` for the first goal | `MVP_SCOPE` and the north-star demo are described generically | Supply the spec |
| Whether `WorktreeRemove` displaces default cleanup (Q-02) | Not answerable from any document | Local smoke test against an installed build |

None of these blocks architecture review. Each blocks a specific implementation task.

## Net effect on V1

| | |
|---|---|
| Findings requiring architecture change | **0** |
| Findings requiring citation correction | 2 (R-01, R-06) |
| New material findings recorded | 1 (R-02) |
| Minor findings | 1 (R-05) |
| V1 claims verified correct | R-03, R-04 |

**No architectural decision in V1 was invalidated by the committed repository.** The BUILD-A2 topology, the invariant set, the routing model, the durable/ephemeral split, and the ADR-001 supersession all survive reconciliation unchanged.
