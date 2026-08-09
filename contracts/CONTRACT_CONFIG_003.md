# CONTRACT-CONFIG-003 — `.receipts/providers.yaml`

**Version:** 1.0.0  
**Owner:** A2-REVIEW  
**Consumers:** A2-REVIEW, A2-CORE, A2-CLAUDE-INTEGRATION  
**Status:** FROZEN  
**First milestone:** M4  
**Depends on:** REVIEW-003

## Freeze rule

This contract is controlled by A1-RECEIPTS. A frozen contract may not be changed by an A2/A3/A4 implementation agent. Any semantic, field, authority, security, compatibility, or failure-behavior change requires a `CONTRACT_CHANGE_REQUEST` reviewed by A1. A change that alters the frozen architecture additionally requires the architecture-deviation protocol.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in their RFC-style sense.

## Purpose

Keep provider, vendor, executable and model as configuration/runtime data.

## Normative YAML

```yaml
version: 1

selection_order:
  - codex
  - claude-session

providers:
  codex:
    adapter: codex_exec
    vendor: openai
    binary: codex
    enabled: true
    timeout_ms: 600000
    # model: "optional-provider-model-id"

  claude-session:
    adapter: claude_headless
    vendor: anthropic
    binary: claude
    enabled: true
    timeout_ms: 600000
    # model: "optional-provider-model-id"
```

## Fields

Top-level version required `1`; selection_order required unique provider IDs; providers required map.

Provider:
- adapter: `codex_exec | claude_headless` for MVP.
- vendor: required opaque nonempty string, only equality semantics.
- binary: required executable name/path.
- enabled: optional default true.
- timeout_ms: positive integer default 600000.
- model: optional nonempty requested model ID.

Unknown fields prohibited.

## Prohibited

No arbitrary args/extra_args/sandbox/permissions/full_auto/prompt/credentials/env values. Config cannot downgrade read-only constraints.

## Model/provider semantics

Configured model is a request. ReviewResult records model as reported by provider. Credentials are not stored here. New provider instance using existing adapter is config-only; new adapter (Gemini) needs current verification + contract update.

## YAML parsing

Safe YAML 1.2; duplicate keys/tags/anchors/aliases/merge keys rejected.

## Failure behavior

Invalid config -> CONFIG. Disabled/unhealthy provider skipped with recorded downgrade/selection fact.

## Security constraints

Adapter owns security-critical argv. Binary resolved/realpath captured. Provider auth values never copied into ledger/export.

## Negative example

`extra_args: ["--dangerously-bypass-approvals-and-sandbox"]` is invalid.

## Normative schema

`providers.yaml` MUST validate against `schemas/providers.schema.json`; validated entries instantiate REVIEW-003 adapters. Configuration may choose binaries/models/timeouts but cannot add authority or privilege flags.

## Field semantics

`version` selects schema major. `selection_order` provides deterministic provider preference. Each provider entry supplies stable provider `id` (map key), adapter kind, vendor attribute, executable path/name, enabled state, timeout, and optional configured model. Runtime `ReviewResult.model` is still recorded as reported by the provider.

## Required fields

Top-level `version`, `selection_order`, and `providers` are required. Each provider requires `adapter`, `vendor`, `binary`, and `enabled` after validation; configured defaults may supply `timeout_ms` where this contract explicitly defines one.

## Optional fields

`model` is optional. `timeout_ms` may be optional only where the frozen YAML/schema gives a default; unknown fields and arbitrary argument arrays are prohibited.

## Invariants

Provider/model identities are configuration, not architecture. Config cannot request write sandbox, bypass, tool expansion, custom arbitrary args, or credentials. Provider selection still obeys POLICY-001 distinct-vendor semantics and REVIEW-003 health/capability checks.

## Validation rules

Validate schema/enums, selection-order references and uniqueness, positive timeout, nonempty vendor/binary, allowed adapter values, and prohibition of unknown/privilege fields before launching a provider.

## Required tests

Schema; order refs/duplicates; disabled fallback; vendor equality; configured model; unknown fields; attempted privilege override; binary resolution.
