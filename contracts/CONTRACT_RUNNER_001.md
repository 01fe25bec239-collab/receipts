# CONTRACT-RUNNER-001 — VerificationRecipe

**Version:** 1.0.0  
**Owner:** A2-RUNNER  
**Consumers:** A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M1  
**Depends on:** CONFIG-001, CORE-001, LEDGER-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Define the only executable authority for deterministic verification.

## Normative schema

```text
VerificationRecipe {
  key: string
  claimType: ClaimType
  argv: [string]
  cwd: RepoRelativePath = "."
  timeoutMs: integer > 0 = 600000
  envAllowlist: [EnvName] = []
  recipeDigest: HexSha256
}

RecipeApproval {
  recipeKey: string
  recipeDigest: HexSha256
  approvedBy: HumanIdentity
  approvedAt: RFC3339
}
```

Approval is ledger state, never a YAML boolean.

## Digest

`recipeDigest = SHA-256(JCS(normalized recipe entry including key, claimType, and defaults))`, lowercase hex. JCS = RFC 8785 JSON Canonicalization Scheme.

## Semantics

`claimType` binds this recipe to one recipe-backed deterministic ClaimType; the broker MUST reject use of a recipe for a different claim. argv is exact spawn vector; no shell. cwd resolved/realpath-checked inside repo. timeout broker-enforced. envAllowlist contains names only. Any semantic edit yields new digest and requires new human approval.

## Required / optional

key/claimType/argv required; cwd/timeout/env defaulted; runtime digest always required.

## Invariants

Only approved digest executes. Agent never supplies argv. Recipe change invalidates relevant evidence. Human approval interactive and ledger-recorded.

## Validation

key lower-case identifier; claimType upper-snake and must resolve to a recipe-backed deterministic claim definition; argv non-empty/no NUL; shell wrappers (`sh|bash -c`, PowerShell command strings) prohibited; cwd no absolute/`..`; env names portable; timeout positive; unknown fields invalid.

## Failure behavior

Unapproved recipe -> CONFIG/INTERACTION_REQUIRED, no process/receipt. Timeout after launch creates negative receipt.

## Compatibility/versioning

Shell-command mode would be breaking/architecture-sensitive. New semantic field changes digest and requires contract review.

## Security constraints

No shell, realpath cwd, minimal env, no credential values in recipe config, human approval outside YAML.

## Example

`test` binds `TESTED` to `["pnpm","test"]`, cwd `.`, timeout 600000, env names `["CI"]`.

## Negative examples

`command: "pnpm test && curl..."`; `["bash","-c","..."]`; `approved:true`; agent `--command`.

## Field semantics

`key` is the stable recipe key; `claimType` is the explicit config-level mapping to the deterministic claim the recipe may satisfy; `argv` is the exact executable argument vector; `cwd` is a repository-relative working directory resolved by the broker; `timeoutMs` bounds the subprocess; `envAllowlist` contains environment-variable names whose existing values may be passed through. Approval is intentionally absent from the recipe object and lives in ledger state keyed by recipe digest.

## Required fields

Normalized `VerificationRecipe` requires `key`, `claimType`, `argv`, `cwd`, `timeoutMs`, and `envAllowlist`; configuration input may omit fields that have frozen defaults in CONFIG-001.

## Optional fields

No fields are optional after normalization. Configuration-level omission of `cwd`, `timeout_ms`, and `env_allowlist` means the defaults defined by CONFIG-001, not unknown/null values.

## Required tests

Digest stability; claim/recipe mismatch; defaults; formatting-only YAML; semantic edit; approval expiry; cwd escape; env stripping; shell-wrapper rejection; unapproved never launches.
