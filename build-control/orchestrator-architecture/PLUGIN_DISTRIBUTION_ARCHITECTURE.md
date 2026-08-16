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

# PLUGIN_DISTRIBUTION_ARCHITECTURE

Plugin install, removal and update are re-probe triggers under `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`.

## One product, two host packages, one core

```
PRODUCT RELEASE
   ├── Claude Code plugin package   (public)
   ├── Codex plugin package         (public)
   ├── Shared local core            (public, Free policy)
   └── Pro module                   (proprietary, post-activation)
```

Anthropic and OpenAI do not share a marketplace. Two packages, coordinated releases, one core.

## Version compatibility

`graph_schema_version` · `core_version` · `claude_plugin_version` · `codex_plugin_version` · `pro_module_version` · `entitlement_version` · `provider_registry_version`

Semantic compatibility, not exact equality. A plugin declares the core range it supports; the core declares the graph schema versions it can read. **A Pro module incompatible with the core fails fast with an explicit message** — silently degrading a paid feature is worse than refusing to start it.

## Update and rollback

Free core, host plugins and the Pro module update independently within their compatibility ranges. Graph schema migrations are **forward-only and never destructive**; a graph is never auto-downgraded. Rolling back a plugin version is supported; rolling back the core below a graph's schema version is refused with an explanation rather than attempted.

## Public repository behaviour

A clone gives `PRODUCT_PLAN = FREE` with no account and no network call. The user can install the plugins, run Free graph engineering, inspect the Pro catalog, run tests, and contribute to OSS portions.

The repository must contain **no** private signing key, production entitlement secret, founder provider credential, Pro bypass secret, or customer credential.

## Entitlement is not a Git flag

`PRO=true` in a tracked file, a `.plan` file, or an env var is **not authority**. Local config may *request* a capability; authority comes only from a verifiable signed entitlement. This matters because the alternative — a repo-visible flag — would make the paid tier trivially self-granted and the licence meaningless.

## Commercial distribution — provider-specific (V1.3.1)

C-13 was one undifferentiated claim in V1.3. It is now split, because the two vendors differ.

**OpenAI — `EXTERNAL_CHECKOUT_SUPPORTED = VERIFIED_CURRENT`** (`C-13-OPENAI`). The App Developer Terms, updated 2026-07-09, explicitly cover plugins and establish that the developer operates independently, that OpenAI does not guarantee listing or discovery, and that a developer may direct users to an external developer-controlled website for payment through External Checkout, subject to policies and law.

This supports exactly the flow the architecture already assumes:

```
Free plugin → user chooses upgrade → our website / payment flow
            → our signed entitlement → our local Pro module
```

**What this does not mean.** OpenAI does **not** process our Pro subscription, does **not** pay us, and does **not** pre-approve every form of premium-feature gating. External Checkout permits directing the user to our own flow — nothing more.

**Anthropic — `ANTHROPIC_NATIVE_PAID_PLUGIN_CHECKOUT = UNVERIFIED / NOT_ESTABLISHED`** (`C-13-ANTHROPIC`). Third-party and community plugin distribution, a community marketplace, and private repository marketplaces are all established (`C-13-ANTHROPIC-DIST`). No reviewer evidence established a first-party paid-plugin checkout mechanism. **Absence of evidence is not evidence of permanent absence**, and this is not asserted as a settled no.

**Our licensing service remains host-independent either way.** Neither vendor is our payment processor, and the entitlement flow does not depend on marketplace commerce on either platform.
