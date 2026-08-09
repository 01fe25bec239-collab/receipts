# 02 — Component Ownership

## Rule

Every source/config artifact has exactly one authoritative A2 owner. Contributors may propose changes through a dependency request, but cannot silently redefine another manager's contract.

| A2 | Decision | Owns | Does not own |
|---|---|---|---|
| A2-CORE | KEEP | Repository identity; CodeStateFingerprint; task/claim domain model; claim status derivation; whole-tree staleness; VerificationPolicy semantics; pure `admit()`; changed-path cause calculation; git adapter semantics needed by fingerprinting. | SQLite/event persistence; subprocess runner; provider CLIs; hook packaging; override UI; benchmark harness. |
| A2-LEDGER | KEEP | Append-only LedgerEvent spine; canonical event serialization; hash chain; SQLite schema and WAL settings; projections/rebuild; transaction boundaries; `verify-ledger`; export mechanics and hash verification. | Domain admission rules; recipe execution; review-provider selection; hook behavior. |
| A2-RUNNER | KEEP | VerificationRecipe schema/semantics; human approval state; recipeDigest; safe argv process execution; cwd/executable resolution; ExecutionReceipt; raw logs; timeout handling; per-(repoId, recipeKey) lock/concurrency; flaky consecutive-run signal storage. | Admission policy; reviewer model calls; Claude hooks; override semantics. |
| A2-CLAUDE-INTEGRATION | KEEP | Plugin manifest; hooks.json; hook normalization; hook/CLI decision adapters; skills; custom reviewer-agent packaging; permission configuration; L1/L2 Claude gates; factual additionalContext; status rendering. | Core policy decisions; ledger storage internals; provider semantics; benchmark oracles. |
| A2-REVIEW | KEEP | ReviewRequest/ReviewResult/ReviewFinding; ReviewProvider interface; Codex provider; Claude-session fallback; provider configuration; health/capabilities; structured parsing; read-only enforcement; provider selection facts and downgrade production. | Admission's final policy decision; deterministic proof; ledger persistence internals. |
| A2-INTEGRITY-SECURITY | KEEP | Trust-boundary requirements; test-change integrity signals; command/path safety requirements; ledger-path protection requirements; recipe/policy protection requirements; override/waiver semantics; prompt-injection-safe context rules; security tests; enforcement-scope audit. | Hook packaging details; SQLite implementation; provider implementation; benchmark execution. |
| A2-EVALUATION | KEEP | 12 benchmark tasks and oracles; arms A–E (F optional); reproducible reset fixtures; repeated-run harness; metrics; raw results; result-integrity checks; no-significance guard. | Product implementation; release prose beyond measured evaluation outputs. |
| A2-DOCS-RELEASE | KEEP | README; architecture/trust/enforcement/provider/hook docs; install/demo docs; name-collision evidence; release checklist; evaluation-report publication from measured M6 outputs only. | Inventing benchmark results; changing architecture; implementing runtime logic. |

## Source-tree ownership projection

| Path / surface | Owner |
|---|---|
| `src/core/fingerprint/` | A2-CORE |
| `src/core/policy/` | A2-CORE |
| `src/core/claims/` | A2-CORE |
| `src/adapters/git/` | A2-CORE |
| `src/core/ledger/` and storage implementation | A2-LEDGER |
| `src/adapters/runner/` | A2-RUNNER |
| `src/adapters/providers/` | A2-REVIEW |
| `src/core/integrity/` | A2-INTEGRITY-SECURITY |
| `src/entry/cli.ts`, `bin/receipts`, plugin packaging, hooks, skills | A2-CLAUDE-INTEGRATION |
| `schemas/recipe.schema.json`, `schemas/receipt.schema.json` | A2-RUNNER |
| `schemas/policy.schema.json` | A2-CORE |
| `schemas/finding.schema.json` | A2-REVIEW |
| `eval/**` | A2-EVALUATION |
| `docs/**`, `README.md` | A2-DOCS-RELEASE |

## Cross-boundary implementation rule

A source file is never "jointly owned." When behavior crosses a boundary, the producer implements to a frozen contract and the consumer tests against that contract. A1 controls integration and can reject boundary leakage.

## Special security rule

A2-INTEGRITY-SECURITY owns the **security requirement**, but not every file that implements it. For example, it defines the required deny behavior for protected configuration; A2-CLAUDE-INTEGRATION implements the Claude permission/hook mechanism and must pass the security acceptance tests.
