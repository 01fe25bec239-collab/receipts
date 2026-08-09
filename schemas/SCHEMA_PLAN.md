# Receipts JSON Schema Plan

**Status:** FROZEN PLAN  
**Dialect:** JSON Schema 2020-12  
**Default object rule:** `additionalProperties: false` unless a contract explicitly marks a payload opaque (`parsed`, for example).

## Required JSON Schema files

| File | Contract | First milestone |
|---|---|---|
| `recipe.schema.json` | CONFIG-001 / RUNNER-001 | M1 |
| `policy.schema.json` | CONFIG-002 / POLICY-001 | M2 |
| `providers.schema.json` | CONFIG-003 / REVIEW-003 | M4 |
| `fingerprint.schema.json` | CORE-001 | M0 |
| `task.schema.json` | CORE-002 | M2 |
| `claim.schema.json` | CORE-003 | M2 |
| `evidence.schema.json` | EVIDENCE-001 | M1 |
| `receipt.schema.json` | RUNNER-002 | M1 |
| `admission.schema.json` | POLICY-002 | M2 |
| `override.schema.json` | OVERRIDE-001 | M5 |
| `review-request.schema.json` | REVIEW-001 | M4 |
| `review-result.schema.json` | REVIEW-002 | M4 |
| `finding.schema.json` | REVIEW-002; provider output schema | M4 |
| `ledger-event.schema.json` | LEDGER-001 | M0 |
| `hook-request.schema.json` | PLUGIN-001 | M3 |
| `hook-decision.schema.json` | PLUGIN-002 | M3 |
| `cli-envelope.schema.json` | CLI-001 | M0 |
| `export.schema.json` | EXPORT-001 | M5 |

## YAML validation pipeline

1. Parse safe YAML 1.2.
2. Reject duplicate keys, custom tags, anchors, aliases and merge keys.
3. Convert to plain JSON-compatible value.
4. Apply JSON Schema.
5. Apply semantic/cross-reference validation.
6. Apply contract defaults/normalization.
7. Compute JCS-based digest only after normalization where required.

## Shared scalar definitions

- RepositoryId: nonempty stable string; root-derived IDs are 64 lowercase hex, UUID fallback is permitted by CORE-001.
- HexSha256: `^[0-9a-f]{64}$`
- ClaimType: `^[A-Z][A-Z0-9_]*$`
- RecipeKey: `^[a-z][a-z0-9_-]*$`
- EnvName: `^[A-Za-z_][A-Za-z0-9_]*$`
- RFC3339 timestamps normalize to UTC `Z`.
- Repo-relative path/glob: nonempty; no NUL; no absolute root/drive; no `..` segment.

## Provider structured output

`finding.schema.json` describes only model-returned structured findings (or a wrapper containing findings). Provider ID/vendor/model/timing/status/rawRef are adapter-produced facts, not fields the model is asked to assert.

## Schema versioning

Config files carry `version:1`. Domain objects follow contract version; export/CLI envelopes expose version where wire compatibility matters. Persistent breaking changes require migration plan.

## Required schema tests

Canonical valid fixture; missing required; unknown field; enum/pattern boundaries; malicious path/shell fixtures; parse-normalize fixture; cross-contract authority tests.
