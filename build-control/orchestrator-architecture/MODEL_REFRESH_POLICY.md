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

# MODEL_REFRESH_POLICY

## Principle

Do **not** run a broad research pass before every dispatch. Refresh is event-driven and cached.

```
dispatch request
      ▼
capability registry
      ▼
   fresh?
  ┌──┴──┐
 YES   NO
  │     ▼
  │  refresh (scoped)
  │     │
  └──▶ route
```

A per-task internet lookup would make routing slower and more expensive than the work being routed.

## Freshness

| Data class | Default TTL | Rationale |
|---|---|---|
| Provider model list | 24 h | New models appear without warning |
| Official capability metadata | 7 d | Changes slowly |
| Pricing | 7 d | Changes occasionally, matters for estimation |
| Runtime version/flags | on install + 7 d | CLI flags drift (A-05, A-15) |
| Availability/quota | seconds–minutes | Observed continuously, not polled |
| Independent benchmarks | 30 d | Slow-moving; low routing weight |
| Local calibration | never expires | Accumulates; sample-size weighted |

TTLs are configuration.

## Triggers (§48)

TTL expiry · provider model-list change · new-model discovery · missing capability for a requested filter · explicit user request · **major critical dispatch** (frontier floor + security-critical) · executor failover · provider connect/disconnect · repeated unexplained failures · vendor deprecation notice.

"Major critical dispatch" is deliberate: before routing security-critical work, the registry is confirmed current even if the TTL has not expired. The cost is one refresh; the alternative is routing that work on stale evidence.

## Scoped, not global

A refresh touches only the affected slice — one provider's model list, one runtime's flags. A missing capability for one filter does not trigger a full re-verification of every provider.

## Failure

Refresh failure never blocks by itself. The stale entry is used with `staleness` recorded on the routing decision, and repeated failure degrades provider availability state. Under `ASK_ON_UNCERTAINTY`, stale data for a critical dispatch prompts the user rather than proceeding quietly.

## Verification discipline

Refreshed facts carry source and access date. A capability claim that cannot be attributed is stored as `UNVERIFIED` and cannot alone justify a frontier dispatch. This mirrors the rule the build process applies to itself: a fact without a citation is not a fact.
