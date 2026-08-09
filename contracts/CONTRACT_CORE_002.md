# CONTRACT-CORE-002 — Task + TaskState

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M2  
**Depends on:** CORE-001, POLICY-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Separate task lifecycle from claim status and freeze each task's verification obligations at OPEN.

## Normative schema

```text
TaskState = DRAFT | OPEN | SUBMITTED | ADMITTED | BLOCKED | ADMITTED_WITH_OVERRIDE | CLOSED

Task {
  taskId: string
  title: string
  repoId: RepositoryId
  baselineSha: GitObjectId
  declaredPaths: [RepoRelativeGlob]
  policyProfile: string
  requiredClaims: [ClaimType]
  state: TaskState
  externalRef: string?
  createdAt: RFC3339
  updatedAt: RFC3339
}
```

## Field semantics

`baselineSha`, `declaredPaths`, `policyProfile`, and normalized `requiredClaims` freeze on OPEN. Required claims are `profile.require` plus implicit `REVIEWED` when review.mode is required. `externalRef` is the Claude task-list ID when available, not Receipts authority.

## State machine

```text
DRAFT -> OPEN -> SUBMITTED -> ADMITTED | BLOCKED | ADMITTED_WITH_OVERRIDE
BLOCKED -> SUBMITTED
ADMITTED -> SUBMITTED              (fingerprint changed)
ADMITTED_WITH_OVERRIDE -> SUBMITTED (fingerprint changed)
ADMITTED | ADMITTED_WITH_OVERRIDE -> CLOSED
```

## Required / optional

All Task fields except `externalRef`.

## Invariants

Task state and ClaimStatus are separate. Admission is recomputed authority; task state is a projection. Mid-task policy edit cannot silently relax required claims. Override state never collapses into ADMITTED.

## Validation

taskId/title non-empty; declared paths repo-relative, no absolute/`..`; baseline resolves in repo; profile exists; duplicate required claims normalized/rejected.

## Failure behavior

Invalid transition/amendment produces INPUT/POLICY error with no ledger mutation.

## Compatibility/versioning

New TaskState/transition meaning is breaking. Optional metadata is minor.

## Security constraints

Declared paths are scope metadata, not sandboxing. Agent absence does not imply human.

## Example

```json
{"taskId":"AUTH-42","title":"Rotate refresh tokens","repoId":"<repo-id>","baselineSha":"abc...","declaredPaths":["src/auth/**"],"policyProfile":"STANDARD","requiredClaims":["IMPLEMENTED","TESTED","LINT_CLEAN","REVIEWED"],"state":"OPEN","externalRef":"task-001","createdAt":"2026-08-09T04:00:00Z","updatedAt":"2026-08-09T04:00:00Z"}
```

## Negative examples

Changing requiredClaims after OPEN without policy-amend; keeping ADMITTED after fingerprint change; `../../` path; externalRef used as primary task ID.

## Required fields

`taskId`, `title`, `repoId`, `baselineSha`, `declaredPaths`, `policyProfile`, `requiredClaims`, `state`, `createdAt`, and `updatedAt` are required.

## Optional fields

`externalRef` only. Its absence MUST NOT affect task identity, admission, or evidence validity.

## Required tests

Transition matrix; OPEN freeze; amendment; staleness withdrawal; override distinction; externalRef optional; path validation.
