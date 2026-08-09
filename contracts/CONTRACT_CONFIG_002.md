# CONTRACT-CONFIG-002 — `.receipts/policy.yaml`

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-CORE, A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-INTEGRITY-SECURITY  
**Status:** FROZEN  
**First milestone:** M2  
**Depends on:** POLICY-001/002

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Freeze the architecture's policy semantics.

## Normative YAML

```yaml
version: 1
default_profile: STANDARD

profiles:
  LIGHT:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
    review:
      mode: optional

  STANDARD:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
      - claim: LINT_CLEAN
    review:
      mode: required
      profile: general
      distinct_vendor: preferred
      blocking_severity: HIGH
      include_test_diff: when_tests_changed

  HIGH_ASSURANCE:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
      - claim: LINT_CLEAN
      - claim: TYPECHECKED
    review:
      mode: required
      profile: security
      distinct_vendor: required
      blocking_severity: MEDIUM
      include_test_diff: always
      max_age: 7d
    test_integrity:
      on_test_deletion: block

path_overrides:
  - match: "src/auth/**"
    profile: HIGH_ASSURANCE
  - match: "docs/**"
    profile: LIGHT
```

## Fields/enums/defaults

- version: required `1`.
- default_profile: required existing profile.
- profiles: required map.
- require[].claim: upper-snake ClaimType.
- review.mode: optional|required.
- review.profile: string; required when mode required.
- distinct_vendor: off|preferred|required; default preferred for required, off for optional.
- blocking_severity: INFO|LOW|MEDIUM|HIGH|CRITICAL; default HIGH.
- include_test_diff: when_tests_changed|always; default when_tests_changed.
- max_age: optional `^[1-9][0-9]*[smhd]$`, review-only.
- test_integrity.on_test_deletion: expose|block; default expose.
- path_overrides: default []; match repo-relative glob; profile existing.

Unknown fields prohibited.

## Resolution/digest

Strictest matching profile per POLICY-001; not first match. Incomparable matches invalid. `policyDigest = SHA-256(JCS(normalized policy with defaults))`.

## Task behavior

Open task obligations remain frozen despite later config edit absent explicit policy-amend event.

## YAML parsing/security

Safe YAML 1.2; duplicate keys/tags/anchors/aliases/merge keys rejected. Agent edits protected. Models/vendors do not belong here as hardcoded architecture choices.

## Failure behavior

Invalid policy -> POLICY error and fail-closed admission.

## Negative examples

Unknown mode; missing profile ref; first-match dependency; model name in admission rule.

## Normative schema

`policy.yaml` MUST validate against `schemas/policy.schema.json`; after YAML-name normalization/defaulting it materializes POLICY-001 and is evaluated only through POLICY-002.

## Field semantics

`version` selects schema major; `default_profile` identifies fallback profile; `profiles` defines obligations; `path_overrides` maps repo-relative globs to named profiles. Review/provider selection semantics are policy attributes, while concrete provider/model identity remains CONFIG-003 data.

## Required fields

Top-level `version`, `default_profile`, and `profiles` are required. Each profile requires `require` and `review`; each `require` item requires `claim`; each path override requires `match` and `profile`.

## Optional fields

Top-level `path_overrides` may be omitted and defaults to `[]`. Profile `test_integrity`, review `profile`, `distinct_vendor`, `blocking_severity`, `include_test_diff`, and `max_age` follow the explicit defaults/conditions in POLICY-001/this contract. Unknown fields are prohibited.

## Invariants

Open-task `requiredClaims` cannot be silently relaxed by later config edits. Review evidence cannot satisfy deterministic claims. Matching path overrides resolve to the strictest profile; incomparable matches are invalid. Policy digests are computed after default normalization.

## Validation rules

Validate schema, all profile references, enums, claim names, durations, repo-relative globs, strictness comparability for potentially co-matching overrides, and unknown-field rejection before policy can drive admission.

## Required tests

Canonical example; all defaults/enums; strictest/incomparable; duration; unknown fields; digest; task freeze.
