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

# PROVIDER_MODEL_REGISTRY

Durable catalog of providers, models, and agent runtimes, with freshness and provenance. Persisted in the state store; never held only in memory.

## Registries

**Provider Registry** — connection identity, auth mechanisms and state, programmatic-use policy, quota visibility class, health.
**Model Registry** — models per provider, tiers, costs (nullable), context limits, capability assessments, lifecycle state.
**Runtime Registry** — agent runtimes, probed versions, headless/structured-output/session-resume support, filesystem/shell/git capability, sandbox modes.

Schemas in `MODEL_CAPABILITY_SCHEMA.md`.

## Compatibility matrix

```
                 claude-code-cli  codex-cli  gemini-cli  api-harness
Anthropic models        ✔             –          –            ✔
OpenAI models           –             ✔          –            ✔
Google models           –             –          ✔            ✔
DeepSeek models         –             –          –            ✔    ← API-only (A-31)
```

The router selects a **(provider, model, runtime) triple**, never a model alone. A model with no agent-capable runtime cannot receive an implementation task no matter how strong it is — this is why the three entities are kept separate.

## Runtime capability probing

At install and on TTL expiry, each adapter probes its CLI for version and supported flags rather than trusting documentation snapshots. A-05 and A-15 record that exact flag spellings are `UNVERIFIED`; probing is how the architecture absorbs that uncertainty instead of hard-coding a guess.

Probe results are stored with a timestamp. A failed probe marks the runtime `install_state: UNAVAILABLE` rather than assuming the previous capabilities still hold.

## Registry seeding

Ships with **structure**, not with a model catalog. On first run it enumerates whatever the user has connected. Shipping a baked-in catalog would be stale on release day and would quietly become the de facto ranking.

## Integrity

Only the Model Intelligence Service writes these registries. No worker agent, no host adapter, and no LLM output path may mutate them — otherwise a compromised or confused worker could promote a model into frontier eligibility. Every write is an event with source and confidence.
