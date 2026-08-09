# CONTRACT-OVERRIDE-001 — OverrideRecord / Waiver Semantics

**Version:** 1.0.0  
**Owner:** A2-INTEGRITY-SECURITY  
**Consumers:** A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M2 semantics; M5 public UX  
**Depends on:** CORE-001/002/003, POLICY-002, LEDGER-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Provide break-glass while preserving the distinction between proven and human-accepted risk.

## Normative schema

```text
OverrideRecord {
  overrideId: string
  taskId: string
  actor: string
  reason: string
  fingerprint: HexSha256
  unmetAtOverride: [UnmetRequirement]
  grantedAt: RFC3339
}

WaiverRecord {
  waiverId: string
  taskId: string
  claimId: string
  actor: string
  reason: string
  fingerprint: HexSha256
  grantedAt: RFC3339
}
```

## Semantics

Override yields ADMIT_WITH_OVERRIDE despite unmet requirements. Waiver yields WAIVED for one claim. Neither creates evidence or PROVED.

## Invariants

Interactive human confirmation required. Agent context rejected. Nonempty reason. Fingerprint-scoped. No standing override/waiver. Full unmet list captured at grant. Never rendered as verified. Frequency measurable.

## Validation

Recompute fingerprint/admission before prompt and immediately before append. If state changes during confirmation, abort and require confirmation again.

## Failure behavior

Agent -> INTEGRITY/no event. Empty reason -> INPUT. No interactive TTY/human channel -> INTERACTION_REQUIRED. Cancel -> CANCELLED/no event. State race -> no grant.

## Compatibility/versioning

Any noninteractive/agent-grant path is architecture-breaking. Optional audit metadata minor.

## Security constraints

No `--yes`, `--force`, stdin/environment confirmation in MVP. Claude hook denies agent Bash override invocation, and CLI independently verifies human context.

## Example

```json
{"overrideId":"o44","taskId":"AUTH-44","actor":"local-human","reason":"CI runner offline; verified manually on staging","fingerprint":"<64hex>","unmetAtOverride":[{"claimType":"REVIEWED","reason":"provider unavailable","causedByPaths":[]}],"grantedAt":"2026-08-09T05:00:00Z"}
```

## Negative examples

Blank reason; survives edit; agent invokes; UI says VERIFIED; waiver without claimId.

## Field semantics

An `OverrideRecord` accepts the complete current unmet set for one task/fingerprint without converting those requirements to proof. A `WaiverRecord` excuses one named claim for one task/fingerprint. `actor` is the human identity available to the local broker; `reason` is mandatory human-supplied rationale.

## Required fields

All fields in `OverrideRecord` and `WaiverRecord` are required. `unmetAtOverride` may be an empty array only if the caller raced with a newly satisfied admission, in which case the broker MUST abort rather than grant a meaningless override.

## Optional fields

None in MVP. Extra audit metadata requires a compatible contract extension and MUST NOT change authority semantics.

## Required tests

Agent rejection; no bypass flag; cancellation; fingerprint race; expiry on edit; complete unmet capture; export/render distinction.
