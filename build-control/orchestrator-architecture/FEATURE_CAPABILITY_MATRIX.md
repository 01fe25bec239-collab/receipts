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

# FEATURE_CAPABILITY_MATRIX

## Capability IDs, not plan checks

Runtime admission asks *"is capability X permitted?"* — never `if plan == "PRO"`. Tier names map to capability sets; the engine checks capabilities. Adding Studio, Team or Enterprise later changes a catalog, not the engine.

Capability IDs are namespaced extensible strings (`^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`), never an enum.

## Catalog

| Capability ID | Feature | Tier | Notes |
|---|---|---|---|
| `graph.core` | Execution Graph compile and execute | FREE | |
| `graph.inspect` | Graph tree, status, node detail | FREE | |
| `graph.validate` | Cycle detection, edge validation | FREE | Safety floor — never paywalled |
| `graph.cache` | Local result cache | FREE | |
| `graph.selective_retry` | Retry a node without rerunning the graph | FREE | |
| `graph.deterministic_checks` | Local quality gates | FREE | |
| `graph.checkpoint` | Crash-safe checkpointing | FREE | Safety floor |
| `graph.workspace_isolation` | Git worktree isolation | FREE | Safety floor |
| `graph.provenance_basic` | Node results with exact SHA | FREE | |
| `graph.resume` | Resume an interrupted graph | FREE | |
| `graph.single_runtime` | Execute via one eligible runtime | FREE | |
| `catalog.view` | See all Free and Pro capabilities | FREE | Pro is never hidden from Free |
| `orchestration.multi_runtime` | Dispatch across multiple eligible runtimes | PRO | |
| `orchestration.distributed_roles` | Durable RUNTIME-A1/A2, ephemeral A3/A4 | PRO | |
| `review.independent_a4` | Fresh independent auditor | PRO | |
| `review.cross_provider` | Reviewer on a different eligible provider | PRO | Requires a second eligible runtime |
| `review.automatic_repair` | Automatic bounded A3→A4→repair | PRO | |
| `routing.model_intelligence` | Current-evidence capability routing | PRO | |
| `routing.provider_failover` | Frontier failover on unavailability | PRO | |
| `routing.cost_to_accepted_result` | Expected-cost optimisation | PRO | |
| `resume.cross_host` | Full distributed resume across hosts | PRO | |
| `provenance.advanced` | Cross-provider attribution, routing archaeology | PRO | |
| `assurance.standard_or_higher` | STANDARD and HIGH_ASSURANCE profiles | PRO | |

## Per-feature status vocabulary

`AVAILABLE_FREE` · `AVAILABLE_ENTITLED` · `LOCKED_REQUIRES_PRO` · `UNAVAILABLE_PROVIDER` · `UNAVAILABLE_POLICY` · `UNAVAILABLE_HOST` · `UNAVAILABLE_RUNTIME` · `BLOCKED_SAFETY`

These are **not** interchangeable. A capability the user has paid for but cannot use because no second provider is eligible is `UNAVAILABLE_PROVIDER`, never `LOCKED_REQUIRES_PRO`. Telling a paying customer to upgrade when the real problem is provider eligibility is the specific failure §71 forbids.

## Catalog UX

`SHOW_CAPABILITIES` renders, on both hosts, with no model tokens consumed:

```
Feature                     Tier    Status
Execution Graph             FREE    AVAILABLE
Dependency Scheduling       FREE    AVAILABLE
Selective Retry             FREE    AVAILABLE
Local Quality Gates         FREE    AVAILABLE

Multi-Runtime Routing       PRO     LOCKED_REQUIRES_PRO
Independent A4 Review       PRO     LOCKED_REQUIRES_PRO
Cross-Provider Review       PRO     LOCKED_REQUIRES_PRO
Automatic Repair            PRO     LOCKED_REQUIRES_PRO
Model Intelligence          PRO     LOCKED_REQUIRES_PRO
Provider Failover           PRO     LOCKED_REQUIRES_PRO
Cross-Host Resume           PRO     LOCKED_REQUIRES_PRO
```

Filterable by `ALL` / `FREE` / `PRO`. Every Pro entry carries name, description, requirement and current status. **Pro is never hidden from Free users** — a user cannot choose to buy what they cannot see.

## Safety floor is not a tier

`graph.validate`, `graph.checkpoint`, `graph.workspace_isolation`, accurate PASS/FAIL, secret redaction and safety-policy enforcement are FREE and unconditional. They are listed as capabilities for catalog completeness, not as things that could ever be withheld.
