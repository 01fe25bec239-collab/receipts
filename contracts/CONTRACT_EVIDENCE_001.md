# CONTRACT-EVIDENCE-001 — Evidence Families + Evidence

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M1  
**Depends on:** CORE-001, CORE-003, LEDGER-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Make CODE, DETERMINISTIC, and REVIEW evidence non-substitutable.

## Normative schema

```text
EvidenceFamily = CODE | DETERMINISTIC | REVIEW

Evidence {
  evidenceId: string
  taskId: string
  claimId: string
  family: EvidenceFamily
  fingerprint: CodeStateFingerprint
  createdAt: RFC3339
  producedBy: "broker"
  payloadRef: string
  prevEventHash: HexSha256
  eventHash: HexSha256
}

CodeEvidence {
  codeEvidenceId: string
  taskId: string
  fingerprint: HexSha256
  baselineSha: GitObjectId
  headSha: GitObjectId
  changedPaths: [string]
  pathBlobIds: [{path:string, blobId:string?}]
  insertions: integer >= 0
  deletions: integer >= 0
  testFilesAdded: integer >= 0
  testFilesModified: integer >= 0
  testFilesDeleted: integer >= 0
  recipeOrPolicyFilesChanged: boolean
  testCountDelta: integer?
  diffRef: string
}
```

## Authority matrix

CODE -> IMPLEMENTED only. DETERMINISTIC/ExecutionReceipt -> deterministic recipe claims. REVIEW/ReviewResult -> ReviewClaims only.

## Required / optional

All Evidence fields. CodeEvidence all fields except `testCountDelta`; `blobId` may be absent for deletions.

## Invariants

Exact fingerprint; broker producer only; historical rows retained; validity derived, not stored forever; recipe-backed evidence additionally matches recipeDigest/schema; review assertions are not deterministic proof.

## Validation/failure

Family/payload mismatch is invalid. IDs resolve within same repo/task. Malformed evidence never admitted.

## Compatibility/versioning

New family or cross-family authority is breaking. Optional diagnostics minor.

## Security constraints

Workers have no evidence-write surface. Raw artifacts broker-owned outside repo.

## Example

```json
{"evidenceId":"e1","taskId":"AUTH-42","claimId":"c1","family":"DETERMINISTIC","fingerprint":{"repoId":"...","headSha":"...","dirty":true,"workingTreeDigest":"...","fingerprint":"..."},"createdAt":"2026-08-09T04:15:00Z","producedBy":"broker","payloadRef":"r-118","prevEventHash":"...","eventHash":"..."}
```

## Negative examples

REVIEW labeled DETERMINISTIC; agent producer; missing fingerprint; deleting stale evidence.

## Field semantics

`family` is an authority boundary, not a display label. `fingerprint` identifies the exact code state. `producedBy` is fixed to `broker`. `payloadRef` names the family-specific broker artifact. Event hashes connect the evidence-recording event to the append-only ledger; they do not upgrade the truth value of the payload.

## Required fields

Every `Evidence` field is required. In `CodeEvidence`, all listed fields are required except `testCountDelta`; each `pathBlobIds` item requires `path` and may omit `blobId` only for deletion.

## Optional fields

`CodeEvidence.testCountDelta` is optional when runner output cannot supply comparable counts. `pathBlobIds[].blobId` is optional only for deleted paths.

## Required tests

Authority matrix; validity/stale/revert; recipe invalidation; broker-only; CodeEvidence integrity counters.
