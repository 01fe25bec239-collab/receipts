# CONTRACT-POLICY-001 — VerificationPolicy

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M2  
**Depends on:** CORE-003, CONFIG-002

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Define declarative verification obligations, profile resolution, review requirements, and integrity policy without embedding provider/model identity into architecture.

## Normative schema

```text
VerificationPolicy {
  version: 1
  defaultProfile: string
  profiles: map<string, PolicyProfile>
  pathOverrides: [PathOverride]
}

PolicyProfile {
  require: [{ claim: ClaimType }]
  review: ReviewPolicy
  testIntegrity: TestIntegrityPolicy?
}

ReviewPolicy {
  mode: optional | required
  profile: string?
  distinctVendor: off | preferred | required
  blockingSeverity: INFO | LOW | MEDIUM | HIGH | CRITICAL
  includeTestDiff: when_tests_changed | always
  maxAge: Duration?
}

TestIntegrityPolicy {
  onTestDeletion: expose | block
}

PathOverride {
  match: RepoRelativeGlob
  profile: string
}
```

## Defaults

- default profile in scaffold: STANDARD
- review.distinct_vendor: preferred when review is required, otherwise off
- review.blocking_severity: HIGH
- review.include_test_diff: when_tests_changed
- test_integrity.on_test_deletion: expose
- max_age: unset

All defaults are applied before `policyDigest`.

## Profile resolution

The strictest matching path override wins, never first-match order. Strictness is compared by obligations:
- required claim superset is stricter;
- required review > optional review;
- distinct_vendor required > preferred > off;
- lower severity threshold is stricter;
- test deletion block > expose;
- a finite max_age is stricter than none; shorter is stricter than longer.

If two matching profiles are incomparable, policy validation fails and the human must resolve ambiguity.

## Task freeze

At OPEN, task `requiredClaims` freezes `require[].claim` plus implicit `REVIEWED` when review.mode is required. Later policy edits cannot silently relax the open task; explicit recorded policy-amend is required.

## Invariants

Deterministic/review evidence do not substitute. Distinct vendor is policy, not invariant. Provider/model names are configuration. Review max_age affects review evidence only; deterministic evidence has no TTL in MVP.

## Validation

All profile refs resolve. Claim names valid. Durations `^[1-9][0-9]*[smhd]$`. Unknown fields invalid. Path globs repo-relative/no `..`.

## Failure behavior

Invalid/ambiguous policy fails closed for admission. No fabricated BLOCK/ADMIT result is produced from a parse error; caller receives POLICY error.

## Compatibility/versioning

New policy dimension or changed strictness semantics is breaking. New profile using existing fields is config-only.

## Security constraints

Policy file is agent-write-protected and digest-tracked. A changed policy cannot silently weaken already-open tasks.

## Example

See CONTRACT-CONFIG-002 for the architecture-preserving YAML.

## Negative examples

First matching override wins; provider model hardcoded in policy; same-vendor review globally forbidden; `TESTED` satisfied by review evidence.

## Field semantics

`defaultProfile` selects the fallback profile. `profiles` defines named obligations. `require` selects claim types; `review` defines whether and how review evidence is required; `testIntegrity` controls test-deletion handling; `pathOverrides` selects stricter profiles by repository-relative glob. Provider/model identity is deliberately absent.

## Required fields

Top-level normalized policy requires `version`, `defaultProfile`, `profiles`, and `pathOverrides`. Each profile requires `require` and `review`; each requirement requires `claim`; each path override requires `match` and `profile`.

## Optional fields

`PolicyProfile.testIntegrity`, `ReviewPolicy.profile`, and `ReviewPolicy.maxAge` are optional. Fields with documented defaults may be omitted in YAML input but are populated before digest/evaluation.

## Required tests

Built-in profiles; defaults; strictest selection; incomparable rejection; implicit REVIEWED; duration parsing; OPEN freeze; unknown fields.
