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

# FAILURE_CRITERIA

Conditions that would indicate this architecture needs simplification or a pivot (§97). Stated plainly, because an architecture that cannot be falsified cannot be evaluated.

| # | Failure condition | How it would show | Response |
|---|---|---|---|
| F-1 | **Routing overhead exceeds its benefit** | Time and cost spent on intelligence, refresh, and decision-making is a large fraction of total spend, while outcomes match a fixed frontier default | Collapse to a small static preference list with manual override. The whole MODEL-ROUTING manager becomes optional. |
| F-2 | **Model intelligence cannot obtain reliable current data** | Providers do not expose enumeration or capability metadata; registry is mostly `UNKNOWN`; routing is effectively guessing | Fall back to user-declared model configuration. Honest, and better than a system pretending to know. |
| F-3 | **Automatic repair loops waste more compute than they save** | Repair attempts routinely fail and escalate; total cost to acceptance exceeds a human-in-the-loop baseline | Reduce default repair bound to 1; make repair opt-in per task. |
| F-4 | **Cross-provider integrations are too unstable** | Adapter breakage from CLI drift dominates engineering time | Narrow to two adapters and treat others as unsupported. Cross-provider routing was the differentiator, so this materially weakens the product. |
| F-5 | **Host parity impossible without invasive wrappers** | A parity row cannot be satisfied on Codex without simulating behaviour Codex does not have | **Do not quietly demote Codex.** Either narrow the parity set with a recorded decision, or accept a documented capability gap. Hiding it would be the real failure. |
| F-6 | **False blocking is excessive** | Gates reject work that is actually fine; users bypass them | Loosen blocking-finding definitions with evidence, not with pressure. If users bypass gates routinely, the gates are wrong. |
| F-7 | **Context rehydration fails to preserve manager continuity** | Rebound managers make decisions inconsistent with prior ones; quality drops after failover | The durable-role premise is wrong. Either enrich manifests substantially or accept `STRICT` failover as default — which costs availability. |
| F-8 | **Cheap routing repeatedly increases cost to acceptance** | Economy tasks routinely escalate | Raise floors; shrink the economy class. Cheap-model use was never the thesis. |
| F-9 | **Users routinely bypass integration gates** | Manual merges outside the system | The gate is too slow or too strict. A bypassed gate provides zero assurance while costing full overhead. |
| F-10 | **Provider auth restrictions make the intended mode non-viable** | Policy prohibits the local-invocation pattern `PERSONAL_LOCAL_MODE` depends on | Pivot to API-key-only operation. Changes the economics substantially for individual developers. |
| F-11 | **The differentiator is commoditised** | An incumbent ships cross-provider routing plus automatic independent review/repair | Compete on depth — quota economics, enterprise auth, provenance — or reconsider the product. The research already flagged this as the live risk. |
| F-12 | **Durable-role machinery is unnecessary** | Sessions rarely fail; simpler tools suffice | Substantial simplification: remove failover, bindings, and much of the state layer. This is the single largest structural bet in the architecture. |

## Honesty commitment

These are recorded before implementation, when they are cheap to admit. The old architecture's falsification section served the same purpose and was correct to exist; this one inherits that discipline.

Any of F-1, F-4, F-11, or F-12 proving true would mean the product is meaningfully smaller than described here. That should be stated when it happens, not discovered by a user.
