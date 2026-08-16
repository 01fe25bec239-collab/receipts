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

# CODEX_PLUGIN_PACKAGING

Hook-definition changes, trust changes and enablement/configuration changes trigger freshness revalidation under `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`.

**Verification status: the central fact is now VERIFIED.** Registry claims `C-01` and `C-02` are `VERIFIED_CURRENT_SELF_FETCHED` — I retrieved the current OpenAI plugin and hooks documentation directly on 2026-08-15.

V1.3 recorded this as `USER_DECLARED` and specified two equally-weighted paths. That is corrected: **the native plugin path is now primary.**

## Primary architecture — native Codex plugin

```
plugins/codex/
├── .codex-plugin/
│   └── plugin.json          required manifest; may carry a `hooks` entry
├── skills/                  product skills
├── hooks/
│   └── hooks.json           default plugin hook location
└── bin/                     thin bridge to shared local core
```

`.app.json` and `.mcp.json` are supported by the platform but are **not** included: neither earns its place, and `MCP_POSITION.md` explicitly rejects adding MCP for symmetry.

Manifest hook entries may be a `./`-prefixed path, an array of paths, an inline hooks object, or an array of them; paths resolve relative to the plugin root and must stay inside it. If the manifest defines `hooks`, Codex uses those entries **instead of** the default `hooks/hooks.json`.

## Verified hook events

`SessionStart` · `SessionEnd` · `SubagentStart` · `SubagentStop` · `PreToolUse` · `PermissionRequest` · `PostToolUse` · `PreCompact` · `PostCompact` · `UserPromptSubmit` · `Stop`

This is a materially richer surface than V1.3 assumed, and maps cleanly onto `NormalizedHostEvent`.

## Three verified constraints that change the design

**1. Plugin hooks are not trusted on install (`C-02a`).** Installing a plugin does not automatically trust its hooks. Codex skips plugin-bundled hooks until the user reviews and trusts the exact definition, and records trust against the hook definition's **hash** — so **new or changed** hook definitions/hashes are re-marked for review and skipped until trusted. A plugin update that does not change the hook definition/hash does not by itself require re-trust.

Consequence: there is a real state where our plugin is installed and its hooks are **silently inert**. The install flow must detect it, say so plainly, and direct the user to `/hooks`. Treating installation as implying activation would produce a product that appears broken for a reason the user cannot see.

**2. Hooks can be switched off entirely (`C-02b`).** `[features] hooks = false` disables them; `allow_managed_hooks_only = true` skips plugin hooks while keeping managed ones; `--dangerously-bypass-hook-trust` exists for one-off automation.

**3. Specialized tool paths can opt out (`C-02c`).** OpenAI's own documentation states hooks are *a useful guardrail, not a complete enforcement boundary*.

Together these are decisive: **entitlement and security authority stay in the shared core.** Not as a precaution — because the host documentation says the hook layer is bypassable, disableable, and untrusted by default.

## Operational limits (`C-02d`)

`SessionEnd` runs synchronously with a 1s default and **3s maximum** — no meaningful work belongs there; persist during the session, not at exit. Model-visible hook output is capped near 2500 tokens and spills to disk beyond that, so hook-facing output must stay bounded. Plugin hooks receive `PLUGIN_ROOT` and `PLUGIN_DATA`, and Codex also sets `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` for compatibility.

## Compatibility fallback — supervised / hybrid

Retained, not primary. Selected by `HOST_CAPABILITY_DISCOVERY.md` for: older Codex versions without plugin hook support; hooks disabled by config or policy; plugin hooks untrusted; events with no hook coverage; and **worker-runtime operations that still require `codex exec`**.

The last is permanent: a Codex *worker* is dispatched through the runtime adapter regardless of how the *host* integrates. Host integration and worker dispatch are different questions.

## Distribution

Public plugins publish once to the universal directory shared by ChatGPT and Codex. Local and repo marketplaces support authoring, testing and private distribution (`C-01`).

## Entitlement UX

Same capability IDs, same entitlement, same core admission as Claude. The plugin displays; the core decides — reinforced by the fact that the hook layer here is explicitly not an enforcement boundary.
