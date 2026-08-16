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

# PERSONAL_LOCAL_MODE

**The MVP mode.** One developer, one machine, their own provider accounts.

## Shape

```
Developer's machine
├── orchestrator (local process / plugin)
├── state store (local)
├── the user's own authenticated CLIs
│     claude · codex · gemini · …
└── the user's repositories
```

The orchestrator **invokes the user's already-authenticated local runtimes**. It does not hold their subscription credentials, because it does not need to: the CLI authenticates itself.

## Why this is the MVP mode

1. It matches how the product will actually be used first — an individual developer with existing subscriptions.
2. It sidesteps the hardest policy question (multi-user subscription routing) rather than pretending it away.
3. It is the lowest-risk credential posture available: delegation beats storage.

## Rules

**Permitted:** invoking the user's authenticated CLI on their own machine for their own work; storing API keys in the OS keychain when the user supplies them; reading auth *status*; using enterprise credentials the user configured.

**Prohibited:** scraping browser cookies; extracting tokens from provider config; storing subscription secrets in the repository; transmitting credentials anywhere; routing another person's work through this user's subscription; presenting subscription capacity as a product resource.

## Single-user boundary

This mode is for **one user's own work**. It never becomes a shared service by adding users — that is a different mode with a different credential model, and the configuration makes the transition explicit rather than incremental.

## Quota reality

Per A-23, subscription paths usually expose no reliable programmatic quota. The availability manager therefore operates largely on **observed** signals — failures, `retry_after` where present, local usage views — and records `UNKNOWN` rather than estimating. Being honestly uncertain about remaining quota is better than being confidently wrong about it.

## Failure surface

If a CLI is not authenticated, the orchestrator surfaces `AUTH_REQUIRED` and directs the user to the official login command. It never attempts to authenticate on the user's behalf.
