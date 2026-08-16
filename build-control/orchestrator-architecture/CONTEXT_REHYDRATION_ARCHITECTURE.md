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

# CONTEXT_REHYDRATION_ARCHITECTURE

## Principle (§59, I-3)

```
CHAT CONTEXT                  = disposable cache
REPOSITORY + DURABLE STATE    = source of truth
```

The goal is **not** to tell agents "remember the instructions". It is to make remembering unnecessary.

## What rehydration is

**Actually rereading authoritative artifacts** at their current version — the spec, architecture, contracts, ownership, decision log, DAG slice, branch and SHA state, active and accepted tasks, findings, open issues, next actions.

## What rehydration is not

**Replaying a summary written by a previous executor** (§62). A summary is lossy compression performed by a model that may itself have misread something; inheriting it inherits the error, and the error becomes invisible because it now looks like established context. This distinction is the entire point of the subsystem.

## Mandatory triggers (§62)

RUNTIME-A1 init · RUNTIME-A2 init · model replacement · provider replacement · **host switch** · context compaction · architecture change · contract change · new implementation wave · configurable completed-task threshold · serious A4 rejection (architecture/security) · security escalation · before A2 integration · before A1 integration · **before declaring goal COMPLETE**.

The last is deliberate: a completion decision made on stale context is the most expensive mistake the system can make.

## Flow

```
trigger
  → load Context Manifest for the role
  → compare recorded digests against current artifacts
  → identify changed / missing / added sources
  → read the current content of each required source
  → rebuild the working context
  → record epoch reconciliation
  → resume
```

Digest comparison keeps rehydration cheap: unchanged sources are recognised as unchanged, and only the delta needs attention. It also detects the dangerous case — a source that changed while the role was unbound.

## Interaction with context compaction

Host compaction is a normalized event (`CONTEXT_COMPACTED`). **Its confidence depends on the active `HostCapabilityReport`, not on the vendor name:**

| Active path | Confidence |
|---|---|
| Claude Code, native hooks active | `OBSERVED` (`PreCompact`/`PostCompact`) |
| Codex, native hooks active | `OBSERVED` (`PreCompact`/`PostCompact`, registry `C-02`) |
| Any host, fallback mode without that lifecycle signal | `INFERRED` |

Tying confidence to the discovered capability rather than to the host identity is what keeps this correct when a vendor ships or withdraws a hook. Either way it triggers rehydration, because after compaction the executor's working context is a host-generated summary — exactly the artifact rehydration refuses to trust.

## Cost control

Rehydration is scoped by the role's manifest, not the whole repository: a RUNTIME-A2 rereads its workstream slice. Digest-based skipping avoids re-reading unchanged large files. Frequency is bounded by the trigger list, not by a timer.

## Failure

If an authoritative source is missing or unreadable, rehydration **fails loudly** and the role does not resume. Continuing on partial context would produce decisions whose basis nobody can reconstruct — indistinguishable from a role that quietly forgot something.

## Verification

Scenario S17 in `SCENARIO_VALIDATION.md`, plus parity row P-11.
