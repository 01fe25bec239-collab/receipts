# CONTRACT-CORE-001 — CodeStateFingerprint

**Version:** 1.0.0  
**Owner:** A2-CORE  
**Consumers:** A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M0  
**Depends on:** architecture §§C,G,Z

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Define the exact repository/code state to which all evidence is bound. No fingerprint means no admissible evidence.

## Normative schema

```text
CodeStateFingerprint {
  repoId: RepositoryId
  headSha: GitObjectId
  dirty: boolean
  workingTreeDigest: HexSha256
  fingerprint: HexSha256
}
```

`RepositoryId` is the architecture-defined stable repo identity string: normally the SHA-256 of the first root-commit object ID; when no root commit exists, a UUID persisted in broker storage is the fallback identity. `HexSha256` is 64 lowercase hex. `GitObjectId` is the exact lowercase ID returned by Git; consumers MUST NOT hardcode SHA-1 length.

## Field semantics

- `repoId`: SHA-256 of the architecture-selected first root-commit object ID returned from `git rev-list --max-parents=0 HEAD`; if no root commit exists, use the UUID fallback persisted in the ledger.
- `headSha`: `git rev-parse HEAD`.
- `dirty`: true iff staged, tracked-unstaged, or untracked-not-ignored state differs from HEAD.
- `workingTreeDigest`: SHA-256 over index entries plus current-byte overlays for tracked-unstaged and untracked-not-ignored files.
- `fingerprint`: SHA-256 of UTF-8 `repoId + "|" + headSha + "|" + workingTreeDigest`.

`receipts init` MAY establish/persist the repository identity before a first commit using the architecture's UUID fallback. A materialized `CodeStateFingerprint`, deterministic verification, and admission require a valid Git `HEAD`; no null/synthetic `headSha` is permitted.

## Working-tree digest

Use NUL-safe Git output. Include:

```text
INDEX\0<raw-path>\0<mode>\0<git-blob-id>\0
WORKTREE\0<raw-path>\0<sha256-or-DELETED>\0
UNTRACKED\0<raw-path>\0<sha256>\0
```

Sort bytewise by raw path then record class and hash the concatenated bytes. Ignored files never participate. Tracked files modified both staged and unstaged contain both INDEX and WORKTREE records.

## Required / optional

All fields required. No optional fingerprint fields in MVP.

## Invariants

1. Every evidence item names exactly one fingerprint.
2. Any included code-state change changes fingerprint.
3. Exact revert restores prior fingerprint.
4. Whole-tree invalidation in MVP.
5. Fingerprint is identity, not proof.

## Validation

Git commands run at broker-discovered repo root. Any Git failure, ambiguous root, invalid status parse, or unreadable included file fails the fingerprint computation; no partial fingerprint.

## Failure behavior

Fail closed for evidence/admission. A missing fingerprint cannot create positive evidence or ADMIT.

## Compatibility/versioning

Any digest-input/encoding/ordering/repo-identity change is major. Optional diagnostic additions are minor only if fingerprint bytes remain identical.

## Security constraints

Agent text cannot supply authoritative repoId/headSha/digest/fingerprint. Paths are broker/Git-derived and NUL-safe.

## Example

```json
{"repoId":"<repo-id>","headSha":"<git-id>","dirty":true,"workingTreeDigest":"<64hex>","fingerprint":"<64hex>"}
```

## Negative examples

HEAD-only fingerprint; hashing ignored build output; accepting an agent-provided digest; comparing a future path-scoped fingerprint as if it were full-tree.

## Required fields

`repoId`, `headSha`, `dirty`, `workingTreeDigest`, and `fingerprint` are all required in every materialized `CodeStateFingerprint`.

## Optional fields

None in MVP. Diagnostic changed-path information is separate from the fingerprint contract and MUST NOT change fingerprint equality semantics.

## Required tests

Clean stability; staged edit; unstaged edit; staged+unstaged same file; untracked file; ignored file; delete; revert restoration; filenames with whitespace/newlines; missing HEAD; golden cross-implementation digest fixture.
