# 01 — Architecture Authority

## Binding source

`Receipts_Final_Architecture(1).md` is authoritative. Sections A–Z and the closing falsification section are binding design authority.

Architecture status: **implementation-ready, no code written**.

The control package does not "improve" or rewrite architecture. It extracts implementation ownership, contracts, gates, and evidence requirements from it.

## Non-negotiable invariants

1. Agents may ASSERT claims but cannot prove their own claims.
2. Every evidence item is bound to exactly one CodeStateFingerprint.
3. Evidence is valid only while its fingerprint matches current state and its recipe/schema compatibility remains valid.
4. Deterministic evidence and model-review evidence are different families and cannot substitute for each other.
5. Admission is derived from policy + claim statuses + current fingerprint; stored admissions are audit artifacts, not truth.
6. Worker agents cannot write to the evidence ledger; the broker is the sole writer.
7. Verification commands come only from approved VerificationRecipes; agent-supplied commands are never execution authority.
8. Human override is always available, human-controlled, fingerprint-scoped, and permanently recorded.
9. `ADMITTED_WITH_OVERRIDE` must never be rendered as `ADMITTED`, `VERIFIED`, or `PROVED`.
10. Receipts governs Claude-Code-mediated actions only; it does not claim to stop a human using another terminal.
11. Model/provider identity is configuration, not architecture.
12. Git worktrees are workspace isolation, not security isolation.
13. MVP topology is Claude Code hooks → short-lived receipts CLI → SQLite. No daemon in MVP.
14. Receipts must not expand into a generic multi-agent orchestrator or resurrect an AgentAdapter/FAM architecture.
15. `ReviewProvider` remains deliberately small and cannot become a general agent runtime.
16. MCP is not introduced unless an existing hooks/skills/CLI boundary cannot satisfy a concrete requirement.
17. Any architecture change caused by mutable external capabilities requires an explicit ARCHITECTURE_DEVIATION_REQUEST and approval.

## Current primary-source verification (accessed 9 August 2026)

Mutable external interfaces were re-checked before freezing implementation-facing contracts.

- Claude Code plugins: https://code.claude.com/docs/en/plugins
- Claude Code plugins reference: https://code.claude.com/docs/en/plugins-reference
- Claude Code hooks reference: https://code.claude.com/docs/en/hooks
- Claude Code permissions: https://code.claude.com/docs/en/permissions
- Claude Code skills: https://code.claude.com/docs/en/skills
- Claude Code custom subagents: https://code.claude.com/docs/en/sub-agents
- Claude Code programmatic/headless mode: https://code.claude.com/docs/en/headless
- Claude Code CLI reference: https://code.claude.com/docs/en/cli-usage
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive/
- Codex CLI reference: https://developers.openai.com/codex/cli/reference/

Freshness findings:
1. The documented Claude plugin layout remains compatible with the architecture: `.claude-plugin/plugin.json` at the plugin root, plus root-level `skills/`, `agents/`, and `hooks/`.
2. Claude command hooks still support exec form through `command` + `args`; with `args`, no shell is involved.
3. `TaskCompleted`, `PostToolBatch`, `SubagentStart`, `SubagentStop`, `WorktreeCreate`, and `WorktreeRemove` remain documented hook events. `TaskCompleted` can block completion with exit code 2. **Corrected by ADR-001 (APPROVED 2026-08-09):** this bootstrap reading of the worktree events was incomplete. Configuring `WorktreeCreate` *replaces* Claude Code's default Git worktree creation, requires the hook to create and return the worktree path, and aborts creation on any non-zero exit. `WorktreeRemove` grants no decision control and its failures are logged in debug mode only. Receipts installs neither hook in MVP.
4. `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` remain documented plugin path variables available to hook processes.
5. Codex still supports `codex exec`, `--sandbox read-only`, `--json`, `--output-schema`, `-o/--output-last-message`, `--ignore-user-config`, `--ignore-rules`, and `--skip-git-repo-check`.
6. Current Codex CLI documentation now describes `--full-auto` as a deprecated compatibility flag. Receipts already forbids passing `--full-auto`, so this does not require an architecture deviation.
7. Claude plugin subagents ignore `permissionMode`, `hooks`, and `mcpServers` frontmatter. Read-only behavior for the plugin reviewer must therefore be established through its explicit tool allowlist and through the provider invocation boundary, not through plugin-subagent `permissionMode`.
8. Claude `-p` supports JSON output and JSON-schema structured output. The exact same-vendor fallback invocation remains an A2-REVIEW implementation-freeze item because it must avoid accidental loading/recursion of Receipts hooks while preserving the intended local authentication path.

Conclusion at the time of A1 bootstrap: no architecture deviation had been identified. **This conclusion was superseded during contract freeze.** `ARCHITECTURE_DEVIATION_REQUEST-001` was raised against the `WorktreeCreate` hook mapping and was **APPROVED** by the architecture authority on 2026-08-09. The approved minimal correction removes `WorktreeCreate` (and, after current-doc verification, `WorktreeRemove`) from the MVP installed hook set and binds workspace identity observationally. No other architecture semantics changed. See `15_ARCHITECTURE_DEVIATION_PROTOCOL.md` and `ARCHITECTURE_DEVIATION_REQUEST_001.md` in the contract-freeze package.

## Freshness/change-control rule

A mutable external capability is allowed to change implementation syntax without an architecture deviation **only** when the architecture's semantics remain intact. If current primary documentation makes a documented architecture requirement impossible, work stops and `ARCHITECTURE_DEVIATION_REQUEST` is created under `15_ARCHITECTURE_DEVIATION_PROTOCOL.md`.

## Current architecture deviation status

**ONE ARCHITECTURE DEVIATION EXISTS: `ADR-001` — APPROVED.**

`ADR-001` was **not** identified at A1 bootstrap. It was discovered during contract freeze, when the plugin hook contracts were written against current official Claude Code documentation, and it was **approved by the architecture authority on 2026-08-09**.

Approved minimal correction:

1. Receipts **MUST NOT** install a `WorktreeCreate` hook in MVP.
2. Receipts **does not own worktree creation**; Claude Code and Git remain responsible.
3. Receipts **MUST NOT** replace Claude Code's default Git worktree behavior and **MUST NOT** implement custom worktree creation.
4. Workspace identity is bound observationally from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations from the active working directory.
5. `WorktreeRemove` was re-verified against current official documentation and is **also omitted** from the MVP installed hook set. It is not retained for symmetry; workspace cleanup remains Claude Code's / Git's responsibility and workspace-binding invalidation is lazy at next session start.
6. **No other architecture semantics changed.** Invariants 1–17 above are unchanged, and `CLAIM → EVIDENCE → POLICY → ADMISSION`, `CodeStateFingerprint`, evidence authority, staleness, `VerificationRecipe`, `ExecutionReceipt`, `ReviewProvider`, the L1 `TaskCompleted` gate, the L2 `PreToolUse` gate, the CLI → SQLite broker topology, the security/trust model, and the evaluation architecture are all untouched.

The full record is `ARCHITECTURE_DEVIATION_REQUEST_001.md` in the contract-freeze package. `OI-009` tracks post-MVP re-verification of the worktree events by local smoke test.

Two other current-documentation findings are **not** deviations. The current Codex `--full-auto` documentation changed, but the architecture already requires that Receipts never pass that flag. Current Claude plugin-subagent limits affect how read-only is implemented, not the required read-only behavior itself.

## MVP boundary

In scope remains: one repo, one machine, one demo ecosystem, four MVP claim types (`IMPLEMENTED`, `TESTED`, `LINT_CLEAN`, `REVIEWED`), short-lived CLI broker, SQLite, whole-tree staleness, Codex reviewer plus Claude-session fallback, L1/L2 Claude enforcement, integrity signals, override, ledger verification, and export.

Out of MVP remains: daemon, dependency/scoped staleness, mutation testing, OTel export, cryptographic signatures/external trust anchor, multi-repo, remote reviewers, web UI, CI L4 integration, learned routing, and exercised advanced claim types.
