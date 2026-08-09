# 06 — Task Ledger

| Task | Owner | Purpose | Status | Gate / evidence |
|---|---|---|---|---|
| A1-RECEIPTS-BOOTSTRAP-001 | A1 | Build-control package from frozen architecture | COMPLETE | This package |
| A1-RECEIPTS-CONTRACT-FREEZE-001 | A1 | Freeze cross-component contracts against current external documentation | COMPLETE | Contract-freeze package; raised ADR-001 |
| A1-RECEIPTS-ADR-001-001 | A1 | Raise, verify, and obtain approval for ADR-001 (WorktreeCreate observation conflict) | COMPLETE — APPROVED | `ARCHITECTURE_DEVIATION_REQUEST_001.md`, STATUS APPROVED 2026-08-09 |
| A1-RECEIPTS-ADR-001-RECONCILE-001 | A1 | Reconcile the build-control and contract-freeze packages to one consistent post-ADR-001 history; finalize PLUGIN-001/002 to 1.0.0 FROZEN; regenerate SHA-256 manifests | COMPLETE | This corrected package; verified manifests |
| A2-CORE-INIT-001 | A2-CORE | Initialize component manager; validate CORE contracts and OI dependencies | READY | A1 bootstrap complete |
| A2-LEDGER-INIT-001 | A2-LEDGER | Initialize component manager; resolve OI-001/OI-002 proposal | READY | A1 bootstrap complete |
| A2-RUNNER-INIT-001 | A2-RUNNER | Initialize component manager; resolve OI-005 and process contract details | READY | A1 bootstrap complete |
| A2-CLAUDE-INTEGRATION-INIT-001 | A2-CLAUDE-INTEGRATION | Initialize; revalidate hook/plugin/permission fixtures; resolve OI-004 proposal; implement the ADR-001 hook set with no worktree hooks and observational workspace binding | READY | A1 bootstrap complete; PLUGIN-001/002 1.0.0 FROZEN |
| A2-REVIEW-INIT-001 | A2-REVIEW | Initialize; freeze Codex provider details and resolve OI-003 | READY | A1 bootstrap complete |
| A2-INTEGRITY-SECURITY-INIT-001 | A2-INTEGRITY-SECURITY | Initialize trust/security manager; review OI-003/004/005 and security tests | READY | A1 bootstrap complete |
| A2-EVALUATION-INIT-001 | A2-EVALUATION | Initialize evaluation manager; define reproducibility/oracle package without running results | READY | A1 bootstrap complete |
| A2-DOCS-RELEASE-INIT-001 | A2-DOCS-RELEASE | Initialize docs/release manager; prepare truthfulness/collision/release gates | READY | A1 bootstrap complete |
| A3-* | A3 | All coding tasks | BLOCKED | No A3 prompt until its contracts + milestone inputs + open issues are cleared |
| A4-* | A4 | Independent code review | NOT_ISSUED | Only after an A3 implementation exists |

## Ledger discipline

- A1 is the only authority that changes milestone integration state.
- A2 may create component-local proposed task IDs, but an A3 task is not executable until it appears here (or in the future repository copy) as READY with frozen contract references.
- A4 review is mandatory for every A3 implementation task and must be a different agent/session.
- No source implementation task is authorized by this bootstrap package.
