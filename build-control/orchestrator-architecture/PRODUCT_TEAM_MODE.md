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

# PRODUCT_TEAM_MODE

**Deferred beyond MVP** (`DEFERRED_CAPABILITIES.md`), designed for now so the architecture does not have to be reshaped later.

## Shape

Multi-user or hosted deployment using **programmatic mechanisms only**: API keys, enterprise credentials, provider gateways, cloud integrations, service accounts.

## The rule (§51)

> **Do not assume a consumer Pro/Plus/Max subscription may legally be routed through a third-party multi-user application.**

Per A-20 this is explicitly disallowed by at least one major provider. The architecture therefore makes it **structurally impossible** rather than merely discouraged: in `PRODUCT_TEAM_MODE`, subscription-backed connection types are not registrable. A policy enforced only by documentation eventually gets violated by someone under deadline.

## Credential model

| Concern | Approach |
|---|---|
| Storage | Secret manager, never the repository |
| Scope | Per-organisation or per-project, not per-end-user-subscription |
| Rotation | Supported; rotation invalidates cached auth state |
| Attribution | Every dispatch records which credential scope was used |
| Isolation | One tenant's work never uses another's credentials |

## Why it is deferred

It requires: verified current terms for every provider (§51); a hosted execution model with real isolation between tenants; billing and quota attribution; and an auth model with no shortcuts. Building it before the core is proven would mean solving compliance and multi-tenancy for an orchestrator that has not yet orchestrated anything.

## Compatibility requirement

The credential broker interface is identical in both modes; only the **registrable connection types** and the **storage backend** differ. Nothing above the broker knows which mode it is in — which is what makes the later addition a configuration change rather than a redesign.

## Pre-implementation obligation

Before implementing, re-verify each provider's current terms for programmatic and multi-user access, cite them with access dates, and record any provider whose terms forbid the intended use. A provider that cannot be used compliantly is simply not offered in this mode.
