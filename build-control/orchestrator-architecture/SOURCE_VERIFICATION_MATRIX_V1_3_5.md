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

# SOURCE_VERIFICATION_MATRIX_V1_3_6

**This document is GENERATED from `evidence/SOURCE_CLAIM_REGISTRY.json`.** That registry is the single authority for evidence status; every other artifact is reconciled against it and checked by `evidence/validate_sources.py`. Editing a status here without editing the registry is a validation failure, not a correction.

## What changed from V1.3

**[HISTORICAL]** At V1.3 the §7 research had not been performed and everything was labelled conservatively. That gap is now **closed**, by two different routes:

1. **Self-fetched.** I independently retrieved the current OpenAI Codex hooks documentation this pass. Those claims carry `VERIFIED_CURRENT_SELF_FETCHED` — verified by me, not taken on report.
2. **Reviewer-supplied.** The remaining primary sources were supplied by independent review on 2026-08-15 with exact URLs. Those carry `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` and are **not** relabelled as my own work, per §1.

## Evidence labels

`VERIFIED_CURRENT_SELF_FETCHED` · `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` · `VERIFIED_HISTORICAL` · `INDEPENDENT_VERIFIED` · `USER_DECLARED` · `DESIGN_DECISION` · `ASSUMPTION` · `UNVERIFIED` · `POLICY_NEEDS_REVIEW`

## Claim register

| ID | Claim | Label | Org | Source | Accessed |
|---|---|---|---|---|---|
| `C-01` | Codex has a native plugin architecture with a required .codex-plugin/plugin.json manifest, optional skills/, .app.json, .mcp.json, assets and lifecycl… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/plugins/build/plugins | 2026-08-15 |
| `C-02` | Codex supports lifecycle hooks. Confirmed events: SessionStart, SessionEnd, SubagentStart, SubagentStop, PreToolUse, PermissionRequest, PostToolUse, P… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/codex/hooks | 2026-08-15 |
| `C-02a` | Plugin-bundled hooks are NOT automatically trusted. Codex skips them until the user reviews and trusts the exact hook definition, and records trust ag… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/codex/hooks | 2026-08-15 |
| `C-02b` | Hooks can be disabled entirely via [features] hooks = false, and allow_managed_hooks_only = true skips user, project, session and plugin hooks while k… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/codex/hooks | 2026-08-15 |
| `C-02c` | Specialized tool paths can opt out of the default hook path; hooks are a useful guardrail, not a complete enforcement boundary. PreToolUse may deny or… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/codex/hooks | 2026-08-15 |
| `C-02d` | Plugin hook commands receive PLUGIN_ROOT and PLUGIN_DATA; Codex also sets CLAUDE_PLUGIN_ROOT and CLAUDE_PLUGIN_DATA for compatibility with existing pl… | `VERIFIED_CURRENT_SELF_FETCHED` | OpenAI | https://developers.openai.com/codex/hooks | 2026-08-15 |
| `C-03` | **[HISTORICAL]** Obsolete 2026-08-13 observation about Codex lifecycle hooks; currently false per `C-02`. | `VERIFIED_HISTORICAL` | internal | internal research pass | 2026-08-13 |
| `C-04` | Claude Code plugins may contain .claude-plugin/plugin.json, skills, agents, hooks, MCP server config, LSP config, monitors, binaries and settings. Plu… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://code.claude.com/docs/en/plugins | 2026-08-15 |
| `C-05` | Claude Code exposes SubagentStart/SubagentStop and WorktreeCreate. Configuring WorktreeCreate replaces Claude Code's default worktree creation and the… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://code.claude.com/docs/en/hooks | 2026-08-15 |
| `C-07` | claude -p provides programmatic operation with structured output and session continuation. | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://code.claude.com/docs/en/headless | 2026-08-15 |
| `C-07a` | A previously announced 2026-06-15 Agent SDK billing change was paused; for now Agent SDK / claude -p / third-party app use may still draw from subscri… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan | 2026-08-15 |
| `C-10` | Anthropic: OAuth is intended for ordinary use by Claude subscription purchasers in Claude Code and native Anthropic applications. Developers building … | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://code.claude.com/docs/en/legal-and-compliance | 2026-08-15 |
| `C-11` | Codex supports Sign in with ChatGPT for subscription access and API-key authentication for usage-based access; codex login is the local ChatGPT sign-i… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | OpenAI | https://developers.openai.com/codex/auth | 2026-08-15 |
| `C-11a` | OpenAI consumer terms effective 2026-01-01 prohibit automatic or programmatic extraction of data/output generally for individual Services. Must be rea… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | OpenAI | https://openai.com/policies/terms-of-use/ | 2026-08-15 |
| `C-12` | Whether a paid third-party commercial orchestrator may drive a customer's consumer subscription as an external worker. | `POLICY_NEEDS_REVIEW` | none | — | — |
| `C-13-OPENAI` | OpenAI App Developer Terms (updated 2026-07-09) cover plugins, establish that the developer operates independently, that OpenAI does not guarantee lis… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | OpenAI | https://openai.com/policies/developer-apps-terms/ | 2026-08-15 |
| `C-13-ANTHROPIC` | A first-party Anthropic paid-plugin checkout mechanism. | `UNVERIFIED` | none | — | — |
| `C-13-ANTHROPIC-DIST` | Third-party and community Claude plugin distribution exists, an Anthropic community marketplace exists, and private repository marketplaces are suppor… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | Anthropic | https://code.claude.com/docs/en/plugins | 2026-08-15 |
| `C-14` | Public graph-engineering prior art exists covering host-neutral DAG execution across multiple coding-agent hosts, caching, quality gates, selective re… | `REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE` | public repositories | https://github.com/gwaghmar/graph (+2 more) | 2026-08-15 |
| `C-06` | codex exec flag surface details (--json, --output-schema, --sandbox modes). | `VERIFIED_HISTORICAL` | internal | internal research pass | 2026-08-13 |
| `C-20` | SQLite is adequate as the MVP state store including graph tables. | `DESIGN_DECISION` | internal | — | 2026-08-15 |
| `C-21` | One graph core with two execution policies. | `DESIGN_DECISION` | internal | — | 2026-08-15 |
| `C-22` | Open-core boundary: public shell plus Free policy, proprietary Pro modules. | `DESIGN_DECISION` | internal | — | 2026-08-15 |
| `C-23` | Host capability discovery remains, with the native path primary where verified. | `DESIGN_DECISION` | internal | — | 2026-08-15 |

## Three findings the reviewer summary did not contain

Self-fetching the Codex hooks documentation surfaced three constraints that are architecturally load-bearing:

**C-02a — plugin hooks are not trusted on install.** Installing a plugin does not automatically trust its hooks. Codex skips plugin-bundled hooks until the user reviews and trusts the exact definition, and records trust against the hook definition's **hash** — so **new or changed** hook definitions/hashes are marked for review and skipped until trusted. A plugin update that leaves the hook definition/hash unchanged is not stated by the source to require re-trust. Our install UX must account for a state where the plugin is installed and its hooks are silently inert.

**C-02b — hooks can be switched off entirely.** `[features] hooks = false` disables them, and `allow_managed_hooks_only = true` skips plugin hooks while keeping managed ones. A `--dangerously-bypass-hook-trust` flag also exists.

**C-02d — operational limits.** `SessionEnd` runs synchronously with a 1s default and 3s maximum; model-visible hook output is capped near 2500 tokens and spills to disk beyond that.

Together, C-02a and C-02b independently confirm the OpenAI warning: **hooks are a guardrail, not an enforcement boundary.** Entitlement and security authority must stay in the shared core, which is already the design — but now for a verified reason rather than a cautious one.

## Provider policy matrix

Generated from the registry. **Routing may select only `VERIFIED_ALLOWED`.**

| Provider | Credential mode | Execution context | Technical | Policy status | Routable | Evidence |
|---|---|---|---|---|---|---|
| OPENAI | `CHATGPT_SUBSCRIPTION` | `HOST_NATIVE_CODEX` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-11 |
| OPENAI | `USER_API` | `PROGRAMMATIC_WORKER` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-11 |
| OPENAI | `ENTERPRISE_ACCESS_TOKEN` | `TRUSTED_NONINTERACTIVE` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-11 |
| OPENAI | `CHATGPT_CONSUMER_SUBSCRIPTION` | `THIRD_PARTY_PAID_EXTERNAL_WORKER` | SUPPORTED | **`POLICY_NEEDS_REVIEW`** | **NO** | C-11,C-11a |
| ANTHROPIC | `FREE_PRO_MAX_SUBSCRIPTION` | `THIRD_PARTY_EXTERNAL_WORKER` | SUPPORTED | **`VERIFIED_DISALLOWED`** | **NO** | C-10 |
| ANTHROPIC | `SUBSCRIPTION_OAUTH` | `HOST_NATIVE_CLAUDE_CODE` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-10 |
| ANTHROPIC | `USER_API` | `THIRD_PARTY_PROGRAMMATIC_PRODUCT` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-10 |
| ANTHROPIC | `SUPPORTED_CLOUD_PROVIDER` | `THIRD_PARTY_PROGRAMMATIC_PRODUCT` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES | C-10 |

Validated: `POLICY_NEEDS_REVIEW_ROUTABLE_PATHS = 0`, `VERIFIED_DISALLOWED_ROUTABLE_PATHS = 0`.

## A-14 — retired as a current assumption

**[HISTORICAL]** The 2026-08-13 observation recorded in `C-03` is **currently false** per `C-02` (self-fetched). It is retired as a current assumption and preserved only as a dated record in the registry. No current architecture statement in this package asserts that Codex lacks hooks; `STALE_A14_CURRENT_ASSERTIONS = 0` is enforced mechanically.

## What remains honestly open

| Claim | Status | Why |
|---|---|---|
| `C-12` | `POLICY_NEEDS_REVIEW` | No current source explicitly permits a paid third-party commercial orchestrator to drive a consumer subscription as an external worker. Not promoted on inference. |
| `C-13-ANTHROPIC` | `UNVERIFIED` | No reviewer evidence established first-party Anthropic paid-plugin checkout. Absence of evidence is not permanent absence. |
| `C-06` | `VERIFIED_HISTORICAL` | `codex exec` flag details were not re-verified this pass. The adapter probes at install rather than trusting a snapshot. |

Per §11, unrelated claims were **not** force-promoted to `VERIFIED_CURRENT` merely because other claims were verified.
