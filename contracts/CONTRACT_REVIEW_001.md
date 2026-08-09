# CONTRACT-REVIEW-001 — ReviewRequest

**Version:** 1.0.0  
**Owner:** A2-REVIEW  
**Consumers:** A2-REVIEW providers, A2-CLAUDE-INTEGRATION, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M4  
**Depends on:** CORE-001/002/003, EVIDENCE-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Bind independent review to exact task, diff, and code state.

## Normative schema

```text
ReviewRequest {
  reviewId: string
  taskId: string
  claimId: string
  fingerprint: CodeStateFingerprint
  profile: string
  diffRef: string
  includeTestDiff: boolean
  contextRefs: [RepoRelativePath]
  schema: object
}
```

## Semantics

diffRef points to exact broker-captured unified diff. contextRefs are read-only paths. schema is broker-selected structured output schema. includeTestDiff is policy-resolved.

## Required / optional

All fields required; contextRefs may be empty.

## Invariants

Exact fingerprint and diff; no "review current working tree later". Agent cannot select schema or privileges. Request is not evidence until provider result captured.

## Validation

Task/claim/fingerprint consistency; diff digest matches CodeEvidence; paths repo-relative/no `..`; schema valid JSON Schema.

## Failure behavior

Fingerprint changes before dispatch -> state changed/no provider call. Bad diff/schema -> INPUT/INTEGRITY; REVIEWED stays UNPROVEN.

## Compatibility/versioning

Removing exact diff/fingerprint binding is architecture-breaking; optional context metadata minor.

## Security constraints

Reviewer gets broker prompt/data and read-only context only.

## Example

```json
{"reviewId":"v07","taskId":"AUTH-42","claimId":"c-review","fingerprint":{"repoId":"...","headSha":"...","dirty":true,"workingTreeDigest":"...","fingerprint":"..."},"profile":"general","diffRef":"diffs/v07.diff","includeTestDiff":true,"contextRefs":["src/auth/rotate.ts"],"schema":{"$ref":"schemas/finding.schema.json"}}
```

## Negative examples

No fingerprint; mutable diff regenerated after start; agent-supplied schema; context path escape.

## Field semantics

`reviewId` is stable for one broker-dispatched review; `taskId`/`claimId` identify the review claim; `fingerprint` binds the request to exact code state; `profile` selects review instructions; `diffRef` identifies the exact immutable diff artifact; `includeTestDiff` is policy-derived; `contextRefs` is the read-only context allowlist; `schema` is the broker-controlled output schema.

## Required fields

All fields in `ReviewRequest` are required. `contextRefs` may be an empty array but MUST be explicit.

## Optional fields

None in MVP. Optionality belongs inside the structured finding schema, not in request identity/state binding.

## Required tests

Fingerprint race; exact diff; schema rejection; path escape; forced test diff.
