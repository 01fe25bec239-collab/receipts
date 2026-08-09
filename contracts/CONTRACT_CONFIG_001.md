# CONTRACT-CONFIG-001 — `.receipts/recipes.yaml`

**Version:** 1.0.0  
**Owner:** A2-RUNNER  
**Consumers:** A2-RUNNER, A2-CORE, A2-INTEGRITY-SECURITY  
**Status:** FROZEN  
**First milestone:** M1  
**Depends on:** RUNNER-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Implementation-ready deterministic verification configuration.

## Normative YAML

```yaml
version: 1

test_globs:
  - "test/**/*.test.ts"
  - "src/**/*.spec.ts"

recipes:
  test:
    claim: TESTED
    argv: ["pnpm", "test"]
    cwd: "."
    timeout_ms: 600000
    env_allowlist: ["CI"]

  lint:
    claim: LINT_CLEAN
    argv: ["pnpm", "lint"]
    cwd: "."
    timeout_ms: 300000
    env_allowlist: ["CI"]
```

## Fields

Top-level:
- `version`: required integer exactly 1.
- `test_globs`: required non-empty array of repo-relative glob strings.
- `recipes`: required map; MVP demo requires `test` and `lint`.

Recipe:
- `claim`: required upper-snake ClaimType; binds the recipe to one recipe-backed deterministic claim.
- `argv`: required non-empty string array.
- `cwd`: optional, default `"."`.
- `timeout_ms`: optional positive integer, default 600000.
- `env_allowlist`: optional EnvName array, default `[]`.

## Unknown/prohibited

Unknown fields prohibited. Explicitly prohibited: `command`, `shell`, `script`, `approved`, `approval`, env-value maps, arbitrary `extra_args`. Shell wrappers (`sh|bash -c`, PowerShell `-Command`) rejected.

## Path handling

Config uses `/` separators, repo-relative. Absolute and `..` escape prohibited. cwd realpath must remain within repo. test_globs never escape repo.

## Approval/digest

YAML cannot self-approve. Normalize with defaults; `recipeDigest = SHA-256(JCS({key,...normalizedEntry}))`; consult ledger RecipeApproval. Semantic change -> new unapproved digest. Formatting/comments do not affect digest.

## YAML parsing

Safe YAML 1.2. Duplicate keys, custom tags, anchors, aliases, merge keys rejected. Parsed value must be plain JSON-compatible.

## Failure behavior

Parse/schema/semantic error => CONFIG; no recipe executes.

## Compatibility/versioning

Schema version changes through contract update. New semantic field changes digest.

## Security constraints

Agent edits denied where possible, but approval digest is primary authority. No credential values.

## Negative example

```yaml
recipes:
  test:
    command: "pnpm test && curl evil"
    approved: true
```

## Normative schema

`recipes.yaml` MUST validate against `schemas/recipe.schema.json` from SCHEMA_PLAN. The normalized semantic object is `VerificationRecipe` from RUNNER-001; YAML syntax is only its configuration serialization.

## Field semantics

`version` selects schema major. `test_globs` identifies test files for integrity signals, not verification execution. `recipes` maps stable recipe keys to one deterministic claim plus exact argv/cwd/timeout/environment-name policy. Approval state is intentionally excluded.

## Required fields

Top-level `version`, `test_globs`, and `recipes` are required. Every recipe entry requires `claim` and `argv`; MVP scaffold/demo requires recipe keys `test` and `lint`.

## Optional fields

Per-recipe `cwd`, `timeout_ms`, and `env_allowlist` are optional in YAML and use the frozen defaults shown above. No other fields are permitted.

## Invariants

No recipe self-approval; no recipe may satisfy a claim other than its configured `claim`; no shell-string execution; normalized semantic changes produce a new digest and lose approval; formatting/comment-only changes do not. Test globs affect integrity observation, not claim proof by themselves.

## Validation rules

Apply JSON Schema first, then claim-type/family validation, semantic path/executable safety, shell-wrapper prohibition, duplicate/tag/alias rejection, default normalization, JCS digest computation, and ledger approval lookup. Any failure stops before subprocess launch.

## Required tests

Schema; claim binding/mismatch; unknown/duplicate/tag/alias; shell wrapper; path escape; defaults; digest/approval; glob fixtures.
