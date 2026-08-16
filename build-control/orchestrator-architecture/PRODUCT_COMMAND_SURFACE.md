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

# PRODUCT_COMMAND_SURFACE

## Host-neutral semantic operations

| Operation | Purpose | Tier |
|---|---|---|
| `START_GOAL` | Submit a goal or spec; compile an ExecutionGraph | FREE |
| `RESUME_GOAL` | Resume an existing graph | FREE |
| `SHOW_GRAPH` | Render the graph tree | FREE |
| `SHOW_STATUS` | Current execution status | FREE |
| `SHOW_CAPABILITIES` | Free/Pro catalog with live status | FREE |
| `SHOW_PROVIDERS` | Connections: auth, policy, availability — separately | FREE |
| `PRODUCT_LOGIN` | Activate Pro entitlement | FREE (to invoke) |
| `PRODUCT_LOGOUT` | Clear local entitlement | FREE |
| `PRODUCT_ENTITLEMENT_STATUS` | Current entitlement state | FREE |
| `UPGRADE_INFO` | What Pro adds and how to activate | FREE |

Every one is available on both hosts. **Identical slash syntax is not required; behavioural parity is** (§59).

## Graph rendering — zero model tokens

```
Goal auth-refresh                                    graph v17
├── ✓ inspect                     ACCEPTED
├── ✓ backend                     ACCEPTED   sha 4f2a1c…
├── ▶ frontend                    RUNNING
├── ○ deterministic-check         PLANNED
└── 🔒 cross-provider-review      LOCKED_REQUIRES_PRO
```

Rendered deterministically from `GraphSnapshot` by template. Spending a model call to draw a tree would be waste, and would make the display of authoritative state dependent on a model — which `SECURITY_TRUST_MODEL.md` forbids (the renderer may present state, never alter it).

Glyphs are cosmetic. The **information** must be identical on both hosts.

## Provider status shows four axes separately

```
OUR PRODUCT      Plan: PRO_ACTIVE

ANTHROPIC        Auth: CONNECTED   Mode: CLAUDE_SUBSCRIPTION
                 Policy (external worker): NEEDS_REVIEW
                 Availability: AVAILABLE

OPENAI CODEX     Auth: CONNECTED   Mode: CHATGPT_MANAGED
                 Policy (external worker): NEEDS_REVIEW
                 Availability: AVAILABLE

Eligible runtimes for Pro dispatch: 0
Cross-provider review: UNAVAILABLE_NO_ELIGIBLE_RUNTIME
```

This is the display that prevents the §71 failure: a paying customer sees that the blocker is **policy eligibility**, not their subscription to us. Telling them to upgrade here would be both wrong and insulting.

## Command syntax is Q-01, reopened

Host extension mechanisms have changed and were not re-verified this pass. Namespaced commands are preferred; a reserved built-in is never hijacked. `START_GOAL` remains the normalized semantic operation regardless of surface syntax.
