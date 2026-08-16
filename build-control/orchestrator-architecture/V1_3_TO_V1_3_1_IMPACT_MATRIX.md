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

# V1_3_TO_V1_3_1_IMPACT_MATRIX

Evidence closure and semantic correction only. **No architecture redesign.**

| Item | Status | Detail |
|---|---|---|
| **A-14 / C-03** — **[HISTORICAL]** obsolete Codex hook observation | **RETIRED** | Currently false per `C-02` (self-fetched). Preserved as a dated 2026-08-13 historical observation. `STALE_A14_CURRENT_ASSERTIONS = 0` enforced mechanically |
| **Codex host posture** | **REVISED** | Native plugin + hooks is now **primary**; supervised/hybrid retained as fallback for old versions, disabled hooks, untrusted hooks, and `codex exec` worker operations |
| **the V1.3 source matrix** | **SUPERSEDED** | Replaced by `SOURCE_VERIFICATION_MATRIX_V1_3_6.md`, **generated** from `evidence/SOURCE_CLAIM_REGISTRY.json` |
| **`ASSUMPTION_REGISTER.md`** | **REVISED** | No longer carries independent evidence status. Points at the registry. This removes the V1.3 contradiction where two documents disagreed |
| **`FeatureAdmissionDecision`** | **REVISED (narrowed)** | Provider, host, runtime and safety outcomes **removed**. Entitlement axis only |
| **`DispatchAdmissionDecision`** | **NEW** | Composes six axes by reference; names exactly one failing axis. Every provider dispatch consumes it |
| **`ActivationState`** | **NEW** | Resolves the `FREE` vs `ENTITLEMENT_UNKNOWN` ambiguity. A lost cache never downgrades a paying user |
| **`GraphEdge`** | **REVISED** | Class exclusivity enforced by conditional validation; four negative fixtures |
| **Provider policy classifications** | **REVISED** | Anthropic external worker `VERIFIED_DISALLOWED`; OpenAI consumer external worker `POLICY_NEEDS_REVIEW`; `USER_API` and enterprise paths `VERIFIED_ALLOWED` |
| **Provider subscription business claim** | **REVISED** | "Connect both and Pro uses both" removed. Participation is conditional on policy eligibility |
| **C-13** | **SPLIT** | OpenAI External Checkout `VERIFIED_CURRENT`; Anthropic paid checkout `UNVERIFIED / NOT_ESTABLISHED` |
| **C-14 prior art** | **CLOSED** | Three public projects reviewed. Novelty claims for graph execution, selective retry, cache, ASCII rendering and host-neutral DAG runtime **withdrawn** |
| **`HOST_PARITY_CONTRACT.md`** | **REVISED** | Behavioural parity retained; rationale updated from capability asymmetry to hook trust/disablement variance |
| Seven BUILD-A2 topology · ExecutionGraph · one graph core · Free/Pro split · SQLite · durable A1/A2 · fresh A3/A4 · exact-SHA review · automatic repair · Model Intelligence · context rehydration · cross-host graph · open core · no managed inference · safety separation | **UNCHANGED** | §14 preserved in full |

## Three findings beyond the reviewer's brief

Self-fetching the Codex hooks documentation surfaced constraints the reviewer summary did not contain, all of which strengthen the existing design rather than challenge it:

1. **`C-02a`** — plugin hooks are untrusted until the user reviews them, and trust is bound to the hook definition's hash, so **new or changed** hook definitions are marked for review and skipped until trusted. Our install UX must handle a state where the plugin is installed and its hooks are inert.
2. **`C-02b`** — hooks can be disabled entirely by user or admin configuration.
3. **`C-02d`** — `SessionEnd` allows at most 3 seconds; hook output is capped near 2500 tokens.

(1) and (2) independently confirm OpenAI's own warning that hooks are a guardrail rather than an enforcement boundary — which is why entitlement and security authority stay in the shared core.
