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

# MODEL_CAPABILITY_LIFECYCLE

## States

```
DISCOVERED ──▶ UNASSESSED ──▶ CAPABILITY_VERIFIED ──▶ CALIBRATING ──▶ ROUTABLE
                                                                        │
     ┌──────────────────────────────────────────────────────────────────┤
     ▼              ▼                ▼                 ▼
DEPRECATED     DISABLED        UNAVAILABLE       USER_BLOCKED
```

| State | Meaning | Routable? |
|---|---|---|
| `DISCOVERED` | Seen in a provider listing | No |
| `UNASSESSED` | Exists; no capability evidence | No |
| `CAPABILITY_VERIFIED` | Official capability + runtime compatibility confirmed | Low-risk work only |
| `CALIBRATING` | Being used under observation on bounded tasks | Yes, with limits |
| `ROUTABLE` | Sufficient evidence for normal routing at its tier | Yes |
| `DEPRECATED` | Vendor announced retirement | Existing tasks only |
| `DISABLED` | Operationally disabled (repeated failures) | No |
| `UNAVAILABLE` | Temporarily unreachable/rate-limited | No, retry later |
| `USER_BLOCKED` | User policy forbids | No |

## The rule that matters (§33)

> **A new model must not automatically become the production default because of vendor marketing.**

A launch announcement moves a model to `DISCOVERED`. Benchmark claims in that announcement are recorded as `UNVERIFIED` and never promote a model on their own. This is the direct answer to "a new frontier model was just released": the system can *find* and *eventually use* it without an architecture rewrite, but it does not hand it security-critical work on the strength of a press release.

## Promotion

**→ `CAPABILITY_VERIFIED`:** official documentation confirms the model exists and a compatible agent runtime can drive it (headless, structured output, filesystem/shell/git where required).

**→ `CALIBRATING`:** enters a bounded trial — lower-risk tasks first, always audited, results recorded. Never a security-critical or architecture task as its first job.

**→ `ROUTABLE`:** sufficient local observations at acceptable first-pass accept rate, or `OFFICIAL_VERIFIED` + `INDEPENDENT_VERIFIED` evidence with user consent under `ASK_ON_UNCERTAINTY`. Sample-size thresholds are configuration; the requirement for *some* evidence is not.

## Demotion

Repeated runtime failures, sustained rejection rate, vendor deprecation, auth loss, or user block. Demotion is automatic and reversible; it is recorded with the observations that caused it, so a temporarily bad week does not become a permanent verdict without evidence.

## Interaction with the frontier floor

A `CALIBRATING` model may serve frontier-floor work only when: no `ROUTABLE` frontier candidate is available, the user has consented under the routing mode, and the result is audited at the normal standard. Under `ASK_ON_UNCERTAINTY` — the default — this asks. Silently promoting an uncalibrated model into security-critical work is exactly what this lifecycle exists to prevent.
