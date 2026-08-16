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

# DEFERRED_CAPABILITIES

Deliberately out of MVP, with the reason and the trigger for revisiting.

| Capability | Why deferred | Revisit when |
|---|---|---|
| `PRODUCT_TEAM_MODE` | Requires verified current terms per provider, tenant isolation, billing attribution, and a secret manager. Building compliance and multi-tenancy for an orchestrator that has not yet orchestrated anything is premature. | Core proven; a concrete team use case exists |
| Gemini adapter (TIER-2) | Verified headless + JSON (A-25), but the core must be proven on two runtimes first | After M6 parity passes |
| Kimi / Grok adapters | Runtimes exist (A-27, A-29) but headless/JSON/sandbox specifics are `UNVERIFIED` (A-28, A-30). Building against unverified surfaces produces adapters that break silently | Specifics verified against primary sources |
| DeepSeek / API-harness path | No confirmed official agent runtime (A-31); requires building our own loop, which is second-class for implementation work | An official runtime ships, or a harness-only use case justifies it |
| HIGH_ASSURANCE broker re-execution | Roughly doubles check cost for a benefit that matters mainly in adversarial settings — which the reframed threat model (§67) says is not the primary case | A user needs audit-grade assurance |
| Full security tooling pipeline | Tool selection is language- and project-specific; no universal scanner exists and inventing one would be worse than deferring (Q-06) | Per-project configuration model designed |
| Hash chain over the event log | The old threat model justified it; the new one does not, and an unsigned chain does not stop a local attacker with DB write access | `PRODUCT_TEAM_MODE`, or an external-audit requirement |
| Portable export + independent verifier | Valuable for audit; no MVP consumer | Team/audit use case |
| Calibration-driven routing | Needs sample size; routing on n=3 is worse than routing on documented evidence | Sufficient observations accumulated |
| MCP bridge | Not a core dependency (§91); used only if Codex integration genuinely benefits | Codex adapter shows a concrete gain |
| Web UI / dashboard | Commoditised, and the hosts are the UI | Not planned |
| Multi-repository goals | Substantial DAG and workspace complexity | Single-repo proven |
| Cost forecasting / spend dashboards | Budgets and enforcement exist; forecasting is polish | User demand |

## Rule

A deferred capability is **not** a dropped requirement. Each is traced in `REQUIREMENTS_TRACEABILITY_MATRIX.md` and has a revisit trigger. Nothing was deferred silently.

## V1.3 additions

| Capability | Why deferred | Revisit when |
|---|---|---|
| **Managed inference / provider resale** | §69 — explicitly **not required by Pro**. Would make us a reseller, invert the commercial model, and create provider-account concentration risk | Only if a distinct managed product is deliberately launched |
| Team cloud control plane | Requires multi-tenancy, isolation, billing attribution | After local-first is proven |
| Web dashboard | Hosts are the UI; terminal catalog satisfies §31 | User demand |
| Enterprise organisation licensing | Seat/device semantics unresolved (open question) | Enterprise demand |
| Centralised telemetry | Must never become a hidden licensing dependency (§43) | Separately designed and disclosed |
| **Knowledge graph** | §119 — explicitly out of scope. Focus is ExecutionGraph plus provenance relations only | V2+ |
| Full provenance visualisation | Text rendering suffices for MVP | After MVP |
| Marketplace paid checkout | `UNVERIFIED` whether supported (C-13); entitlement designed independent of it | If a marketplace commercial program is verified |
| Device-bound licensing | Optional field exists; semantics unresolved | If piracy measurably matters |
