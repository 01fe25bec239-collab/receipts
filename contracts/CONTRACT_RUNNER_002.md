# CONTRACT-RUNNER-002 — ExecutionReceipt

**Version:** 1.0.0  
**Owner:** A2-RUNNER  
**Consumers:** A2-CORE, A2-LEDGER, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M1  
**Depends on:** CORE-001, CORE-003 (AgentIdentity), RUNNER-001, EVIDENCE-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Record what the broker actually ran, where, when, against which state, and what it observed.

## Normative schema

```text
ExecutionReceipt {
  receiptId: string
  claimId: string
  recipeKey: string
  recipeDigest: HexSha256
  repoId: RepositoryId
  baselineSha: GitObjectId
  headSha: GitObjectId
  workingTreeDigest: HexSha256
  fingerprint: HexSha256
  argv: [string]
  cwd: AbsoluteRealPath
  resolvedExecutable: AbsoluteRealPath
  startedAt: RFC3339
  finishedAt: RFC3339
  durationMs: integer >= 0  # optional MVP diagnostic
  exitCode: integer
  timedOut: boolean
  stdoutDigest: HexSha256
  stderrDigest: HexSha256
  stdoutBytes: integer >= 0  # optional MVP diagnostic
  stderrBytes: integer >= 0  # optional MVP diagnostic
  rawLogRef: string
  runnerVersion: string
  invokedByAgent: AgentIdentity?  # field required; value nullable
  parsed: object?                 # field required; value nullable
}
```

MVP admission MUST NOT depend on deferred `toolVersionDigest`, env-value digest, runnerHost, runnerUser.

## Semantics / invariants

Exact argv/cwd/executable. Output digests hash raw bytes. Exit 0 may prove mapped deterministic claim only if fingerprint/digest/schema valid. Nonzero/timeout negative evidence. Spawn failure before child launch does not fabricate receipt. Receipt does not prove test meaningfulness/correctness/toolchain integrity.

## Validation

Require `Claim.type == VerificationRecipe.claimType`, then recheck fingerprint immediately before spawn. If state changes during run, receipt remains historical for launch fingerprint and is stale relative to current state. Timestamps ordered; digests valid.

## Failure behavior

Spawn failure -> PROCESS error/no receipt. Launched nonzero/timeout -> receipt and negative claim state.

## Compatibility/versioning

Removing MVP fields/changing exit semantics breaking; optional reproducibility fields minor.

## Security constraints

Raw logs outside SQLite, restrictive permissions, bounded excerpts through hooks.

## Example

A receipt with `argv:["pnpm","test"]`, exit 1, parsed `{passed:237,failed:1}` is negative deterministic evidence.

## Negative examples

Receipt from agent text; receipt after launch failure; review output as receipt; reusing after recipe digest changed.

## Field semantics

Identity/provenance fields name the claim, recipe, repository, baseline and exact fingerprint. Execution fields record exact argv/cwd/executable and timing. Outcome fields record exit/timeout plus raw-byte output digests and sizes. `invokedByAgent` records who triggered broker execution but does not make that agent an evidence producer. `parsed` is a bounded structured interpretation of output and never supersedes exit/digest facts.

## Required fields

The architecture's MVP-required fields are: `receiptId`, `claimId`, `recipeKey`, `recipeDigest`, `repoId`, `baselineSha`, `headSha`, `workingTreeDigest`, `fingerprint`, `argv`, `cwd`, `resolvedExecutable`, `startedAt`, `finishedAt`, `exitCode`, `timedOut`, `stdoutDigest`, `stderrDigest`, `rawLogRef`, `runnerVersion`, `invokedByAgent`, and `parsed`. The `invokedByAgent` and `parsed` fields are present but may contain null.

## Optional fields

`durationMs`, `stdoutBytes`, and `stderrBytes` are optional MVP diagnostics supported by the architecture's full receipt schema but not required by the architecture's MVP field subset. `invokedByAgent` may be null for human/broker-triggered verification; `parsed` may be null when output has no supported parser. Deferred architecture fields (`toolVersionDigest`, `envDigest`, `envAllowlist`, `runnerHost`, `runnerUser`) are not part of MVP-required schema.

## Required tests

Success, nonzero, timeout, spawn failure, claim/recipe mismatch, exact argv/cwd, raw byte digests, fingerprint drift, large output.
