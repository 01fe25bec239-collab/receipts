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

# HOST_CAPABILITY_DISCOVERY

## Why this document exists

V1.2.3 hardcoded a host posture: Claude Code embedded via plugin and hooks, Codex driven by an external supervisor. That was correct **given the facts available on 13 August 2026**, and it is exactly the kind of decision that rots when a vendor ships a feature.

§9 flagged it as a stale assumption candidate. §7 required re-verification. **That research is now closed** — registry claims `C-01` and `C-02` are `VERIFIED_CURRENT_SELF_FETCHED` (2026-08-15) — but the architecture below was built to stop depending on the answer, so closing the research changed no design.

Rather than guess in either direction, the architecture stops depending on the answer.

## The mechanism

```
adapter.discover()
   → probe host for: plugin manifest support, hook events, skills,
                     commands, subagents, MCP, data path, sandbox
   → HostCapabilityReport (persisted, versioned, timestamped)
   → select integration mode
```

| Discovered | Mode | Event provenance |
|---|---|---|
| Native lifecycle hooks present | **EMBEDDED** — register hooks, emit `OBSERVED` events | mostly `OBSERVED` |
| No native hooks | **SUPERVISED** — companion process, derive events from process output | mixed `OBSERVED` / `INFERRED` |
| Partial | **HYBRID** — native where available, derived elsewhere | per-event |

Both modes satisfy the same `HOST_PARITY_CONTRACT`. The parity conformance suite runs against whichever mode is active, so a host that gains hooks later gains fidelity without an architecture change.

## Consequence for A-14 — now retired

Verification happened. Registry claims `C-01` and `C-02` are `VERIFIED_CURRENT_SELF_FETCHED`: Codex has native plugins and lifecycle hooks.

- A-14 is **RETIRED as a current assumption**; it is currently false.
- It is preserved as a dated 2026-08-13 historical observation and nothing more.
- Discovery selected EMBEDDED for Codex, and **no architecture document needed to change** — which is exactly what the discovery mechanism was built to guarantee.

Discovery is not now redundant. It remains load-bearing for three verified reasons: hook coverage varies by host version; plugin hooks require user trust before they run (`C-02a`); and hooks can be disabled by user or admin configuration (`C-02b`). A host that *supports* hooks is not the same as a host where *our* hooks are currently running.

## Modes are no longer equally-weighted unknowns

| Host | Primary mode | Basis |
|---|---|---|
| Claude Code | **EMBEDDED** | `C-04`, `C-05` |
| Codex | **EMBEDDED** | `C-01`, `C-02` — verified this pass |
| Any host, hooks absent / disabled / untrusted | SUPERVISED or HYBRID | runtime discovery |

Discovery now answers *"is the native path currently usable here?"* rather than *"does this host have hooks at all?"*

## HostCapabilityReport

`host_id` · `host_version` · `probed_at` · `last_verified_at` · `probe_status` · `validity_fingerprint` · `hook_definition_digest` · `relevant_config_digest` · `stale_reason` · `plugin_supported` · `plugin_installed` · `manifest_path` · `supports_skills` · `supports_commands` · `supports_subagents` · `supports_mcp` · `hooks_supported` · `hooks_configured` · **`hook_trust_required`** · **`hooks_trusted`** · **`hooks_enabled`** · **`hooks_allowed_by_admin_policy`** · `hook_events[]` · `blocking_hook_events[]` · `hook_coverage_class` · **`required_hook_coverage_satisfied`** · `selected_mode` · `mode_override` · `inactive_reason` · `plugin_data_path` · `sandbox_modes[]` · `evidence_label` · `source_claim_id`

This is the canonical field vocabulary — it must match `schemas/HostCapabilityReport.schema.json` exactly; `HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES = 0` enforces it. `plugin_installed` is distinct from `plugin_supported`: a host can support plugins at all without ours being installed. `hooks_trusted`, `hooks_enabled` and `hooks_allowed_by_admin_policy` exist because of `C-02a` and `C-02b`: a host may fully support hooks while *our* hooks are untrusted, globally disabled, or excluded by admin policy. Reporting only `hooks_supported` would make an inert installation look healthy. `hook_trust_required` is the host-neutral trust-model flag: `true` for a host with an explicit trust model (Codex, `C-02a`) — where `hooks_trusted` must then be a concrete boolean, never `null` — and `false` for a host with no trust model, where a `null` `hooks_trusted` is legitimate rather than ambiguous. `required_hook_coverage_satisfied` is independent of `hook_coverage_class`: `hook_coverage_class` describes coverage against the host's entire vendor hook surface (a host can be `PARTIAL` there), while `required_hook_coverage_satisfied` says whether this orchestrator's own required lifecycle events are actually covered — only the latter gates `EMBEDDED`. `selected_mode` is the discovery result (`EMBEDDED`/`HYBRID`/`SUPERVISED`); `inactive_reason` names exactly why the native path is not selected, under a deterministic precedence order (plugin install → hooks support → hooks configured → trust → enabled → admin policy → coverage → unknown) so the reason reported is never masked by a lower-precedence condition, and the system never silently waits for events that cannot arrive.

Owned by **BUILD-A2-HOST-INTEGRATION**. Persisted so a mode change between releases is visible rather than silent.

## Freshness, unknowns and deterministic selection

`evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json` is the single parseable authority for validity inputs, re-probe triggers and selection. At install, plugin lifecycle/config/trust/policy/definition or host-version change, session start/resume, or before EMBEDDED when freshness cannot be proven: perform a lightweight fingerprint check; reuse only a matching report, otherwise re-probe then select. Never probe per command. A stale or unproven report never authorizes EMBEDDED.

`COMPLETE` means every load-bearing state is known. `PARTIAL` preserves known facts and represents unknown facts as `null`; `FAILED` invents no capability truth and uses conservative SUPERVISED + `UNKNOWN`. UNKNOWN is allowed only when no higher-precedence known failure exists. A COMPLETE healthy native path selects EMBEDDED; HYBRID/SUPERVISED requires an explicit `mode_override` source and reason when it departs from that result.

## What this does not solve

Discovery establishes what a host **can** do technically. It says nothing about what is **contractually permitted** — that is `ProviderPolicyEligibility`, a separate gate. A host that technically supports plugin-driven worker dispatch may still be ineligible for it commercially.
