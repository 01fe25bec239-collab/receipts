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

# REPOSITORY_LAYOUT_PROPOSAL

```
receipts/                              (name provisional)
├── src/
│   ├── core/
│   │   ├── orchestration/   goal orchestrator, role engine        ORCHESTRATION
│   │   ├── goal/            goal + evaluator                      ORCHESTRATION
│   │   ├── dag/             graph, edges, cycle detection         ORCHESTRATION
│   │   ├── capsules/        task/repair capsule construction      ORCHESTRATION
│   │   └── scheduler/       admission, concurrency, budgets       ORCHESTRATION
│   ├── intelligence/        model intelligence service            MODEL-ROUTING
│   ├── registry/            provider/model/runtime registries     MODEL-ROUTING
│   ├── routing/             router, estimator, policies           MODEL-ROUTING
│   ├── availability/        quota + availability manager          MODEL-ROUTING
│   ├── adapters/
│   │   ├── runtime/         claude, codex, gemini, harness        RUNTIME-ADAPTERS
│   │   └── credentials/     credential broker                     RUNTIME-ADAPTERS
│   ├── workspace/           git, branches, worktrees, checkpoints WORKSPACE-EXECUTION
│   ├── execution/           process runner, capture, recovery     WORKSPACE-EXECUTION
│   ├── review/              A3→A4 controller, verdicts            REVIEW-INTEGRATION
│   ├── assurance/           profiles, provenance                  REVIEW-INTEGRATION
│   ├── security/            security pipeline, safety states      REVIEW-INTEGRATION
│   ├── integration/         acceptance + integration gates        REVIEW-INTEGRATION
│   ├── state/               store, schema, repositories           STATE-CONTEXT
│   ├── context/             manifests, epochs, rehydration        STATE-CONTEXT
│   ├── events/              append-only event log, redaction      STATE-CONTEXT
│   └── hosts/               host adapter interface, detection     HOST-INTEGRATION
├── plugins/claude/          .claude-plugin, hooks, skills, cmds   HOST-INTEGRATION
├── plugins/codex/           .codex-plugin, hooks, skills — native, PRIMARY   HOST-INTEGRATION
├── integrations/codex-fallback/  supervised/hybrid companion, codex exec driving (fallback only)   HOST-INTEGRATION
├── schemas/                 one file per contract                 contract owner (per-file `x-owner`)
├── tests/
│   ├── unit/                mirrors src ownership
│   ├── integration/
│   ├── parity/              host parity conformance               HOST-INTEGRATION
│   └── security/            security acceptance tests             REVIEW-INTEGRATION
├── docs/                    owned by the subsystem's manager
├── architecture/            this package, once accepted           BUILD-A1
└── build-control/           BUILD-A1 control artifacts            BUILD-A1
```

## Principles

1. **Directory boundaries match manager boundaries.** Ownership is inferable from a path, which makes write-scope verification mechanical rather than judgemental.
2. **One schema file per contract**, owned by that contract's owner.
3. **Tests mirror ownership**, except `parity/` and `security/`, which are cross-cutting and owned by the manager that defines the requirement.
4. **`docs/` has no single owner** — each subsystem's documentation belongs to its engineering manager (§87). There is no docs directory owner because there is no docs manager.
5. **`build-control/` is BUILD-A1 only.** No BUILD-A2 writes it.

## Required path authority

**Canonical, machine-derived — not prose.** Every required implementation path below must resolve to exactly one owner in `BUILD_A2_OWNERSHIP_MATRIX.md`: a `BUILD-A2-*` engineering manager, or the explicit `BUILD-A1`-controlled class. `evidence/validate_package.py` derives `UNOWNED_REQUIRED_PATHS`, `AMBIGUOUS_REQUIRED_PATHS` and `REQUIRED_PATH_OWNER_MISMATCHES` from this table — never by grepping surrounding prose. `PATH_OWNER_COLLISIONS = 0` proves the ownership matrix is internally consistent; this table separately proves it is *complete*.

| Required path | Required owner |
|---|---|
| `src/core/orchestration/**` | BUILD-A2-ORCHESTRATION |
| `src/core/goal/**` | BUILD-A2-ORCHESTRATION |
| `src/core/dag/**` | BUILD-A2-ORCHESTRATION |
| `src/core/capsules/**` | BUILD-A2-ORCHESTRATION |
| `src/core/scheduler/**` | BUILD-A2-ORCHESTRATION |
| `src/core/graph/**` | BUILD-A2-ORCHESTRATION |
| `src/core/entitlement/admission/**` | BUILD-A2-ORCHESTRATION |
| `src/pro/orchestration/**` | BUILD-A2-ORCHESTRATION |
| `src/intelligence/**` | BUILD-A2-MODEL-ROUTING |
| `src/registry/**` | BUILD-A2-MODEL-ROUTING |
| `src/routing/**` | BUILD-A2-MODEL-ROUTING |
| `src/routing/policy_eligibility/**` | BUILD-A2-MODEL-ROUTING |
| `src/availability/**` | BUILD-A2-MODEL-ROUTING |
| `src/pro/model-routing/**` | BUILD-A2-MODEL-ROUTING |
| `src/adapters/runtime/**` | BUILD-A2-RUNTIME-ADAPTERS |
| `src/adapters/credentials/**` | BUILD-A2-RUNTIME-ADAPTERS |
| `src/workspace/**` | BUILD-A2-WORKSPACE-EXECUTION |
| `src/execution/**` | BUILD-A2-WORKSPACE-EXECUTION |
| `src/review/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/assurance/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/security/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/integration/**` | BUILD-A2-REVIEW-INTEGRATION |
| `tests/security/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/pro/review-integration/**` | BUILD-A2-REVIEW-INTEGRATION |
| `src/state/**` | BUILD-A2-STATE-CONTEXT |
| `src/context/**` | BUILD-A2-STATE-CONTEXT |
| `src/events/**` | BUILD-A2-STATE-CONTEXT |
| `src/state/entitlement/**` | BUILD-A2-STATE-CONTEXT |
| `src/hosts/**` | BUILD-A2-HOST-INTEGRATION |
| `plugins/claude/**` | BUILD-A2-HOST-INTEGRATION |
| `plugins/codex/**` | BUILD-A2-HOST-INTEGRATION |
| `integrations/codex-fallback/**` | BUILD-A2-HOST-INTEGRATION |
| `tests/parity/**` | BUILD-A2-HOST-INTEGRATION |
| `architecture/**` | BUILD-A1 |
| `build-control/**` | BUILD-A1 |

Paths not listed here (`schemas/**`, resolved per-file by each schema's `x-owner`; `docs/**`, resolved per-subsystem-directory; `tests/unit/**`, which mirrors `src/` ownership rather than carrying its own) are explicitly excluded, not silently unowned — each has its own stated resolution mechanism above.

## Dependency direction

`state` and `context` depend on nothing internal. `hosts`, `plugins`, and `integrations` are depended on by nothing. A CI check enforces that `src/core/**` contains no import from `src/hosts/**`, `plugins/**`, or `integrations/**` — the mechanical guarantee behind "one shared core" (I-16) and cross-host resume.

## Name

The repository is still `receipts`, which no longer describes the product. Renaming is deferred (`OPEN_QUESTIONS.md` Q-08) — it is cosmetic, and doing it before the architecture is accepted would churn history for no benefit.

## V1.3 additions

```
src/
├── core/
│   ├── graph/            ExecutionGraph, compiler, scheduler, mutation   [PUBLIC]
│   └── entitlement/
│       └── admission/    FeatureAdmission                                [PUBLIC]
├── state/
│   └── entitlement/      token persistence, verification, cache          [PUBLIC verifier]
├── routing/
│   └── policy_eligibility/  ProviderPolicyEligibility                    [PUBLIC]
├── pro/
│   ├── orchestration/        distributed execution policy      [PROPRIETARY] ORCHESTRATION
│   ├── model-routing/        distributed routing implementation [PROPRIETARY] MODEL-ROUTING
│   └── review-integration/   automatic A4 / repair automation  [PROPRIETARY] REVIEW-INTEGRATION
plugins/
├── claude/               .claude-plugin, skills, hooks, commands, bin    [PUBLIC]
└── codex/                .codex-plugin, hooks, skills, bin — native, PRIMARY   [PUBLIC]
integrations/
└── codex-fallback/       supervised/hybrid compatibility companion       [PUBLIC]
fixtures/graphs/          graph validation fixtures                       [PUBLIC]
fixtures/host_capability/ HostCapabilityReport validation fixtures        [PUBLIC]
```

**`src/pro/**` is the only proprietary source path.** It is deliberately outside every path promised as OSS (§121), so the open-core boundary is a directory boundary rather than a per-file judgement. **It is not one undifferentiated Pro owner.** Each deterministic subtree is owned by the engineering manager that owns the corresponding public capability — `src/pro/orchestration/**` by `BUILD-A2-ORCHESTRATION`, `src/pro/model-routing/**` by `BUILD-A2-MODEL-ROUTING`, `src/pro/review-integration/**` by `BUILD-A2-REVIEW-INTEGRATION` — so no manager gains write authority over another manager's proprietary code. `BUILD_A2_OWNERSHIP_MATRIX.md` is authoritative for the concrete mapping.
