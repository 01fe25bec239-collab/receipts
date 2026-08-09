# CONTRACT-EXPORT-001 — Portable Ledger Export

**Version:** 1.0.0  
**Owner:** A2-LEDGER  
**Consumers:** A2-EVALUATION, A2-DOCS-RELEASE, future CI/L4 verifier  
**Status:** FROZEN  
**First milestone:** M5  
**Depends on:** LEDGER-001/002 and event payload contracts

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Produce a portable independently hash-verifiable JSON snapshot without upgrading local trust claims.

## Normative schema

```text
LedgerExport {
  format: "receipts-ledger-export"
  version: 1
  exportedAt: RFC3339
  repoId: RepositoryId
  brokerVersion: string
  contractVersions: map<string,string>
  eventCount: integer >= 0
  chainHead: HexSha256
  events: [LedgerEvent]
  artifacts: [ExportArtifact]
}

ExportArtifact {
  ref: string
  kind: LOG | DIFF | REVIEW_RAW
  sha256: HexSha256
  bytes: integer >= 0
}
```

Raw artifact bytes are not inlined in MVP; manifest allows an external bundle to carry them later. Event-chain verification is self-contained.

## Invariants

Events preserved exactly. Overrides/waivers/downgrades/negative evidence never filtered. chainHead equals last hash or genesis zero hash for empty ledger. Export snapshot not local source of truth. No tamper-proof claim.

## Validation/failure

Verify chain before successful export. Integrity failure => exit 17/no successful bundle. I/O uses temp + atomic rename so no partial success file.

## Compatibility/versioning

Top-level export version controls verifier. Canonical/hash breaking change bumps export version.

## Security constraints

No credentials/env values. Raw logs not inlined by default.

## Example

```json
{"format":"receipts-ledger-export","version":1,"exportedAt":"2026-08-09T05:30:00Z","repoId":"<repo-id>","brokerVersion":"0.1.0","contractVersions":{"CONTRACT-LEDGER-001":"1.0.0"},"eventCount":42,"chainHead":"<64hex>","events":[],"artifacts":[]}
```

## Negative examples

Only positive events; override omitted; timestamps rewritten; exporting invalid chain as verified.

## Field semantics

`format`/`version` identify the portable bundle contract; `repoId` binds export to repository identity; `contractVersions` declares interpretation dependencies; `eventCount`/`chainHead` summarize the exported authoritative event chain; `events` preserves ordered LedgerEvent records; `artifacts` is a manifest of broker-owned referenced content included or separately materialized according to the export mode.

## Required fields

All top-level fields shown in the normative schema are required. Each event MUST satisfy LEDGER-001. Each artifact manifest entry MUST identify its reference/path, content digest, byte length, and kind when that artifact is included/referenced by the bundle.

## Optional fields

Raw logs are not embedded by default in MVP. Optional artifact payload inclusion MAY be added only in a compatible mode that preserves the same manifest/digest semantics and does not expose secrets by default.

## Required tests

Independent verifier; mutation; empty ledger; override/downgrade preservation; atomic file; artifact manifest digest.
