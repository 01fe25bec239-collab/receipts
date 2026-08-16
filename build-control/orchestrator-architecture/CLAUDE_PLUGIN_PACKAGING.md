<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# CLAUDE_PLUGIN_PACKAGING

**Verification status:** current. Registry claims `C-04` (plugin structure), `C-05` (hooks and `WorktreeCreate` semantics) and `C-07` (headless `claude -p`) are `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE`, accessed 2026-08-15.

Capability probing at install is **retained regardless** (`HOST_CAPABILITY_DISCOVERY.md`): current evidence is not a permanent assumption, and a host can change between our release and the user's install.

## Proposed layout

```
plugins/claude/
├── .claude-plugin/
│   └── plugin.json          manifest
├── skills/                  graph, status, capabilities, product login
├── hooks/
│   └── hooks.json           lifecycle → NormalizedHostEvent
├── commands/                namespaced product commands
└── bin/                     thin bridge to shared local core
```

**Deliberately omitted unless justified:** `agents/` (no packaged subagent is required at MVP — workers are dispatched through runtime adapters, not host subagents) and `.mcp.json` (see `MCP_POSITION.md`; not added for symmetry).

Use the minimum architecture required. Every component shipped is a component that must be maintained against a moving host.

## The shell contains no authority

```
Claude plugin  →  bin bridge  →  SHARED LOCAL CORE
```

No graph semantics, no routing engine, no entitlement rules, no review logic, no goal-completion logic in any Claude-specific file. A dependency-direction test enforces this: `plugins/claude/**` may not be imported by `src/core/**`.

## Worktree handler

Per C-05, configuring `WorktreeCreate` **replaces** Claude Code's default git worktree logic and the handler must return the created path. Because the orchestrator owns workspace lifecycle, this is the right mechanism — but the handler is therefore **correctness- and latency-critical**, sits on an interactive path, and must fall back to a plain `git worktree` if the core is unreachable so the user is never blocked.

## Operational concerns

**Install / update / uninstall:** through the host's supported plugin mechanism; the core is installed separately and version-checked. **Local development loading:** local path install for iteration. **Marketplace distribution:** available as a channel; the product does **not** depend on marketplace billing (C-13 unverified). **Hook trust:** hooks run with user privileges — the plugin declares exactly what it registers, and the core is the only component with state-write authority. **Plugin data path:** used for host-scoped cache only; entitlement and graph state live in user/install-scoped and project-scoped locations respectively.

## Command namespace

Namespaced product commands, never a reserved built-in. `START_GOAL` is the semantic operation; exact syntax is **Q-01, reopened** and must be verified before freezing (§60).

## Entitlement UX

Login, status and capability catalog are rendered by the plugin from core-supplied structure. The plugin **displays**; the core **decides**. A hidden command is not a gate.
