# 00 — A1 Receipts Decomposition and Index

**Role:** A1-RECEIPTS — Principal Build Orchestrator and System Integration Manager  
**Architecture authority:** `Receipts_Final_Architecture(1).md`  
**Control instruction:** `Pasted text(20260809-041820).txt`  
**Architecture date:** 9 August 2026  
**A1 phase:** Build-control bootstrap only. **No source implementation, repository creation, branch creation, dependency installation, A3 prompt, or A4 prompt is authorized.**

## System abstraction

Receipts is a durable, code-state-bound evidence ledger and admission engine for Claude Code work:

**CLAIMS → EVIDENCE → POLICY → ADMISSION**

The broker, not the coding agent, is the evidence authority. Deterministic checks are broker-run under approved recipes; review output remains probabilistic evidence; all evidence is exact-code-state-bound; stale evidence loses validity; admission is derived and enforced only on Claude-Code-mediated surfaces.

## Decomposition decision

The eight proposed A2 boundaries are **accepted without merge, split, or rename**.

| Manager | Decision | Primary ownership | Explicit exclusions |
|---|---|---|---|
| A2-CORE | KEEP | Repository identity; CodeStateFingerprint; task/claim domain model; claim status derivation; whole-tree staleness; VerificationPolicy semantics; pure `admit()`; changed-path cause calculation; git adapter semantics needed by fingerprinting. | SQLite/event persistence; subprocess runner; provider CLIs; hook packaging; override UI; benchmark harness. |
| A2-LEDGER | KEEP | Append-only LedgerEvent spine; canonical event serialization; hash chain; SQLite schema and WAL settings; projections/rebuild; transaction boundaries; `verify-ledger`; export mechanics and hash verification. | Domain admission rules; recipe execution; review-provider selection; hook behavior. |
| A2-RUNNER | KEEP | VerificationRecipe schema/semantics; human approval state; recipeDigest; safe argv process execution; cwd/executable resolution; ExecutionReceipt; raw logs; timeout handling; per-(repoId, recipeKey) lock/concurrency; flaky consecutive-run signal storage. | Admission policy; reviewer model calls; Claude hooks; override semantics. |
| A2-CLAUDE-INTEGRATION | KEEP | Plugin manifest; hooks.json; hook normalization; hook/CLI decision adapters; skills; custom reviewer-agent packaging; permission configuration; L1/L2 Claude gates; factual additionalContext; status rendering. | Core policy decisions; ledger storage internals; provider semantics; benchmark oracles. |
| A2-REVIEW | KEEP | ReviewRequest/ReviewResult/ReviewFinding; ReviewProvider interface; Codex provider; Claude-session fallback; provider configuration; health/capabilities; structured parsing; read-only enforcement; provider selection facts and downgrade production. | Admission's final policy decision; deterministic proof; ledger persistence internals. |
| A2-INTEGRITY-SECURITY | KEEP | Trust-boundary requirements; test-change integrity signals; command/path safety requirements; ledger-path protection requirements; recipe/policy protection requirements; override/waiver semantics; prompt-injection-safe context rules; security tests; enforcement-scope audit. | Hook packaging details; SQLite implementation; provider implementation; benchmark execution. |
| A2-EVALUATION | KEEP | 12 benchmark tasks and oracles; arms A–E (F optional); reproducible reset fixtures; repeated-run harness; metrics; raw results; result-integrity checks; no-significance guard. | Product implementation; release prose beyond measured evaluation outputs. |
| A2-DOCS-RELEASE | KEEP | README; architecture/trust/enforcement/provider/hook docs; install/demo docs; name-collision evidence; release checklist; evaluation-report publication from measured M6 outputs only. | Inventing benchmark results; changing architecture; implementing runtime logic. |

Why no structural change:
- The architecture's internal decomposition already separates pure core, persistence, runner, providers, integration, integrity, evaluation, and documentation.
- Each manager has one coherent authority boundary and a testable artifact surface.
- Cross-cutting concerns are handled through formal contracts rather than shared ownership.
- Splitting A2-CORE would make the admission/fingerprint/staleness invariant span managers unnecessarily.
- Merging evaluation or documentation into implementation would weaken independence and increase the risk of overstated results.

## Authoritative milestone sequence

`M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7`

No A3 implementation task may start until every contract it consumes is frozen and its component-specific open issues are cleared.

## Build-control package

1. `00_AGENT1_DECOMPOSITION_AND_INDEX.md`
2. `01_ARCHITECTURE_AUTHORITY.md`
3. `02_COMPONENT_OWNERSHIP.md`
4. `03_IMPLEMENTATION_DEPENDENCY_GRAPH.md`
5. `04_CONTRACT_INDEX.md`
6. `05_MILESTONE_PLAN.md`
7. `06_TASK_LEDGER.md`
8. `07_COMPONENT_STATUS.md`
9. `08_DEPENDENCY_REQUESTS.md`
10. `09_DECISION_LOG.md`
11. `10_OPEN_ISSUES.md`
12. `11_INTEGRATION_GATES.md`
13. `12_EVIDENCE_REQUIREMENTS.md`
14. `13_RELEASE_GATES.md`
15. `14_AGENT_HANDOFF_PROTOCOL.md`
16. `15_ARCHITECTURE_DEVIATION_PROTOCOL.md`

Plus `A1_RECEIPTS_ORCHESTRATION_REPORT.md` (index/summary), `README_PACKAGE.md`, and `MANIFEST.sha256` (integrity manifest for this package).

## Architecture correction of record — ADR-001 (APPROVED)

This package was first produced at A1 bootstrap, when no architecture deviation had been identified. `ARCHITECTURE_DEVIATION_REQUEST-001` was subsequently discovered during contract freeze and **approved by the architecture authority on 2026-08-09**. The build-control package was then reconciled so that it and the contract-freeze package tell one consistent history.

Approved correction, in force throughout this package:

1. Receipts **MUST NOT** install a `WorktreeCreate` hook in MVP.
2. Receipts **does not own worktree creation**; Claude Code and Git remain responsible.
3. Receipts **MUST NOT** replace Claude Code's default Git worktree behavior, and implements no custom worktree creation.
4. Workspace identity is bound **observationally** from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations from the active working directory.
5. `WorktreeRemove` is also omitted from the MVP installed hook set after current-documentation verification; it is not retained for symmetry, and workspace cleanup remains Claude Code's / Git's responsibility.
6. **No other architecture semantics changed.**

The full record is `ARCHITECTURE_DEVIATION_REQUEST_001.md` in the contract-freeze package; the register is `15_ARCHITECTURE_DEVIATION_PROTOCOL.md`.

## A1 bootstrap decision

**GO for component-manager initialization.**

This is **not** a GO for coding. A2 managers may now be initialized to inspect their component, validate current mutable interfaces, resolve their listed open issues, and prepare bounded A3 tasks. A3 remains contract- and issue-gated.

## MCP decision

**MCP NOT REQUIRED FOR MVP.**

Hooks already observe/control Claude lifecycle events; skills provide human/model invocation surfaces; the short-lived receipts CLI is the broker entry point; SQLite is the state authority. A local MCP server would duplicate authority and add another invocation path without satisfying a missing MVP requirement.
