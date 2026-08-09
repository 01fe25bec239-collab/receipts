# CONTRACT-REVIEW-003 — ReviewProvider

**Version:** 1.0.0  
**Owner:** A2-REVIEW  
**Consumers:** A2-CORE, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY  
**Status:** FROZEN  
**First milestone:** M4  
**Depends on:** REVIEW-001/002, CONFIG-003, POLICY-001

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Keep provider integration deliberately small and read-only.

## Normative interface

```text
ReviewProvider {
  id: string
  vendor: string
  health() -> { ok: boolean, detail: string }
  capabilities() -> {
    structuredOutput: boolean
    maxDiffBytes: integer
    readOnlyEnforced: boolean
    resume: boolean
  }
  review(req: ReviewRequest) -> ReviewResult
  cancel(reviewId: string) -> void
}
```

No general session management, delegation, write operations, or routing framework.

## Provider resolution

Use enabled providers in CONFIG-003 selection order. Skip unhealthy providers and record skip. Policy `distinct_vendor`: required = different vendor only; preferred = different if healthy else same-vendor fallback + recorded downgrade; off = vendor equality irrelevant. Vendor/model strings are data.

## Frozen MVP process surfaces

### Codex

Broker argv:

```text
codex exec --sandbox read-only --json --output-schema <schema> -o <out>
            --ignore-user-config --ignore-rules <broker-review-prompt>
```

Spawn as argv, never shell. Optional configured model adds `--model <model>`. `--full-auto`, write sandbox, danger-full-access, approval/sandbox bypass are prohibited.

### Claude same-vendor fallback

Current Claude Code `--bare` skips hooks, skills, plugins, MCP, auto-memory and CLAUDE.md. Broker argv:

```text
claude --bare -p --tools Read,Grep,Glob
       --output-format json --json-schema <schema-json>
       --no-session-persistence <broker-review-prompt>
```

Optional configured model adds `--model <model>`. Exact diff is supplied as broker-controlled context/stdin, never shell syntax. This is a separate process/session; it provides session independence but not model/vendor independence.

## Invariants

Reviewer read-only mandatory. `capabilities().readOnlyEnforced=false` makes provider ineligible for admission-producing review. Fallback downgrade explicit.

## Failure behavior

Health failure -> skip/fallback. Required different vendor unavailable -> REVIEWED UNPROVEN. Timeout/malformed -> UNPROVEN.

## Compatibility/versioning

New adapter (e.g. Gemini) requires current-interface verification and contract change. Arbitrary config extra_args prohibited.

## Security constraints

No write tools, no shell, no config-controlled privilege flags, auth values not persisted.

## Example

Claude implementer under STANDARD chooses Codex; Codex unhealthy -> Claude fallback with downgrade record.

## Negative examples

Codex `--full-auto`; Claude Bash/Edit tools; reviewer fixes finding; config overrides sandbox.

## Field semantics

`id` is the configured provider identity and `vendor` is the policy-comparison attribute. `health()` determines temporary usability. `capabilities()` advertises properties the broker may enforce, especially read-only review. `review()` is the only operation that obtains a review result. `cancel()` may terminate an outstanding provider operation but cannot alter recorded historical evidence.

## Required fields

Every provider implementation MUST expose `id`, `vendor`, `health`, `capabilities`, `review`, and `cancel`. Capability objects MUST include all four documented booleans/limits.

## Optional fields

None in the interface. Provider-specific model/binary/timeout values are optional configuration fields under CONFIG-003 and are not added to this architecture interface.

## Validation rules

Provider `id` MUST resolve to one enabled CONFIG-003 entry. Runtime `vendor` MUST equal that entry's configured vendor attribute. A provider is ineligible for admission-producing review when `readOnlyEnforced` is false, output cannot be normalized to REVIEW-002, or the requested diff exceeds its declared `maxDiffBytes` without a contract-approved transport strategy. Returned `reviewId` MUST match the request.

## External interface sources

- Claude Code CLI reference — `https://code.claude.com/docs/en/cli-usage` — accessed 2026-08-09.
- Codex CLI reference — `https://developers.openai.com/codex/cli/reference/` — accessed 2026-08-09.

These URLs support only mutable process syntax/capabilities. Receipts review authority semantics remain defined by this contract.

## Required tests

Health; vendor modes; fallback/downgrade; read-only enforcement; write attempt denied; cancellation; configured vs reported model.
