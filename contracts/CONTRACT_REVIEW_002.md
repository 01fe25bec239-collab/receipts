# CONTRACT-REVIEW-002 — ReviewResult + ReviewFinding

**Version:** 1.0.0  
**Owner:** A2-REVIEW  
**Consumers:** A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-EVALUATION  
**Status:** FROZEN  
**First milestone:** M4  
**Depends on:** REVIEW-001, POLICY-001/002, EVIDENCE-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Normalize review-provider output into structured review evidence without treating model assertions as deterministic proof.

## Normative schema

```text
ReviewStatus = COMPLETED | FAILED | TIMEOUT | MALFORMED
Severity = INFO | LOW | MEDIUM | HIGH | CRITICAL

ReviewFinding {
  findingId: string
  severity: Severity
  category: string
  path: RepoRelativePath?
  line: integer >= 1 ?
  summary: string
  rationale: string
  resolvesClaimId: string?
}

ReviewResult {
  reviewId: string
  providerId: string
  vendor: string
  model: string?
  startedAt: RFC3339
  finishedAt: RFC3339
  status: ReviewStatus
  findings: [ReviewFinding]
  rawRef: string
  parseOk: boolean
}
```

## Semantics

`model` is provider-reported, never copied from requested config. A COMPLETED result MUST carry a nonempty reported model; FAILED/TIMEOUT/MALFORMED may leave it null when the provider never reported identity. COMPLETED+parseOk with no policy-blocking finding may satisfy REVIEWED. Blocking finding -> REVIEWED REJECTED. FAILED/TIMEOUT/MALFORMED or parseOk=false -> REVIEWED UNPROVEN.

## Required / optional

All ReviewResult fields. Finding path/line/resolvesClaimId optional.

## Invariants

Review never proves deterministic claim. Malformed never defaults green. Findings remain structured. Provider/model substitution remains visible.

## Validation

Enums exact; MALFORMED implies parseOk=false; line requires path; IDs unique; vendor nonempty; COMPLETED requires nonempty provider-reported model; rawRef required once invocation starts.

## Failure behavior

Provider invocation that starts and fails/times out/malforms should yield corresponding ReviewResult when raw evidence exists. Claim remains UNPROVEN.

## Compatibility/versioning

Severity order/status semantics breaking; optional finding metadata minor.

## Security constraints

Parser never executes output. Finding paths are data until repo validation.

## Example

```json
{"reviewId":"v07","providerId":"codex","vendor":"openai","model":"<reported>","startedAt":"2026-08-09T04:20:00Z","finishedAt":"2026-08-09T04:20:22Z","status":"COMPLETED","findings":[{"findingId":"f1","severity":"HIGH","category":"auth","path":"src/auth/rotate.ts","line":64,"summary":"Reuse window","rationale":"...","resolvesClaimId":null}],"rawRef":"reviews/v07.raw","parseOk":true}
```

## Negative examples

Configured model copied as reported identity; parseOk=false + PROVED; severity BLOCKER; review used for TESTED.

## Field semantics

`providerId` and `vendor` identify the runtime provider; `model` records the model as reported by provider output/runtime rather than trusting configured intent. `status` describes provider execution; `parseOk` describes structured-output validity. Findings are reviewer assertions with severity/category/location/rationale; they remain probabilistic review evidence.

## Required fields

All `ReviewResult` fields except `model` are required; `model` is conditionally required for COMPLETED. In each `ReviewFinding`, `findingId`, `severity`, `category`, `summary`, and `rationale` are required.

## Optional fields

`ReviewResult.model` is optional only for FAILED/TIMEOUT/MALFORMED when the provider did not report identity; it MUST NOT be populated from configured intent. `ReviewFinding.path`, `line`, and `resolvesClaimId` are optional. Absence of location MUST NOT be converted to a fabricated path/line.

## Required tests

Status/parse matrix; thresholds; empty findings; invalid line/path; provider model identity; malformed parser.
