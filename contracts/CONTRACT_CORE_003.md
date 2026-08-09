# CONTRACT-CORE-003 — Claim + ClaimStatus

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-CLAUDE-INTEGRATION, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M2  
**Depends on:** CORE-001, CORE-002, EVIDENCE-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Represent assertions without allowing the asserting agent to prove them.

## Normative schema

```text
ClaimKind = DETERMINISTIC | REVIEW
ClaimStatus = UNPROVEN | PROVED | REJECTED | STALE | WAIVED
ClaimType = /^[A-Z][A-Z0-9_]*$/

AgentIdentity = "human" | { agentId: string, agentType: string? }

Claim {
  claimId: string
  taskId: string
  type: ClaimType
  assertedBy: AgentIdentity
  assertedAt: RFC3339
  status: ClaimStatus
  evidenceRefs: [string]
  provedAt: RFC3339?
  provedAgainst: CodeStateFingerprint?
}
```

## MVP claim types

`ClaimKind` is **derived from the claim-type definition**, not stored as a mutable Claim field. MVP mappings are: IMPLEMENTED (CODE-backed deterministic special case), TESTED (DETERMINISTIC / recipe `test`), LINT_CLEAN (DETERMINISTIC / recipe `lint`), REVIEWED (REVIEW). Other deterministic claim names may be configuration data using the same machinery.

## Status semantics

UNPROVEN = no admissible evidence. PROVED = current positive evidence. REJECTED = current negative evidence. STALE = previously positive evidence no longer valid for current fingerprint/recipe compatibility. WAIVED = active human waiver for one task/claim/fingerprint.

## Invariants

Agent assertion never proves. ReviewEvidence never proves deterministic claims. ExecutionReceipt never proves REVIEWED. WAIVED is not proof. Stale evidence remains historical and can become valid again on exact revert.

## Validation

provedAt/provedAgainst present iff PROVED. Evidence-family/type-definition mismatches are INTEGRITY errors. MVP type-to-kind mapping is fixed by this contract.

## Failure behavior

Malformed/mismatched evidence cannot update to PROVED.

## Compatibility/versioning

New claim type on existing kind/recipe machinery is configuration-compatible; new kind or status semantics is breaking.

## Security constraints

Worker cannot directly write status/evidence projections.

## Example

```json
{"claimId":"c1","taskId":"AUTH-42","type":"TESTED","assertedBy":{"agentId":"a17","agentType":"implementer"},"assertedAt":"2026-08-09T04:10:00Z","status":"UNPROVEN","evidenceRefs":[]}
```

## Negative examples

Agent-provided PROVED; TESTED from Codex review; REVIEWED from test exit 0; permanent WAIVED.

## Field semantics

`claimId` is the stable claim identity; `taskId` binds the claim to one task; `type` selects claim semantics/evidence family; `assertedBy` records provenance but never grants proof authority; `assertedAt` is assertion time; `status` is derived from admissible evidence/current fingerprint/waiver state; `evidenceRefs` are historical references; `provedAt` and `provedAgainst` exist only when status has been positively established for a state.

## Required fields

`claimId`, `taskId`, `type`, `assertedBy`, `assertedAt`, `status`, and `evidenceRefs` are required.

## Optional fields

`provedAt` and `provedAgainst` are optional and MUST be absent/null when no positive proof has ever been established. They are historical metadata and MUST NOT override current derived status.

## Required tests

Family matrix; all statuses; rejected vs unproven; stale/revert; waiver expiry; configured new deterministic type.
