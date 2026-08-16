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

# PROVIDER_CREDENTIAL_ARCHITECTURE

## Principle

Use **only officially supported authentication**. Never scrape browser cookies, never extract hidden tokens, never store plaintext secrets in the repository.

## Broker interface (§52)

```
interface ProviderConnection {
  connect(): ConnectResult          // delegates to official flow
  disconnect(): void
  auth_status(): CONNECTED | AUTH_REQUIRED | EXPIRED | NOT_CONFIGURED | UNKNOWN
  health(): HealthReport
  models(): ModelRef[] | UNKNOWN
  runtime_capabilities(): RuntimeCapabilities
  usage(): UsageReport | UNKNOWN
  quota(): QuotaState | UNKNOWN
}
```

`usage()` and `quota()` returning `UNKNOWN` is normal and correct: per A-23, subscription-backed CLI paths expose only approximate local views. Inventing a number would make the availability manager confidently wrong.

## Supported mechanisms

Official CLI login (delegated) · official OAuth/device authorisation · provider-supported subscription-backed coding login · API key · enterprise gateway (e.g. Bedrock/Vertex-style routing) · service accounts.

## Storage

| Mechanism | Where the secret lives |
|---|---|
| Official CLI login | **In the provider's own tooling.** The orchestrator never reads it — it invokes the CLI, which authenticates itself |
| OAuth / device | OS keychain via the platform credential API |
| API key | OS keychain, or an env var the user controls |
| Enterprise gateway | Existing enterprise mechanism |

Delegation is the safest default: a credential the orchestrator never possesses is one it cannot leak. Nothing is written to the repository, and no secret appears in logs, events, capsules, handoffs, or error messages — enforced by a redaction layer on every persistence path.

## The critical distinction (§16, §51)

> **A web subscription is not automatically programmable agent access.**

Per registry claim `C-10` (reviewer-supplied current primary source), Anthropic states that subscription OAuth is for individual use of Claude Code and native applications, that developers including Agent SDK users should use API-key authentication, and that third parties may not offer claude.ai login or route requests through Free/Pro/Max credentials on behalf of their users. Per registry claim `C-11`, OpenAI does contemplate ChatGPT sign-in for the Codex CLI including `codex exec`, but the subscription endpoint is undocumented and API keys are recommended for production.

These differ by provider, and the architecture must not average them into one convenient assumption. Hence two explicitly separated modes:

- `PERSONAL_LOCAL_MODE` — one developer, local machine, own credentials;
- `PRODUCT_TEAM_MODE` — programmatic mechanisms only.

The mode is a first-class configuration value. No code path allows a team deployment to reach a personal subscription credential.

## Re-verification

Provider auth policy is volatile. Before implementing any credential path, current terms must be re-verified and cited with an access date (§51). A `VERIFIED_CURRENT` (per `SOURCE_CLAIM_REGISTRY.json`) label in `ASSUMPTION_REGISTER.md` is a snapshot, not a licence.
