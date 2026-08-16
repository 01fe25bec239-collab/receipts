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

# RUNTIME_ADAPTER_INTERFACE

One contract for every agent runtime that can execute a RUNTIME-A3 or A4.

## Interface

```
interface RuntimeAdapter {
  runtime_id: string
  health(): HealthReport
  authenticateStatus(): AuthStatus            // CONNECTED | AUTH_REQUIRED | EXPIRED | UNKNOWN
  capabilities(): RuntimeCapabilities         // probed, not assumed
  models(): ModelRef[] | UNKNOWN
  start(task: Capsule, workspace: WorkspaceHandle, policy: ExecutionPolicy): AttemptHandle
  streamEvents(h: AttemptHandle): AsyncIterable<AttemptEvent>
  collectResult(h: AttemptHandle): AttemptResult
  cancel(h: AttemptHandle, reason): void
  classifyFailure(e: RawFailure): FailureClass
  resume?(attempt_id): AttemptHandle          // OPTIONAL
}
```

`resume` is optional by design (§46): A3 and A4 are fresh sessions per attempt (I-2), so no core behaviour depends on it.

## `classifyFailure` is load-bearing

Every runtime reports failure differently. Normalisation into one vocabulary is what lets the scheduler, router, and safety machinery behave identically across providers:

```
FailureClass = RATE_LIMITED | SESSION_EXHAUSTED | AUTH_REQUIRED | PROVIDER_DOWN
             | TIMEOUT | SANDBOX_DENIED | SAFETY_CHECK_PENDING | POLICY_BLOCKED
             | RUNTIME_CRASH | INVALID_OUTPUT | USER_CANCELLED | UNKNOWN
```

`UNKNOWN` is a legitimate result. Guessing `RATE_LIMITED` for an unclassifiable error would trigger the wrong recovery — and `SAFETY_CHECK_PENDING` versus `POLICY_BLOCKED` in particular must never be conflated (see `SAFETY_INTERRUPTION_PROTOCOL.md`).

## Execution policy

```
ExecutionPolicy { sandbox_mode, network_access, filesystem_scope,
                  timeout, turn_budget, structured_output_schema?, read_only }
```

`read_only: true` for A4 reviewers. Adapters that cannot enforce read-only report reduced isolation rather than claiming it.

## Adapter maturity tiers (§47)

**Integration maturity, not quality ranking.**

| Tier | Runtimes | Basis |
|---|---|---|
| **TIER-1** | Claude Code CLI, Codex CLI | Headless, structured output, session resume, OS sandbox all `VERIFIED_HISTORICAL` / registry `C-06`, `C-07` — probed at install |
| **TIER-2** | Gemini CLI | Headless + JSON verified (A-25); resume less clear |
| **VERIFY-BEFORE-PRODUCTION** | Kimi Code CLI, Grok Build, others | Runtime exists (A-27, A-29); headless/JSON/sandbox specifics `UNVERIFIED` (A-28, A-30) |
| **API/HARNESS PATH** | DeepSeek and other API-only providers | No confirmed official agent runtime (A-31) |

A tier says how confident we are that we can *drive* the runtime — nothing about how good the model is.

## Harness path

For API-only providers, an internal harness supplies the loop, tool use, and filesystem access. It is explicitly **second-class for implementation work**: an internally built harness is less battle-tested than a vendor's own agent runtime, and the routing decision records that the model was reached this way.

## Adding an adapter

Implement the interface; probe capabilities; supply `classifyFailure` mappings; pass the adapter conformance suite (start/stream/collect/cancel, failure classification, read-only enforcement, sandbox behaviour, structured output). Only then does the runtime become routable.

No core file changes when an adapter is added. If one does, the abstraction is wrong.
