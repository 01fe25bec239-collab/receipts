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

# SAFETY_INTERRUPTION_PROTOCOL

## Two distinct states (§73)

| State | Meaning |
|---|---|
| `SAFETY_CHECK_PENDING` | The provider indicates an additional safety review or check is still in progress. Not a refusal. |
| `POLICY_BLOCKED` | The provider declined on policy grounds. A refusal. |

Both differ from `RATE_LIMITED`, `RUNTIME_CRASH`, `REVIEW_REJECTED`, and `AUTH_REQUIRED`. Conflating any of them with a rate limit would trigger provider failover — which for a policy refusal is the prohibited behaviour (I-12).

## Detectability

Per A-32 (`ASSUMPTION`), no provider currently exposes a normalised machine-readable safety state. Detection is therefore **conservative and honest**: adapters classify from available signals via `classifyFailure`, and where the signal is ambiguous the result is `UNKNOWN`, not an optimistic guess. This is recorded as **Q-07**; the architecture must not manufacture certainty the runtime does not provide.

## `SAFETY_CHECK_PENDING` handling (§74)

```
1. preserve task state (capsule, workspace, SHA, partial evidence)
2. mark the attempt PENDING — not failed, not complete
3. DO NOT immediately duplicate expensive work across many providers
4. wait per configured timeout/policy
5. provider resumes  → continue normally
6. timeout expires   → classify terminal outcome and escalate
```

Step 3 matters: fanning the same task out to every provider the moment one hesitates burns budget, and — if the pause was a safety evaluation — it is indistinguishable from shopping for a permissive provider.

## `POLICY_BLOCKED` handling (§75)

For a **legitimate defensive security task**, the orchestrator MAY:

- preserve the blocked attempt and its full record;
- classify the task explicitly as defensive;
- narrow the capsule to reduce unnecessary exploit-generation detail;
- construct a bounded defensive audit capsule;
- rely on deterministic security tooling that needs no model;
- try another eligible provider **only when doing so is policy-compatible and is not intended to obtain a refused capability**.

## The prohibition (§76, I-12)

> **The orchestrator must NOT provider-shop for the purpose of circumventing a provider safety restriction.**

Explicitly not implemented:

```
Provider A refuses a dangerous request
        ↓
find a provider with weaker safeguards
        ↓
run the same request
```

**Provider nationality is never a safety-bypass heuristic**, and never an input to routing at all.

## The distinguishing test

Retry after a block is permitted only if **all** hold:

1. the task is genuinely defensive and classified as such;
2. the capsule was **narrowed**, not merely re-sent — an unchanged retry is shopping;
3. the retry is policy-compatible for the target provider;
4. the block, the narrowing, and the retry are all recorded;
5. the purpose is completing legitimate defensive work, not obtaining the refused capability.

Condition 2 is the operative one. Resending an identical request to a different provider has no purpose other than finding a weaker filter. The event log makes repeated patterns visible to the user and to audit.

## Escalation

```
POLICY_BLOCKED
   → narrowed defensive capsule → retry (conditions above)
       → still blocked → deterministic tooling only
           → insufficient → HUMAN_REQUIRED
```

Never a false `PASS` (§77). The record hands the human the exact SHA, all tool findings, prior reviewer findings, and every interruption event.

## Non-security tasks

A policy block on ordinary implementation work is treated as a provider capability limit: record it, route elsewhere on capability grounds, and — if it recurs — surface the pattern rather than routing around it indefinitely.
