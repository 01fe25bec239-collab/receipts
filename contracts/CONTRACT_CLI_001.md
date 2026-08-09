# CONTRACT-CLI-001 — `receipts` CLI Semantics and Exit Codes

**Version:** 1.0.0  
**Owner:** A2-CLAUDE-INTEGRATION for entry contract; domain behavior owned by respective A2  
**Consumers:** hooks, skills, humans, all A2 components  
**Status:** FROZEN  
**First milestone:** M0  
**Depends on:** domain contracts as commands become available

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Provide one stable short-lived broker entry point. No daemon is introduced.

## Global output modes

Human mode is default. Machine mode uses `--json` and emits exactly one JSON object to stdout on every controlled outcome:

```text
CliEnvelope {
  version: 1
  command: string
  ok: boolean
  result: object?
  error: {
    category: INPUT|CONFIG|GIT|STORAGE|PROCESS|PROVIDER|POLICY|INTEGRITY|INTERACTION|INTERNAL
    code: string
    message: string
    detail: object?
  }?
}
```

Diagnostics/logging go to stderr. Raw logs are never dumped by default.

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | Successful command / requested positive state |
| 2 | Admission BLOCK / hook-semantic block |
| 3 | Negative admissible evidence (claim REJECTED) |
| 4 | Requested evidence remains UNPROVEN (failed/timeout/malformed provider) |
| 10 | INPUT/usage |
| 11 | CONFIG / recipe approval missing |
| 12 | GIT/state discovery |
| 13 | STORAGE |
| 14 | PROCESS failure before admissible receipt |
| 15 | PROVIDER operational failure not normalized to ReviewResult |
| 16 | POLICY |
| 17 | INTEGRITY |
| 18 | INTERACTION_REQUIRED |
| 19 | CANCELLED |
| 70 | INTERNAL invariant failure |

## Command matrix

| Command | Input | Output | Ledger mutation | Human interaction | Agent invocation | Primary exits |
|---|---|---|---|---|---|---|
| `receipts init` | repo cwd | repo/data/scaffold status | yes: initialize ledger; repo files scaffolded | only collision/overwrite protection; never silent overwrite | no | 0/10/11/12/13 |
| `receipts fingerprint [--recompute]` | repo cwd | CodeStateFingerprint | no semantic mutation | no | yes | 0/12 |
| `receipts verify <task> [recipeKey]` | task/key only; no command string | claim/receipt/cache status | yes if evidence/event created | no; returns 18 if approval requires human | yes | 0/3/11/18 |
| `receipts review <task>` | task, policy-resolved profile | ReviewResult/findings | yes | no | yes | 0/3/4 |
| `receipts status [task]` | optional task | current claims/fingerprint/admission diagnosis | no | no | yes | 0 |
| `receipts admit --task <id>` | task | recomputed Admission | yes: ADMISSION_EVALUATED when used as a gate; diagnostic calls may record source | no | yes | 0 or 2 |
| `receipts override <task> --reason <text>` | task/reason | OverrideRecord/new admission | yes | **required interactive confirmation** | **no** | 0/17/18/19 |
| `receipts verify-ledger` | repo | chain/projection report | no primary mutation | no | yes | 0/17 |
| `receipts export [--output <path>]` | repo/output | portable bundle metadata/path | no semantic mutation; writes output file | no | yes | 0/13/17 |
| `receipts recipe status [key]` | optional key | digest/approval status | no | no | yes | 0/11 |
| `receipts recipe approve <key>` | key | RecipeApproval | yes | **required interactive confirmation** | **no** | 0/11/18/19 |

## Detailed command behavior

### init
Requires a Git repository, but MAY run before the first commit by using CORE-001's persisted repository-identity fallback. It creates the broker data root/ledger and scaffolds `.receipts/recipes.yaml` and `.receipts/policy.yaml` only if absent. It MUST NOT overwrite an existing config. `fingerprint`, verification, and admission remain unavailable until a valid `HEAD` exists.

### fingerprint
Computes CORE-001. `--recompute` bypasses any non-authoritative memoized view. Output always reports complete fingerprint.

### verify
Agent/user selects task and recipe key only. Broker resolves approved recipe. Cache hit for exact fingerprint+recipeDigest returns PROVED without running. If command launches, ExecutionReceipt is recorded. Nonzero/timeout -> exit 3 REJECTED.

### review
Captures exact ReviewRequest and selects provider under policy. Blocking finding -> exit 3. Failed/timeout/malformed provider -> ReviewResult + exit 4.

### status
Read-only diagnosis. Shows STALE as stale, not failed; shows changed paths; shows override distinctly.

### admit
Always recomputes and appends `ADMISSION_EVALUATED` for that evaluation. Stored Admission never source of truth.

### override
No `--yes`, `--force`, noninteractive confirmation, stdin approval, or agent pathway in MVP.

### verify-ledger
Read-only to primary ledger; no repair.

### export
Verifies chain before successful export and uses atomic output replacement.

### recipe
`receipts recipe` with no subcommand is equivalent to `receipts recipe status`. `status [key]` is read-only. `approve <key>` displays normalized key/claim/argv/cwd/timeout/env names/digest before human confirmation. Approval binds exact digest in ledger. YAML has no approval field.

## Failure semantics

No evidence is fabricated for pre-launch process failure. Policy BLOCK is not INTERNAL. JSON mode remains parseable on controlled failures.

## Compatibility/versioning

Removing command/reusing exit code/changing meaning is major. Adding optional output field is minor.

## Security constraints

No shell-command CLI input. Agent-callable commands cannot grant human authority. Machine messages redact secrets.

## Normative schema

The global machine-readable response schema is `CliEnvelope` above. Each command's `result` MUST validate against the domain contract named in the command matrix (for example CORE-001 for `fingerprint`, RUNNER-002/CORE-003 for `verify`, REVIEW-002 for `review`, POLICY-002 for `admit`, OVERRIDE-001 for `override`, EXPORT-001 for `export`).

## Field semantics

`version` versions the CLI envelope, `command` is the normalized command name, `ok` means the CLI operation completed according to its command semantics (not necessarily that evidence was positive), `result` contains the typed controlled outcome, and `error` represents operational/config/input/integrity failures. Exit code remains the primary process-level classification and MUST agree with the envelope.

## Required fields

Machine mode always requires `version`, `command`, and `ok`. Exactly one of `result` or `error` is present for a controlled terminal outcome, except a command whose successful result is intentionally empty may use an empty object. `error.category`, `error.code`, and `error.message` are required when `error` is present.

## Optional fields

`error.detail` is optional and MUST be redacted/bounded. Command-specific optional result fields are defined by their domain schemas. Human-readable mode has no JSON-field requirement but MUST preserve the same decision semantics and exit code.

## Invariants

The CLI is the only hook entry point, is short-lived, and cannot accept agent-supplied shell commands as verification authority. Human-authority operations (`override`, `recipe approve`, authority-establishing `init`) are not agent-invokable. Stored admission is never trusted instead of recomputation.

## Validation rules

Argument parsing MUST reject unknown flags/extra positional data that would alter authority. Machine output MUST be one JSON object on stdout. Exit code/envelope pairs MUST be contract-consistent. Task/recipe/provider identifiers are treated as data, never shell fragments.

## Example

```json
{"version":1,"command":"admit","ok":true,"result":{"decision":"BLOCK","taskId":"AUTH-42","fingerprint":"<64hex>"}}
```
with process exit `2` for the BLOCK decision.

## Negative examples

`receipts verify AUTH-42 --command "pnpm test"`; JSON-mode diagnostics mixed into stdout; exit `0` for `BLOCK`; agent invocation of `receipts override`; `status` presenting `ADMITTED_WITH_OVERRIDE` as verified.

## Required tests

Human+JSON mode for every command; exact exits; stdout/stderr; interaction/cancel; cache hit; negative evidence vs operational error; agent invocation matrix.
