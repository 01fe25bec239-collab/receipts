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

# PRODUCT_ENTITLEMENT_ARCHITECTURE

## Four concepts that must never collapse into one boolean

| Concept | Question | Owner |
|---|---|---|
| **Product entitlement** | May this installation use OUR Pro capabilities? | BUILD-A2-ORCHESTRATION (admission) / BUILD-A2-STATE-CONTEXT (token mechanics) |
| **Provider authentication** | Is the provider CLI technically authenticated? | BUILD-A2-RUNTIME-ADAPTERS |
| **Provider policy eligibility** | Is this connection *permitted* for this execution context? | BUILD-A2-MODEL-ROUTING |
| **Provider availability** | Is it up and within quota right now? | BUILD-A2-MODEL-ROUTING |

Collapsing these produces the failure §71 names explicitly: telling a paying customer to upgrade when the real problem is that their provider connection is not eligible.

Product entitlement has **nothing** to do with Claude Pro, Claude Max, ChatGPT Plus, ChatGPT Pro, Gemini Advanced, or any provider balance.

## Ownership split (§91)

| Concern | Owner | Reason |
|---|---|---|
| Normative feature admission | **ORCHESTRATION** | Admission is core authority; it is where dispatch is permitted or refused |
| Entitlement token persistence, verification, cache | **STATE-CONTEXT** | It is a durable local artifact with the same integrity requirements as other state |
| Login / activation / status presentation | **HOST-INTEGRATION** | Presentation only — it displays, it does not decide |

Host Integration does **not** own entitlement truth merely because it renders the login screen.

## Default is FREE, with no account

A public clone or fresh marketplace install resolves to `FREE` immediately, with no account, no network call, and no prompt. Pro is activated through our own product account flow.

The system must **never** infer our tier from a GitHub username, a Claude plan, a ChatGPT plan, provider API keys, or unrelated environment variables. Those are other companies' billing relationships and say nothing about ours.

## Activation provenance (V1.3.1)

V1.3 could not distinguish a fresh install from a previously-Pro install whose cache was lost — both observe only *"no token exists"*. That ambiguity is resolved with an explicit, serialized activation state.

```
NEVER_ACTIVATED  ──PRODUCT_LOGIN──▶  ACTIVATED_KNOWN  ──PRODUCT_LOGOUT──▶  LOGGED_OUT
                                            ▲                                    │
                                            └──────────PRODUCT_LOGIN─────────────┘
```

| Activation state | Entitlement present | Service | Resolves to |
|---|---|---|---|
| `NEVER_ACTIVATED` | none | unreachable | **`FREE`** |
| `LOGGED_OUT` | none | any | **`FREE`** |
| `ACTIVATED_KNOWN` | missing / corrupt | unreachable | **`ENTITLEMENT_UNKNOWN`** |
| any | valid | any | `PRO_ACTIVE` / `PRO_GRACE` |
| any | expired | any | `PRO_EXPIRED` |

**A previously-Pro user is never inferred as `FREE` because a cache was lost.** That would silently downgrade a paying customer on a disk error.

`LOGGED_OUT` is explicit and deliberate: the user asked to release Pro on this install, so resolving to `FREE` is correct rather than a downgrade.

**FREE is the locally available baseline capability set.** It requires **no** signed licence from our server — no fake "Free entitlement" is issued or required. A signed `ProductEntitlement` is required only for **paid** capability authority.

`ActivationState` is serialized (schema: `schemas/ActivationState.schema.json`) precisely because the distinction must survive the loss of the entitlement cache. It also carries `last_observed_server_time` for clock-rollback detection.

## Entitlement states

`FREE` · `PRO_ACTIVE` · `PRO_GRACE` · `PRO_EXPIRED` · `ENTITLEMENT_UNKNOWN`

`ENTITLEMENT_UNKNOWN` is distinct from `FREE`. Treating an unreachable licensing service as "user is Free" would silently destroy in-progress Pro state — §36 forbids exactly that.

## Signed entitlement

Carries only: `subject_id`, `tier_id`, `capabilities[]`, `issued_at`, `expires_at`, `offline_grace_until`, `entitlement_version`, `key_id`, `signature`, optional opaque `device_binding`.

It carries **no** provider credentials, source code, prompts, graph content, or repository paths.

`tier_id` is an extensible string, not an enum — a future tier must not require a schema change (§83).

Private signing keys never ship in the public plugin or repository. Local verification uses public key material only.

## Storage

User/install-scoped, never project-scoped, never committed to Git. OS keychain for anything secret-like; a signed non-secret cached entitlement file is acceptable because the signature is the integrity mechanism, not secrecy.

Claude and Codex on one machine resolve the **same** entitlement — it belongs to the installation, not the host.

## Licensing service privacy

Our licensing service may receive: account/licence ID, product version, minimal device/licence metadata, entitlement request metadata.

It must **never** receive: repository contents, prompts, graph nodes, diffs, provider API keys, provider OAuth tokens, or host conversations.

This is easy to honour precisely because we do not sell inference. Any telemetry is a separate, disclosed design — never a hidden licensing dependency.
