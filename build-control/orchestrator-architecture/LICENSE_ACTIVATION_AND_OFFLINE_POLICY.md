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

# LICENSE_ACTIVATION_AND_OFFLINE_POLICY

## Activation flow

```
plugin / core
     │  PRODUCT_LOGIN
     ▼
browser or device authorisation
     │
     ▼
OUR entitlement service
     │  signed entitlement
     ▼
local verification (public key) → cache → PRO_ACTIVE
```

UX differs between Claude and Codex; the entitlement itself is host-neutral and shared. Activating in one host activates for both on that machine (§101).

**No restart or recompile** should be required. Admission reads current entitlement state on each decision, so a newly cached entitlement takes effect on the next admission call. If a host surface caches a capability list, it must refresh on entitlement change — a cached UI is a display concern, never an enforcement one.

## Offline grace

`offline_grace_until` is carried in the signed entitlement. Duration is **business policy, not architecture** — the architecture requires only that the field exists, is signed, and is honoured.

| State | Meaning |
|---|---|
| `PRO_ACTIVE` | Valid, within `expires_at` |
| `PRO_GRACE` | Past a refresh attempt but within `offline_grace_until` |
| `PRO_EXPIRED` | Past grace — no new Pro dispatch |
| `ENTITLEMENT_UNKNOWN` | Cannot determine; conservative, not FREE |

## Clock handling

Grace depends on local time, which a user controls. Mitigations: record the last observed server time; treat a local clock earlier than the last observed server time as tampering and fall back to `ENTITLEMENT_UNKNOWN`; never extend grace on clock evidence alone.

This deters casual extension. It does not defeat a determined attacker, and the threat model in `OPEN_CORE_COMMERCIAL_BOUNDARY.md` says so plainly.

## FREE never depends on the service

A network failure to our licensing service must never disable graph engineering. FREE resolves without any network call. This is a hard architectural rule because the alternative — a local developer tool that stops working when our server has an outage — would be indefensible.

## Downgrade preserves everything

Graph, node history, provenance, results, accepted SHAs, checkpoints and status all persist and remain readable. Only new Pro-only capability is blocked. Nothing is deleted.
