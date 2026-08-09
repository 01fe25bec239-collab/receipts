# CONTRACT-LEDGER-001 — LedgerEvent

**Version:** 1.0.0  
**Owner:** A2-LEDGER  
**Consumers:** all event producers/consumers  
**Status:** FROZEN  
**First milestone:** M0  
**Depends on:** CORE-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Define append-only source of truth and byte-stable hash chain.

## Normative schema

```text
LedgerEvent {
  seq: integer >= 1
  ts: RFC3339 UTC
  kind: LedgerEventKind
  payload: object
  prevHash: HexSha256
  hash: HexSha256
}
```

Minimum event kinds: TASK_OPENED, CLAIM_ASSERTED, RECEIPT_RECORDED, REVIEW_RECORDED, ADMISSION_EVALUATED, OVERRIDE_GRANTED, WAIVER_GRANTED, RECIPE_APPROVED, POLICY_AMENDED.

## Canonical hash rule

This resolves the prior canonical-serialization blocker.

1. Hash body = `{seq, ts, kind, payload}`.
2. Serialize with RFC 8785 JCS as UTF-8.
3. Genesis prevHash = 64 ASCII `0` characters.
4. Later prevHash = prior lowercase hex hash.
5. `hash = hexlower(SHA-256(JCS(body) || ASCII(prevHash)))`.
6. Hash fields are excluded from body.
7. Timestamp normalized to UTC `Z`.
8. Payload must be finite JSON-compatible data; no NaN/Infinity/undefined/custom classes.

## Required / optional

All fields required.

## Invariants

Append-only. Chain continuity. Events source of truth. Projection never authority. Hash chain is tamper-evident relative to supervised agents, not machine owner.

## Validation/failure

Validate payload schema before append. Serialization/hash/storage failure aborts transaction.

## Compatibility/versioning

Any canonical/hash change major + export migration. New event kind requires versioned payload contract.

## Security constraints

Broker sole writer; data dir 0700 and DB 0600; never call tamper-proof.

## Example

```json
{"seq":12,"ts":"2026-08-09T04:15:00Z","kind":"RECEIPT_RECORDED","payload":{"receiptId":"r118"},"prevHash":"<64hex>","hash":"<64hex>"}
```

## Negative examples

Update event row; hash pretty JSON; locale ordering; projection-only mutation.

## Field semantics

`seq` is monotonically allocated ledger order, `ts` is event time, `kind` selects the typed payload, `payload` is canonical JSON-compatible event data, `prevHash` links the previous event (or genesis), and `hash` authenticates sequence/time/kind/payload plus prior link against accidental/agent-side mutation. Hash chaining is tamper-evident to supervised agents, not a machine-owner trust anchor.

## Required fields

All `LedgerEvent` fields are required.

## Optional fields

None at the envelope level. Event-kind payload schemas define their own allowed optional fields.

## Required tests

Golden JCS; genesis; Unicode/numbers; mutation; truncation/chain mismatch; independent hash implementation; rollback.
