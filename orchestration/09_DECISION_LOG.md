# 09 — Decision Log

| Decision | Choice | Rationale | State |
|---|---|---|---|
| D-001 | KEEP the eight A2 manager boundaries | Matches real architectural cohesion; no merge/split/rename adds value. | FROZEN |
| D-002 | MVP broker remains short-lived CLI → SQLite, no daemon | Architecture Q/Z binding. | FROZEN |
| D-003 | MCP NOT REQUIRED FOR MVP | Hooks + skills + CLI satisfy all model/user invocation needs; MCP would duplicate authority. | FROZEN |
| D-004 | Codex reviewer invocation explicitly uses read-only sandbox and never `--full-auto` | Architecture P + current Codex docs; current docs mark --full-auto deprecated. | FROZEN |
| D-005 | Claude plugin layout uses root `.claude-plugin/plugin.json`, `hooks/`, `skills/`, `agents/` | Current official Claude plugin docs agree with architecture Y. | FROZEN |
| D-006 | Plugin reviewer read-only authority cannot depend on plugin-subagent `permissionMode` | Current Claude docs say plugin subagents ignore permissionMode; use explicit tool allowlist/provider boundary. | FROZEN |
| D-007 | Do not freeze exact Claude fallback process argv at A1 | Need to avoid hook recursion while preserving local auth; OI-003 assigned to A2-REVIEW. | OPEN IMPLEMENTATION DECISION, semantics frozen |
| D-008 | Whole-tree evidence invalidation in MVP | Architecture M; false rerun preferred over false green. | FROZEN |
| D-009 | Stored Admission never source of truth | Architecture L; recomputation wins. | FROZEN |
| D-010 | A2-DOCS-RELEASE remains independent from A2-EVALUATION | Prevents prose from becoming evidence and protects 'no invented results'. | FROZEN |
| D-011 | A3/A4 branches/worktrees are future workspace mechanics only, not security boundary | Architecture invariant; no branch/worktree created during A1 bootstrap. | FROZEN |
| D-012 | Receipts does not install a `WorktreeCreate` hook and does not own worktree creation; workspace identity is bound observationally | ADR-001, APPROVED 2026-08-09. Configuring `WorktreeCreate` replaces Claude Code's default Git worktree creation and requires the hook to create/return the path, so no observational handler is possible; taking over creation would violate invariants 12 and 14. | FROZEN (ADR-001 APPROVED) |
| D-013 | `WorktreeRemove` is also omitted from the MVP installed hook set | Current docs give it no decision control and no MVP requirement depends on early removal notification; independent third-party integrations suggest configuring it may also displace default cleanup, and that conflict is unresolved without a local smoke test. Not retained for symmetry. Invalidation is lazy at next session start. | FROZEN; OI-009 tracks post-MVP re-verification |

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
