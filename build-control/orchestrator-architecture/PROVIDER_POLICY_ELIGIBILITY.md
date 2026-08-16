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

# PROVIDER_POLICY_ELIGIBILITY

## The distinction

> **Technical authentication does not imply contractual permission.**

An authenticated CLI on the user's machine may still be ineligible for a requested execution context. `ProviderPolicyEligibility` makes that a first-class, machine-checkable fact instead of an assumption buried in a routing decision.

## Owner

**BUILD-A2-MODEL-ROUTING** owns the contract and decides status. **BUILD-A2-RUNTIME-ADAPTERS** gathers evidence (technical auth state, credential mode, runtime identity) and supplies it through a frozen contract. **The router consumes it.**

This ordering avoids a concrete build cycle: MODEL-ROUTING already hard-depends on RUNTIME-ADAPTERS, and the reverse direction is contract-only (§92).

## Record

`provider_id` · `runtime_id` · `credential_mode` · `technical_status` · `policy_status` · `allowed_execution_contexts[]` · `verified_at` · `evidence_source` · `evidence_label` · `terms_version_or_digest` · `reason` · `reverification_deadline`

All identifiers are **extensible strings**. No vendor is baked into a core schema enum (§84, §129); vendor-specific policy evidence lives in registry data.

## Policy status

`VERIFIED_ALLOWED` · `VERIFIED_DISALLOWED` · `NEEDS_REVIEW` · `UNKNOWN`

**Routing may select only `VERIFIED_ALLOWED`.** `NEEDS_REVIEW` and `UNKNOWN` are not usable by default. A frozen, explicit user or policy mechanism may permit `UNKNOWN` for a specific context, but that is opt-in and recorded — never a default.

Defaulting `UNKNOWN` to allowed would convert every gap in our own research into a compliance risk for the customer.

## Credential modes

`HOST_NATIVE` · `PERSONAL_LOCAL_CLI` · `USER_API` · `ENTERPRISE_GATEWAY` · `CLOUD_PROVIDER` — extensible.

## Execution contexts

`HOST_NATIVE_INTERACTIVE` · `HOST_NATIVE_PLUGIN` · `LOCAL_EXTERNAL_WORKER` · `NONINTERACTIVE_CLI` · `CI_AUTOMATION` · `TEAM_SERVICE` · `CLOUD_SERVICE`

One provider rule does not apply equally to every context. A credential permitted for interactive host-native use is not thereby permitted as an external worker driven by a paid third-party product — that is precisely the distinction that must not be hand-waved.

## Current status — closed, not pending

Statuses below are generated from `evidence/SOURCE_CLAIM_REGISTRY.json`. The source-closure gap that existed at V1.3 was closed at V1.3.1.

| Provider | Credential mode | Execution context | Technical | Policy status | Routable |
|---|---|---|---|---|---|
| OPENAI | `CHATGPT_SUBSCRIPTION` | `HOST_NATIVE_CODEX` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |
| OPENAI | `USER_API` | `PROGRAMMATIC_WORKER` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |
| OPENAI | `ENTERPRISE_ACCESS_TOKEN` | `TRUSTED_NONINTERACTIVE` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |
| OPENAI | `CHATGPT_CONSUMER_SUBSCRIPTION` | `THIRD_PARTY_PAID_EXTERNAL_WORKER` | SUPPORTED | **`POLICY_NEEDS_REVIEW`** | **NO** |
| ANTHROPIC | `FREE_PRO_MAX_SUBSCRIPTION` | `THIRD_PARTY_EXTERNAL_WORKER` | SUPPORTED | **`VERIFIED_DISALLOWED`** | **NO** |
| ANTHROPIC | `SUBSCRIPTION_OAUTH` | `HOST_NATIVE_CLAUDE_CODE` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |
| ANTHROPIC | `USER_API` | `THIRD_PARTY_PROGRAMMATIC_PRODUCT` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |
| ANTHROPIC | `SUPPORTED_CLOUD_PROVIDER` | `THIRD_PARTY_PROGRAMMATIC_PRODUCT` | SUPPORTED | **`VERIFIED_ALLOWED`** | YES |

### What each row means

**`ANTHROPIC / FREE_PRO_MAX_SUBSCRIPTION / THIRD_PARTY_EXTERNAL_WORKER` → `VERIFIED_DISALLOWED`.** Anthropic states it does not permit third-party developers to offer Claude.ai login or route requests through Free, Pro, or Max credentials on behalf of users (`C-10`). This path is **structurally unroutable**. It is not a gap awaiting research; it is a decided no.

**`ANTHROPIC / SUBSCRIPTION_OAUTH / HOST_NATIVE_CLAUDE_CODE` → `VERIFIED_ALLOWED`.** The same source describes OAuth as intended for ordinary use by subscription purchasers *in Claude Code*. A user running our plugin inside their own Claude Code session is doing exactly that. This is the distinction §5 requires: our plugin may be hosted in the user's Claude Code environment while their subscription credentials remain ineligible for our external-worker routing.

**`ANTHROPIC / USER_API` and `SUPPORTED_CLOUD_PROVIDER` → `VERIFIED_ALLOWED`.** The documented programmatic direction for products.

**`OPENAI / CHATGPT_CONSUMER_SUBSCRIPTION / THIRD_PARTY_PAID_EXTERNAL_WORKER` → `POLICY_NEEDS_REVIEW`.** Codex supports ChatGPT sign-in and `codex login` (`C-11`), and consumer terms restrict programmatic extraction generally (`C-11a`) while Codex documentation explicitly supports programmatic workflows through defined authentication. Those two facts do **not** resolve into permission for a paid third-party commercial orchestrator. Not promoted on inference; **not routable by default**.

**`OPENAI / USER_API` and `ENTERPRISE_ACCESS_TOKEN` → `VERIFIED_ALLOWED`.** API-key auth is explicitly recommended for programmatic Codex CLI workflows, and enterprise access tokens for trusted noninteractive workflows (`C-11`).

### Neither oversimplification is adopted

Not *"all automation prohibited"* — Codex documents programmatic workflows through defined mechanisms. Not *"a ChatGPT subscription allows arbitrary commercial automation"* — the consumer terms restrict programmatic extraction. The execution-context gate exists precisely to hold both facts at once instead of collapsing them.

### Billing behaviour is a third, separate thing

`C-07a` records that an announced Anthropic Agent SDK billing change was paused, so subscription usage limits may still be drawn on. **That is billing behaviour and does not supersede the credential restriction.** Three axes stay distinct: `TECHNICAL_CAPABILITY`, `BILLING_BEHAVIOR`, `POLICY_ELIGIBILITY`. A path can be technically supported and billed to a subscription and still be contractually ineligible.


## No provider shopping

Policy eligibility is **not** a search space. If a provider is `VERIFIED_DISALLOWED` for a context, the answer is to use a different **credential mode** or a different **context** — never to find a provider whose terms are vaguer. This is the commercial analogue of the safety-bypass prohibition, and it is enforced the same way: the gate returns `PROVIDER_POLICY_DISALLOWED`, and that is a terminal answer for that path.

## Reverification

Every record carries `verified_at` and `reverification_deadline`. Provider terms are volatile; a stale `VERIFIED_ALLOWED` past its deadline degrades to `NEEDS_REVIEW` automatically rather than continuing to authorise dispatch.
