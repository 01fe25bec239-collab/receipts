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

# RATE_LIMIT_AND_AVAILABILITY

## Normalized states (§53)

| State | Meaning | Router | Recovery |
|---|---|---|---|
| `AVAILABLE` | Normal | Eligible | — |
| `DEGRADED` | Elevated errors/latency | Eligible, penalised | Observe |
| `RATE_LIMITED` | Limit hit | Ineligible | `retry_after` or backoff |
| `SESSION_EXHAUSTED` | Session/window budget spent | Ineligible | Window reset |
| `AUTH_REQUIRED` | Credentials missing/expired | Ineligible | User action |
| `PROVIDER_DOWN` | Outage | Ineligible | Health probe |
| `SAFETY_CHECK_PENDING` | Provider indicates a pending safety review | Special handling | `SAFETY_INTERRUPTION_PROTOCOL.md` |
| `POLICY_BLOCKED` | Provider refused on policy grounds | Special handling | `SAFETY_INTERRUPTION_PROTOCOL.md` |
| `UNKNOWN` | Cannot determine | Treated as `DEGRADED` | Probe |

The last two are deliberately **not** failure states: conflating a safety refusal with a rate limit would trigger provider failover, which is exactly the prohibited behaviour under I-12.

## Signals

| Source | Quality |
|---|---|
| API rate-limit headers + `retry_after` (A-22) | Best — precise and forward-looking |
| Exit codes / stderr classification via `classifyFailure` | Good |
| Local usage views on subscription paths (A-23) | Approximate |
| Latency and error-rate trends | Weak but continuous |

`retry_after` is stored whenever observable and drives scheduling directly.

## No unfounded quota assumptions (§53)

> Do not assume all models from one provider share a quota unless verified. Do not assume separate quotas unless verified.

Default is `quota_scope: UNKNOWN`, treated conservatively: a rate limit observed on one model marks that model `RATE_LIMITED` and the provider `DEGRADED`, without asserting anything about sibling models. Observed behaviour may later establish a scope; assumption never does.

## Backoff

Exponential with jitter, capped, respecting `retry_after` when present. Per `(provider, model)`. Backoff state is persisted so a restart does not stampede a limited provider.

## Effect on dispatch

Unavailability affects **future** decisions immediately: candidates are filtered before scoring, running attempts are left alone, and queued tasks are re-routed. When all eligible candidates for a required floor are unavailable, `QUALITY_COST_POLICY.md` applies (WAIT/BLOCK/ASK/HUMAN_REQUIRED) — never a downgrade.

## Failover, not loss (§54)

A rate limit never destroys work. The capsule, workspace, branch, SHA, diff, checks, findings, and dependencies are already durable, so failover is a routing decision rather than a recovery operation. That is the point of persisting them.

## Health probing

Cheap, periodic, and scoped. A probe is never a full task. Repeated probe failure escalates `DEGRADED → PROVIDER_DOWN`; success de-escalates. Probes are rate-limited themselves so health checking cannot become the thing that exhausts the quota.

## Availability does not overload policy or licence (V1.3)

Four independent axes, never collapsed:

```
technical:  AVAILABLE | DEGRADED | RATE_LIMITED | AUTH_REQUIRED | PROVIDER_DOWN | UNKNOWN
policy:     VERIFIED_ALLOWED | VERIFIED_DISALLOWED | NEEDS_REVIEW | UNKNOWN
product:    ALLOW | LOCKED_REQUIRES_PRO | ENTITLEMENT_UNKNOWN
safety:     CLEAR | SAFETY_CHECK_PENDING | POLICY_BLOCKED
```

A provider that is healthy, authenticated, and policy-ineligible reports `AVAILABLE` on the technical axis and `VERIFIED_DISALLOWED` on the policy axis. Reporting it as unavailable would hide the real reason and send the user to fix the wrong thing.
