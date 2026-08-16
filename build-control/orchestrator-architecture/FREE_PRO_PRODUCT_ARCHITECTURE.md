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

# FREE_PRO_PRODUCT_ARCHITECTURE

## Product shape

```
                    ONE PRODUCT
                         │
                 EXECUTION GRAPH CORE
                         │
             ┌───────────┴───────────┐
             ▼                       ▼
           FREE                     PRO
   graph engineering        distributed multi-runtime
     single runtime              orchestration
```

## What the customer pays us for

Graph execution infrastructure, orchestration, model/runtime coordination, review and repair automation, routing intelligence, failure recovery, provenance, cross-host continuity.

**Not** inference. We do not sell, bundle, or resell model credits, and we do not proxy customer work through our own provider accounts. The customer brings whatever provider access they are permitted to use.

```
CUSTOMER ──pays──▶ provider A
         ──pays──▶ provider B
         ──pays──▶ OUR PRODUCT ──▶ orchestration software
```

## FREE is a real product

FREE is not a crippled trial. It includes: goal/spec → explicit graph; nodes and edges; dependency scheduling; graph validation and cycle detection; graph tree and status; deterministic local quality checks; local cache; selective retry; checkpointing; Git/worktree isolation; basic provenance; graph resume; failure isolation; bounded parallelism where the runtime supports it; one eligible runtime; inspectable results; and the full Pro capability catalog visible.

**FREE must not require our entitlement service to be reachable.** A network failure to our licensing infrastructure must never disable graph engineering. That is a hard rule, not a courtesy.

## The monetisation line

Not *"Free = no agents"*. The line is:

```
FREE : single-runtime / single-provider graph execution
PRO  : distributed multi-runtime / multi-provider graph orchestration
```

FREE may use bounded parallelism and same-host native subagents where that is what a credible graph-engineering tool requires. FREE must **not** silently acquire: cross-provider routing · cross-provider independent A4 · Model Intelligence multi-provider optimisation · provider failover · distributed durable RUNTIME-A2 federation · Pro-only cross-host orchestration.

## PRO

PRO unlocks the entire V1.2.3 distributed orchestrator: durable RUNTIME-A1 and RUNTIME-A2, ephemeral A3/A4, Model Intelligence, capability routing, cross-provider independent review when eligible, automatic A3→A4→repair, frontier failover, quota-aware scheduling, manager executor failover, cross-host resume, advanced provenance, expected-cost-to-accepted-result routing, higher assurance profiles.

**PRO unlocks orchestration software. It does not grant provider credentials.**

## The provider-participation claim, corrected (V1.3.1)

The product must **not** be described as *"connect Claude Max and ChatGPT Pro and Pro uses both as external workers."* That is not supported by current evidence — the Anthropic path is `VERIFIED_DISALLOWED` and the OpenAI consumer path is `POLICY_NEEDS_REVIEW`.

The accurate principle:

> **Use customer-owned provider access where the provider's current policy permits the execution context.**

Pro still unlocks distributed orchestration, multi-runtime routing, RUNTIME-A1/A2/A3/A4, review and repair, Model Intelligence, provider failover and cross-host continuity. **Actual provider participation is conditional** on policy eligibility, and a customer may hold Pro while a given provider path stays unroutable.

Honest composite status:

```
PRO_ACTIVE
+ ORCHESTRATION_MULTI_RUNTIME          = UNLOCKED
+ CLAUDE_SUBSCRIPTION_EXTERNAL_WORKER  = POLICY_DISALLOWED
+ OPENAI_SUBSCRIPTION_EXTERNAL_WORKER  = POLICY_NEEDS_REVIEW
+ USER_API_RUNTIME                     = AVAILABLE_WHEN_CONFIGURED
```

**A provider-policy failure is never reported as `LOCKED_REQUIRES_PRO`.** The customer has paid; the constraint is elsewhere, and saying otherwise would be both wrong and insulting.

## PRO with one eligible provider

A paying customer with a single policy-eligible runtime must never see `UPGRADE_REQUIRED` — they already paid. The honest report is:

```
PRODUCT_ENTITLEMENT   = PRO_ACTIVE
MULTI_RUNTIME_FEATURE = UNLOCKED
CROSS_PROVIDER_REVIEW = UNAVAILABLE_NO_SECOND_ELIGIBLE_RUNTIME
```

The system may run same-provider independent sessions where assurance policy allows, preserving independence by fresh context, and **must label the result `PROVIDER_DIVERSITY = SAME_PROVIDER`**. Claiming cross-provider review when one provider participated would be a false evidence claim — the same class of defect as a false PASS.

## Differentiation — prior art reviewed (V1.3.1)

C-14 is closed. Current public prior art was reviewed; **none of its code was copied or consulted for implementation.**

| Project | Already provides |
|---|---|
| `gwaghmar/graph` | Claude Code, Codex, OpenCode and Cursor thin host adapters; shared local runtime; DAG execution; caching; quality gates; selective retries; resume; zero-token live graph rendering |
| `ayaangazali/graph-engineering` | Claude graph decomposition; JSON-schema handoffs; adversarial verification |
| `heggria/taskflow` | Host-neutral compiled task graphs; resume/replay; incremental recomputation; multiple coding-agent hosts |

### What we may no longer claim as novel

Graph execution · Claude + Codex graph support · selective retry · local cache · ASCII/zero-token graph rendering · host-neutral DAG runtime · resume · caching · quality gates.

**All of these are already public and free.** V1.3 listed several of them as differentiation hypotheses; that was wrong, and stating so now is cheaper than discovering it after launch.

This also sets the FREE tier's competitive floor: a Free tier lacking any of the above is **not competitive**, because a developer can already get all of it for nothing.

### Intended differentiation — the combination, still a hypothesis

- first-class **durable versioned** ExecutionGraph with an audited mutation history;
- **the same graph** across Free and Pro, upgraded by policy rather than migration;
- **signed product capability entitlement** with axis-separated admission;
- **provider policy eligibility** as a first-class routable/non-routable gate;
- distributed RUNTIME-A1/A2/A3/A4;
- current **Model Intelligence** rather than training-memory selection;
- **expected-cost-to-accepted-result** routing;
- **policy-filtered** multi-provider execution;
- **independent fresh A4** with exact-SHA review;
- automatic bounded repair;
- provenance/evidence graph;
- durable cross-host continuation.

**Superiority remains a hypothesis until measured.** No individual element above may be published as novel on its own.
