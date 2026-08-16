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

# OLD_RECEIPTS_REUSE_MATRIX

Item-by-item disposition of the historical Receipts concepts (§34, §68).

| Concept | Old purpose | Disposition | New role |
|---|---|---|---|
| **CodeStateFingerprint** | Bind evidence to exact code state | **KEEP_BUT_REFRAME** | Simplified to SHA + dirty-state binding. The full working-tree digest machinery was built for adversarial staleness detection; the orchestrator controls its own worktrees, so git SHAs carry most of the weight. Retain digest only where uncommitted state must be identified. |
| **Exact-SHA evidence binding** | A review of `abc` never validates `xyz` | **KEEP_AS_IS** | I-5. The single most valuable idea from the old design and the backbone of `INTEGRATION_GATE_ARCHITECTURE.md`. |
| **ExecutionReceipt** | Immutable record of a verification run | **KEEP_BUT_REFRAME** | Becomes `WORKER_EXECUTION` / `BROKER_EXECUTION` evidence in `Provenance`. Same fields largely survive; the framing shifts from "proof against a lying agent" to "durable record across sessions". |
| **ReviewEvidence** | Probabilistic review as a distinct family | **KEEP_BUT_REFRAME** | A4 verdicts and findings. The deterministic/probabilistic distinction is retained conceptually but is no longer an enforced type-level invariant — the orchestrator's gates are about SHA identity and blocking findings, not family substitution. |
| **Staleness** | Evidence invalid after code change | **KEEP_AS_IS** | Directly reused. Prefer false invalidation over false validity. |
| **Append-only events** | Tamper-evident history | **KEEP_AS_IS** | `EVENT_MODEL.md`, `STATE_AND_CHECKPOINT_ARCHITECTURE.md`. |
| **Hash chain** | Tamper-evidence over the event log | **DEFER** | The threat model changed: the adversary is no longer a dishonest agent, and a local attacker with DB write access defeats an unsigned chain anyway (the old architecture admitted this). Append-only + git provenance is sufficient for MVP. Revisit for `PRODUCT_TEAM_MODE`. |
| **VerificationPolicy / profiles** | LIGHT / STANDARD verification policy | **KEEP_BUT_REFRAME** | Becomes `ASSURANCE_PROFILES.md` with LIGHT / STANDARD / HIGH_ASSURANCE. |
| **`admit()` purity** | Pure admission function | **MODIFY** | The integration gate is deterministic code with explicit inputs, but it performs I/O (git, test re-execution). Purity as a *type-level* discipline is dropped; determinism and reconstructibility are kept. |
| **Admission / AdmissionDecision** | Gate on claim satisfaction | **KEEP_BUT_REFRAME** | `IntegrationDecision` with ACCEPT / REPAIR / BLOCKED / HUMAN_REQUIRED. |
| **Human override** | Break-glass, fingerprint-scoped, never rendered as proof | **KEEP_BUT_REFRAME** | Becomes `HUMAN_REQUIRED` plus explicit user authority at the integration gate. The rule that an override never renders as proof survives as an overclaim prohibition. |
| **VerificationRecipe + approval** | Only human-approved commands execute | **RETIRE** | Built to stop an agent inventing commands. The orchestrator *issues* the verification plan in the capsule, so the authority question is answered structurally. Argv-only, no-shell execution discipline is **kept** in `WORKSPACE_EXECUTION_ARCHITECTURE.md`. |
| **Broker-only ledger writes** | Single writer to evidence | **KEEP_AS_IS** | I-18: core-only write path to the state store. |
| **Claim types (IMPLEMENTED/TESTED/…)** | Four MVP claim types | **RETIRE** | Replaced by task acceptance criteria and A4 verdicts. The evidence *labels* (`IMPLEMENTED`/`TESTED`/`NOT_TESTED`/`BLOCKED`/`ASSUMED`) are **kept** in `A3_HANDOFF`. |
| **ReviewProvider (4 ops)** | Deliberately tiny provider interface | **KEEP_BUT_REFRAME** | Becomes `RuntimeAdapter`. Necessarily larger, because it now drives agent runtimes rather than one-shot reviews — but the minimality discipline is retained. |
| **Portable export + independent verifier** | Evidence verifiable without the codebase | **DEFER** | Valuable, not MVP. Revisit when a team/audit use case exists. |
| **Enforcement-scope honesty** | Never claim enforcement beyond what is enforced | **KEEP_AS_IS** | `SECURITY_TRUST_MODEL.md` overclaim prohibitions; I-20. |
| **Evidence-family separation** | LLM verdict cannot prove TESTED | **MODIFY** | Retained as a design principle rather than a hard type invariant; expressed through assurance profiles and blocking-finding definitions. |
| **Build-control methodology (A1–A4)** | How Receipts itself was to be built | **KEEP_AS_IS, and promote** | Reused as BUILD-control **and** promoted into the product runtime. The methodology turned out to be the product. |
