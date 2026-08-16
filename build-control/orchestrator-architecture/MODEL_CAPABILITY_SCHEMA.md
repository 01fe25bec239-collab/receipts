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

# MODEL_CAPABILITY_SCHEMA

## Three separate entities (§45)

```
Provider  ──has many──▶  Model  ──accessible through──▶  AgentRuntime
```

A raw LLM API is **not** automatically an autonomous coding agent. DeepSeek exposes strong models with no confirmed official agent runtime (A-31); the model is routable only through a harness path, and the schema must be able to say that.

## Schema

```
Provider {
  provider_id, display_name
  auth_mechanisms[]        OAUTH_SUBSCRIPTION | API_KEY | ENTERPRISE_GATEWAY | CLI_LOGIN
  auth_state               CONNECTED | AUTH_REQUIRED | EXPIRED | NOT_CONFIGURED
  programmatic_policy      PERSONAL_ONLY | PROGRAMMATIC_ALLOWED | UNKNOWN
  quota_visibility         HEADERS | LOCAL_APPROX | NONE | UNKNOWN
  health, last_refreshed_at
}

Model {
  model_id, provider_id, display_name
  quality_tier             FRONTIER | BALANCED | ECONOMY | UNASSESSED
  cost_tier                HIGH | MEDIUM | LOW | UNKNOWN
  input_cost, output_cost  nullable — UNKNOWN if unpublished
  context_limit            nullable
  capabilities             CapabilityMap
  lifecycle_state          see MODEL_CAPABILITY_LIFECYCLE
  discovered_at, last_verified_at, deprecation_notice?
}

AgentRuntime {
  runtime_id               e.g. claude-code-cli, codex-cli, gemini-cli, api-harness
  provider_id
  supported_models[]
  headless                 boolean
  structured_output        boolean
  session_resume           boolean
  filesystem_write, shell, git
  sandbox_modes[]
  install_state, version_probed_at
}

CapabilityAssessment {
  subject                  model_id | runtime_id | (model_id, runtime_id)
  capability               coding | review | reasoning | documentation
                           | structured_output | long_context | tool_use | security_review
  value                    scalar | boolean | UNKNOWN
  confidence               OFFICIAL_VERIFIED | INDEPENDENT_VERIFIED
                           | LOCAL_EMPIRICAL | USER_DECLARED | UNVERIFIED
  source_ref               URL or local observation set
  observed_at, sample_size?
}
```

## Confidence classes (§34)

| Class | Source | Weight |
|---|---|---|
| `OFFICIAL_VERIFIED` | Vendor documentation, cited with access date | Highest for *existence* and *runtime* facts |
| `INDEPENDENT_VERIFIED` | Credible third-party evaluation | Moderate for *quality* claims |
| `LOCAL_EMPIRICAL` | This orchestrator's own accepted/rejected history | Highest for *this* project once sample size is adequate |
| `USER_DECLARED` | Explicit user statement or pin | Authoritative where the user has authority |
| `UNVERIFIED` | Anything else | Never sufficient alone for a frontier dispatch |

Local empirical evidence eventually outranks vendor claims for the router's purposes, because it measures the thing that actually matters: accepted results in this repository.

## UNKNOWN is a real value

Pricing, context limits, and quota state are frequently unpublished. `UNKNOWN` is stored and propagated. Fabricating a plausible number would corrupt cost estimation with data no one can trace.

## Compatibility matrix

Routing needs `(model, runtime)` pairs, not models. The same model may be frontier-capable through an agent runtime and merely API-callable elsewhere. Only pairs with `filesystem_write + shell + git` are eligible for implementation tasks.
