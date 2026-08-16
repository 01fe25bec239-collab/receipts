<!--
MultiAgent Orchestrator Architecture — HISTORICAL SNAPSHOT
DOCUMENT_AUTHORITY: HISTORICAL_SNAPSHOT
SNAPSHOT: V1.3
Issued by: BUILD-A1-BOOTSTRAP
Status: PRESERVED HISTORICAL RECORD — NOT current architectural truth.
ADR set as decided at V1.3, including ADR-HOST-001's then-correct rejection of retiring A-14 on unverified evidence. Current ADRs: ARCHITECTURE_DECISION_RECORDS_V1_3_2.md.
This document records what was believed at the time it was written. Where it
disagrees with a CURRENT_NORMATIVE document, the current document governs.
It contributes NO current evidence assertion to normative validation.
-->

# ARCHITECTURE_DECISION_RECORDS_V1_3

## ADR-GRAPH-001 — ExecutionGraph becomes a first-class runtime artifact

**Decision.** Promote `TaskDag` from an internal scheduling structure into a durable, versioned, auditable `ExecutionGraph` that is the authoritative plan and the primary user-facing object.

**Rejected — keep the DAG internal and add a viewer.** Cheaper, but a viewer over an ephemeral structure cannot be resumed, versioned, or upgraded from FREE to PRO without a migration. The graph *is* the FREE product; it cannot be a rendering detail.

**Rejected — a graph database.** SQLite with relational graph tables is sufficient at single-project scale. Adding an operational dependency to a local-first tool for no measured need would be premature.

**Consequences.** New contracts and schemas; append-only mutation with versioning; repair expressed as expansion rather than a cycle; `TaskDag` reduced to a compatibility view.

## ADR-COMMERCIAL-001 — One graph core, two execution policies

**Decision.** FREE and PRO share one engine, one graph model, one node identity space, one evidence model. `GraphExecutionPolicy` selects single-runtime or distributed dispatch.

**Rejected — separate Free and Pro engines.** Guarantees divergence, doubles maintenance, and makes the upgrade path a migration — which would also make it the least-tested path in the product.

**Rejected — Free as a time-limited trial.** §23 requires FREE to be genuinely useful. A trial that expires produces no ecosystem and no reason to contribute.

**Consequences.** Upgrade and downgrade are policy swaps over the same graph; no recompile; history always preserved.

## ADR-ENTITLEMENT-001 — Product entitlement is separate from provider authentication

**Decision.** Four independent axes: product entitlement, provider technical auth, provider policy eligibility, provider availability. Capability-based admission, not `if plan == "PRO"`.

**Rejected — infer tier from the host plan.** A Claude Max or ChatGPT Pro subscription is a relationship with another company and says nothing about ours.

**Rejected — plan checks at call sites.** Every new tier would require touching the engine.

**Consequences.** Signed local entitlement; FREE never depends on our service; distinct failure vocabulary; catalog UX.

## ADR-POLICY-001 — Technical authentication does not imply policy eligibility

**Decision.** `ProviderPolicyEligibility` is a first-class gate owned by MODEL-ROUTING. Routing may select only `VERIFIED_ALLOWED`. `UNKNOWN` and `NEEDS_REVIEW` are not usable by default.

**Rejected — treat authenticated as permitted.** Convenient and wrong: it converts every gap in our research into a compliance risk carried by the customer.

**Consequences.** The product must work with every subscription path disabled — and does, via `USER_API`, `ENTERPRISE_GATEWAY` and host-native FREE execution.

## ADR-HOST-001 — Host posture is discovered, not hardcoded

**Decision.** `HostCapabilityReport` is probed at install; the adapter selects EMBEDDED, SUPERVISED or HYBRID. Both hosts satisfy the same parity contract.

**Rejected — retire A-14 on the strength of an unverified report.** The claim that Codex now ships plugins is `USER_DECLARED`. Acting on it as fact would repeat the original error in the opposite direction.

**Rejected — keep supervised-only.** Equally wrong for the same reason: it hardcodes a 13 August snapshot.

**Consequences.** The unverified Codex fact stops being load-bearing. Verification changes fidelity, not architecture.

## ADR-OPENCORE-001 — Open core with proprietary Pro modules

**Decision.** Public: graph core, FREE policy, schemas, plugin shells, basic provenance, entitlement verifier, capability catalog. Proprietary: distributed orchestration policy, routing implementation, independent-A4 automation, advanced failover and provenance. Pro module delivered post-activation, signed.

**Rejected — fully open, honour system.** Makes the paid tier unfundable.

**Rejected — server-heavy orchestration.** Would compromise source-code privacy, local Git, customer-owned credentials and offline operation — damaging the product to protect its price.

**Consequences.** No DRM claim. Deters casual copying and organisational non-compliance; does not deter determined reverse engineering of a locally shipped module. Stated plainly rather than denied.
