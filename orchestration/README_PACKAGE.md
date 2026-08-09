# Receipts A1 Build-Control Package

Generated from:
- `Receipts_Final_Architecture(1).md` (authoritative architecture)
- `Pasted text(20260809-041820).txt` (A1 orchestration instruction)

Date: 9 August 2026 (revised same day for ADR-001 reconciliation)

Status:
- Architecture read completely.
- A1 build-control phase completed.
- Eight A2 managers retained.
- Semantic cross-component contracts frozen.
- Current Claude/Codex primary docs re-checked.
- **One architecture deviation on record: ADR-001 — APPROVED.** Discovered during contract freeze, not at bootstrap. Receipts does not install a `WorktreeCreate` hook, does not own worktree creation, and binds workspace identity observationally. `WorktreeRemove` is also omitted from the MVP installed hook set. No other architecture semantics changed.
- Contract freeze status: **READY / FROZEN**; `CONTRACT-PLUGIN-001` and `CONTRACT-PLUGIN-002` are 1.0.0 FROZEN.
- **GO for A2 manager initialization.**
- **NO GO for A3 implementation yet.**
- No repository, branch, worktree, dependency, source implementation, A3 prompt, or A4 prompt was created by this phase.

Start with `A1_RECEIPTS_ORCHESTRATION_REPORT.md`, then the numbered control files. `MANIFEST.sha256` lists the SHA-256 of every file in this package; verify with `sha256sum -c MANIFEST.sha256` from the package root.
