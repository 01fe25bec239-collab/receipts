# CONTRACT-POLICY-002 — Admission + AdmissionDecision

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M2  
**Depends on:** CORE-001/002/003, POLICY-001, OVERRIDE-001, REVIEW-002

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Freeze the pure admission decision. Stored admissions are audit artifacts, never the source of truth.

## Normative schema

```text
AdmissionDecision = ADMIT | BLOCK | ADMIT_WITH_OVERRIDE

UnmetRequirement {
  claimType: string
  reason: string
  causedByPaths: [string]
}

Admission {
  admissionId: string
  taskId: string
  fingerprint: HexSha256
  decision: AdmissionDecision
  unmet: [UnmetRequirement]
  policyDigest: HexSha256
  evaluatedAt: RFC3339
  downgrades: [string]
  overrideId: string?
}
```

## Normative function

```text
admit(task, normalizedPolicy, derivedClaims, currentFingerprint,
      currentReviewEvidence, integrityFacts, activeOverrideOrNull, now)
  -> Admission
```

No filesystem, Git, database, network, model, or subprocess I/O occurs inside `admit()`.

## Decision rules

1. Required claim satisfies only with PROVED or active WAIVED.
2. Required review must be current, COMPLETED, parseOk, satisfy vendor rule, and have no finding at/above blocking severity.
3. Integrity block requirements must be satisfied/waived.
4. No unmet -> ADMIT.
5. Unmet + active task/fingerprint override -> ADMIT_WITH_OVERRIDE, retaining full unmet list.
6. Otherwise BLOCK.
7. `now` affects only review max_age.
8. Staleness reasons include specific changed paths when available.

## Required / optional

All fields required. `overrideId` null except ADMIT_WITH_OVERRIDE. causedByPaths may be empty for non-staleness reasons.

## Invariants

Recomputed value wins over stored. Every gate consultation appends the evaluated Admission audit record. ADMITTED_WITH_OVERRIDE never aliases ADMIT. Downgrades explicit.

## Validation

ADMIT => unmet empty + override null. BLOCK => unmet nonempty + override null. ADMIT_WITH_OVERRIDE => unmet nonempty + active override id.

## Failure behavior

Missing/invalid policy facts produce POLICY/INTERNAL error, not a fake admission decision. Enforcement adapter then applies its fail-closed/fail-open rule.

## Compatibility/versioning

Decision enum or pure-input meaning changes are breaking. Added optional diagnostics minor.

## Security constraints

Agent cannot submit desired decision. Provider/vendor compared as data only.

## Example

```json
{"admissionId":"a201","taskId":"AUTH-42","fingerprint":"<64hex>","decision":"BLOCK","unmet":[{"claimType":"TESTED","reason":"STALE","causedByPaths":["src/auth/store.ts"]}],"policyDigest":"<64hex>","evaluatedAt":"2026-08-09T04:31:00Z","downgrades":[],"overrideId":null}
```

## Negative examples

Trusting last stored ADMIT; override with empty unmet; provider timeout as success; stale TESTED omitted.

## Field semantics

`admissionId` identifies one recorded evaluation artifact; `taskId` and `fingerprint` identify what was evaluated; `decision` is the recomputed result; `unmet` explains unsatisfied obligations; `policyDigest` binds the decision to policy input; `evaluatedAt` timestamps evaluation; `downgrades` records explicit provider-assurance degradation; `overrideId` is present only for `ADMIT_WITH_OVERRIDE`.

## Required fields

`admissionId`, `taskId`, `fingerprint`, `decision`, `unmet`, `policyDigest`, `evaluatedAt`, and `downgrades` are required.

## Optional fields

`overrideId` is optional and MUST be present for `ADMIT_WITH_OVERRIDE` and absent for ordinary `ADMIT`/`BLOCK`.

## Required tests

Complete decision matrix; stale causes; max-age; vendor modes; severity boundaries; override; malformed review; purity/no-I/O.
