# 10 — Open Issues

| Issue | Blocking level | Question | Resolution owner / closure |
|---|---|---|---|
| OI-001 | BLOCKS A3-LEDGER | Select Node/TypeScript runtime baseline, package manager, build/test framework, and SQLite driver. Architecture fixes topology/semantics, not library choice. | A2-LEDGER proposes; A1 approves before first ledger A3 task. |
| OI-002 | BLOCKS A3-LEDGER | Freeze canonical JSON serialization algorithm used by LedgerEvent hash chain so independent verification is byte-stable. | A2-LEDGER proposes deterministic canonicalization with fixtures; A1 freezes CONTRACT-LEDGER-001 serialization appendix. |
| OI-003 | BLOCKS A3-REVIEW-CLAUDE-FALLBACK | Freeze same-vendor `claude -p` invocation that is read-only, separate-session, structured, and does not recursively load Receipts hooks while still supporting the intended local authentication path. | A2-REVIEW + A2-CLAUDE-INTEGRATION verify locally; A2-INTEGRITY-SECURITY signs off. |
| OI-004 | BLOCKS A3-CLAUDE-PERMISSIONS | Verify exact deny-rule representation for protecting project `.receipts/policy.yaml` and `.receipts/recipes.yaml`, and ledger-path access where the persistent path is outside the repo and dynamically rooted at CLAUDE_PLUGIN_DATA. | A2-CLAUDE-INTEGRATION + A2-INTEGRITY-SECURITY test current Claude version; freeze fixtures. |
| OI-005 | BLOCKS A3-RUNNER-APPROVAL | Choose concrete interactive human recipe-approval UX and persistence representation without allowing an agent to manufacture approval. | A2-RUNNER + A2-INTEGRITY-SECURITY propose; A1 freezes before implementation. |
| OI-006 | NONBLOCKING UNTIL RELEASE | Product name collision check across GitHub, npm, PyPI, crates.io, and web. `Receipts` is provisional. | A2-DOCS-RELEASE performs before name adoption/release. |
| OI-007 | NONBLOCKING UNTIL M6 | Exact demo language ecosystem and benchmark fixture implementation details; architecture indicates a small TypeScript demo but benchmark code must be authored reproducibly. | A2-EVALUATION proposes after product contracts are stable. |
| OI-008 | NONBLOCKING / DEFERRED | Gemini provider. MVP includes it only if implementation cost is under one day; no Gemini syntax is frozen at A1 bootstrap. | A2-REVIEW may propose after Codex + Claude fallback are complete. |
| OI-009 | NONBLOCKING / POST-MVP | Does configuring a `WorktreeRemove` hook also displace Claude Code's default worktree cleanup? Current official documentation gives it no decision control and does not say it replaces default behavior, but independent third-party integrations implement removal themselves. Unresolvable from documentation; needs a local version smoke test. | A2-CLAUDE-INTEGRATION runs the smoke test post-MVP. Until then both worktree hooks stay uninstalled per ADR-001. Re-introduction requires a `CONTRACT_CHANGE_REQUEST` against PLUGIN-001/002. |

## What blocks component-manager initialization?

**Nothing currently blocks A2 initialization.**

The blocking issues above block specific future A3 implementation tasks, not A2 analysis. A2 initialization exists partly to resolve them.

## What does not count as a blocker?

- Gemini implementation: optional.
- Product-name adoption: release blocker, not architecture/build bootstrap blocker.
- Benchmark outcomes: must be measured later, never guessed now.
- CI L4 enforcement: explicitly deferred.
- OI-009 worktree-hook re-verification: Receipts ships with neither worktree hook installed, so the ambiguity has no MVP effect.

## Relationship to ADR-001

ADR-001 is **APPROVED and closed**; it is not an open issue. It is recorded as historical architecture evidence in `ARCHITECTURE_DEVIATION_REQUEST_001.md` and summarized in `15_ARCHITECTURE_DEVIATION_PROTOCOL.md`. None of the issues above is an architecture blocker: OI-001 through OI-005 block specific future A3 tasks, and OI-006 through OI-009 block nothing before release or post-MVP. **No open issue blocks contract freeze.**
