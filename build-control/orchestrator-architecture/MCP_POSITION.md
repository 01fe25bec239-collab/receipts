<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# MCP_POSITION

## Position (§91)

> **MCP is not a mandatory core dependency.**

Used where a host or provider integration gains real value; not added for symmetry, and never as a substitute for durable orchestrator state.

## Where MCP may earn its place

**Codex host integration's compatibility fallback.** Codex supports MCP both as client and as server (A-10 family). The primary `CodexHostAdapter` path is the native plugin and its lifecycle hooks; where MCP gives the supervised/hybrid fallback a cleaner bridge than process supervision alone, it is a legitimate implementation choice for that fallback path only.

**Exposing orchestrator operations to a host.** A narrow MCP surface (`start_goal`, `status`, `dag`, `routing_decision`) is a reasonable way to give a host access to core operations without embedding logic in the adapter.

## Where MCP is explicitly not used

- **Not** added to the Claude Code adapter for symmetry. Claude Code's plugin, hooks, and skills already provide a richer, more direct integration; adding MCP there would be a second path to the same place with more moving parts.
- **Not** as a transport for durable state. Product state lives in the state store, never in MCP session memory.
- **Not** as the worker-execution mechanism. Workers are driven through agent runtimes (`RUNTIME_ADAPTER_INTERFACE.md`).

## If an MCP interface is designed

Verify the current official MCP specification at design and implementation time; use the current recommended protocol architecture; do not rely on obsolete session assumptions; and keep all durable product state in the orchestrator store, not in hidden session memory.

MCP specifics carry **no current-source claim** in `evidence/SOURCE_CLAIM_REGISTRY.json` — no MCP behaviour was verified, so any MCP-dependent design must carry its own verification before implementation. Codex plugins may bundle `.mcp.json` (registry claim `C-01`), but that establishes packaging, not any behaviour this architecture relies on.

## Consequence for the architecture

Nothing in the core depends on MCP. If MCP proves unnecessary for Codex, the architecture is unchanged. That independence is the point: an optional integration mechanism should not be able to become a load-bearing dependency by accident.
