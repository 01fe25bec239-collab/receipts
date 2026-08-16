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

# GRAPH_EXECUTION_POLICIES

## The central commercial architecture

```
              ExecutionGraph          ← one model, one identity, one evidence chain
                    │
              ExecutionPolicy
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
 FREE single-runtime      PRO distributed
   graph policy         orchestration policy
```

**There is exactly one engine.** No `FreeGraphEngine` and `ProGraphEngine`. Same graph, same node identities, same evidence model, same scheduler. The policy changes **which capabilities are admitted and how nodes are dispatched** — nothing else.

Two engines would guarantee divergence, double the maintenance, and make the FREE→PRO upgrade a migration instead of a policy change. It would also make the upgrade path the least-tested path in the product.

## What a policy actually controls

| Dimension | FREE | PRO |
|---|---|---|
| Runtime breadth | One eligible host-native/local runtime | Multiple eligible runtimes |
| Provider selection | Current runtime only | Capability-based routing with Model Intelligence |
| Reviewer | Deterministic checks; optional same-context review | Fresh independent A4, cross-provider where eligible |
| Repair | Manual re-run, selective retry | Automatic bounded A3→A4→repair |
| Roles | `GraphCoordinator` (single logical coordinator) | Durable RUNTIME-A1 + RUNTIME-A2, ephemeral A3/A4 |
| Failover | None | Frontier failover, quota-aware scheduling |
| Cross-host resume | Graph resumes; single-runtime | Full distributed state resume |
| Assurance | Deterministic floor | LIGHT / STANDARD / HIGH_ASSURANCE |

## What FREE keeps unconditionally

Correctness and safety are **not** paywalled (§55, §114): no false PASS, accurate node state, cycle validation, safe Git boundaries, secret redaction, deterministic check integrity, crash-safe state, explicit failures, baseline sandboxing, safety-policy enforcement.

PRO sells orchestration scale and intelligence. It does not sell the absence of deliberately broken behaviour — a FREE tier that lies about test results would poison the product's entire premise.

## Policy is not a scheduler fork

The scheduler admits a node when dependencies are satisfied **and** `FeatureAdmissionDecision.outcome == ALLOW`. FREE and PRO differ in what admission returns, not in how scheduling works.

```
node READY
   → FeatureAdmission(node.required_capabilities)
       ├── ALLOW                → ADMITTED → dispatch per policy
       └── LOCKED_REQUIRES_PRO  → node state LOCKED_REQUIRES_PRO, zero dispatch
```

## Free may still be genuinely parallel

FREE is not "no concurrency". Bounded parallelism within one runtime is permitted where the runtime supports it, and same-host native subagents may be used if that is what a credible graph-engineering experience requires. What FREE must **not** silently gain is cross-provider routing, cross-provider independent A4, Model Intelligence multi-provider optimisation, provider failover, distributed durable manager federation, or Pro-only cross-host orchestration.

The monetisation line is **single-runtime vs distributed multi-runtime**, not "agents vs no agents". A FREE tier with no execution would be a demo, and §23 explicitly forbids that.

## Upgrade and downgrade are policy swaps

FREE → PRO: same `graph_id`, same versions, same accepted work. The policy changes and PRO may append routing/review nodes. No recompile, no new project, no reset.

PRO → FREE or expiry: graph, node history, provenance, results, accepted SHAs and checkpoints all persist and remain readable. New Pro-only dispatch stops. **Downgrade never deletes history** — the user paid for that work and it remains theirs.
