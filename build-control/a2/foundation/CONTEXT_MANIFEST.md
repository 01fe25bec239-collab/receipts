<!--
Receipts — A2 Component Manager Initialization (5-manager FINAL topology, V2)
Issued by: A1-BOOTSTRAP (temporary bootstrap A1: designs, freezes, and packages the
           Receipts multi-agent operating system; retires on authority transfer)
Issued: 2026-08-10
Repository: 01fe25bec239-collab/receipts   Remote: origin -> https://github.com/01fe25bec239-collab/receipts   Integration branch: main
CONTRACT_FREEZE_SHA: 2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221
AGENT_SYSTEM_FREEZE_SHA: <AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>
Authority at runtime: A1-RUNTIME (not yet initialized). Report upward to the
currently active A1 -- never to a specific model, vendor, or conversation.
Supersedes the 8-manager package for manager topology only. Product architecture and
frozen contract semantics are UNCHANGED.
-->

# CONTEXT_MANIFEST — A2-FOUNDATION

**Purpose.** This manifest controls what you ingest. The five-manager topology exists to reduce managerial overhead; loading every contract into every manager would give the overhead back immediately. Read `MANDATORY` in full before acting. Open `CONSUMED` when a task touches it. Open `REFERENCE` only on demand. Never modify `EXCLUDED`.

## Class definitions

| Class | Meaning |
|---|---|
| **MANDATORY** | Ingest in full during initialization, and re-read before issuing any A3 task. |
| **CONSUMED** | Your component directly depends on it and must obey it. Load when a task touches it. |
| **REFERENCE** | May be inspected if a specific question requires it. Do not load for every task. |
| **EXCLUDED / FOREIGN OWNERSHIP** | Visible, never yours. **Do not modify. Do not redefine. Do not assume ownership.** |

`EXCLUDED` does not mean invisible. You may read anything in the repository to understand a contract. You may write only what `OWNERSHIP_MANIFEST.md` grants.

## MANDATORY

- `Receipts_Final_Architecture.md` — §§C, G, L, M, Q, T, Z and the closing falsification section
- `architecture-decisions/ARCHITECTURE_DEVIATION_REQUEST_001.md`
- `orchestration/01_ARCHITECTURE_AUTHORITY.md`, `04_CONTRACT_INDEX.md`, `05_MILESTONE_PLAN.md`, `10_OPEN_ISSUES.md`, `11_INTEGRATION_GATES.md`, `12_EVIDENCE_REQUIREMENTS.md`
- `contracts/CONTRACT_INDEX.md`
- **All ten owned contracts, in full**
- `schemas/SCHEMA_PLAN.md` — you own eight of the eighteen planned schemas
- `A2_CONSOLIDATION_DECISION.md`, `A2_OWNERSHIP_REMAP.md`
- This manager file, `CONTEXT_MANIFEST.md`, `OWNERSHIP_MANIFEST.md`, `FIRST_MANAGER_TASK.md`

## CONSUMED

- `contracts/CONTRACT_RUNNER_001.md`, `CONTRACT_RUNNER_002.md` — receipt facts that drive deterministic claim status
- `contracts/CONTRACT_REVIEW_002.md` — review result facts that drive review claim status
- `contracts/CONTRACT_OVERRIDE_001.md` — override representation inside admission
- `contracts/CONTRACT_CLI_001.md` — typed errors and exit contract (GAP-001)
- `orchestration/03_IMPLEMENTATION_DEPENDENCY_GRAPH.md`, `08_DEPENDENCY_REQUESTS.md`, `14_AGENT_HANDOFF_PROTOCOL.md`

## REFERENCE

- `contracts/CONTRACT_PLUGIN_001.md`, `CONTRACT_PLUGIN_002.md` — how your admission facts are surfaced and gated
- `contracts/CONTRACT_REVIEW_001.md`, `CONTRACT_REVIEW_003.md` — provider request shape and provider resolution
- `orchestration/00`, `02`, `06`, `07`, `09`, `13`, `15`
- The superseded 8-manager package, **if committed to the repository** — historical planning input only, never authority, and never required to operate

## EXCLUDED / FOREIGN OWNERSHIP

- `contracts/CONTRACT_CONFIG_001.md` (A2-VERIFICATION), `contracts/CONTRACT_CONFIG_003.md` (A2-TRUST) — foreign configuration surfaces you never touch
- All foreign source trees and schemas listed in `OWNERSHIP_MANIFEST.md`

Visible, never yours. Do not modify, do not redefine, do not assume ownership.

## Contract classification — all 21 frozen contracts

| Contract | Subject | Owner | Your class |
|---|---|---|---|
| `CONTRACT_CORE_001.md` | CodeStateFingerprint | A2-FOUNDATION | **OWNED** |
| `CONTRACT_CORE_002.md` | Task + TaskState | A2-FOUNDATION | **OWNED** |
| `CONTRACT_CORE_003.md` | Claim + ClaimStatus | A2-FOUNDATION | **OWNED** |
| `CONTRACT_EVIDENCE_001.md` | Evidence families + Evidence | A2-FOUNDATION | **OWNED** |
| `CONTRACT_POLICY_001.md` | VerificationPolicy | A2-FOUNDATION | **OWNED** |
| `CONTRACT_POLICY_002.md` | Admission + AdmissionDecision | A2-FOUNDATION | **OWNED** |
| `CONTRACT_CONFIG_002.md` | .receipts/policy.yaml | A2-FOUNDATION | **OWNED** |
| `CONTRACT_LEDGER_001.md` | LedgerEvent | A2-FOUNDATION | **OWNED** |
| `CONTRACT_LEDGER_002.md` | Append-only event / projection invariants | A2-FOUNDATION | **OWNED** |
| `CONTRACT_EXPORT_001.md` | Portable ledger export | A2-FOUNDATION | **OWNED** |
| `CONTRACT_RUNNER_001.md` | VerificationRecipe | A2-VERIFICATION | CONSUMED |
| `CONTRACT_RUNNER_002.md` | ExecutionReceipt | A2-VERIFICATION | CONSUMED |
| `CONTRACT_CONFIG_001.md` | .receipts/recipes.yaml | A2-VERIFICATION | EXCLUDED |
| `CONTRACT_PLUGIN_001.md` | Normalized hook -> broker request | A2-CLAUDE-INTEGRATION | REFERENCE |
| `CONTRACT_PLUGIN_002.md` | Broker -> hook decision / error response | A2-CLAUDE-INTEGRATION | REFERENCE |
| `CONTRACT_CLI_001.md` | receipts CLI semantics + exit codes | A2-CLAUDE-INTEGRATION | CONSUMED |
| `CONTRACT_REVIEW_001.md` | ReviewRequest | A2-TRUST | REFERENCE |
| `CONTRACT_REVIEW_002.md` | ReviewResult + ReviewFinding | A2-TRUST | CONSUMED |
| `CONTRACT_REVIEW_003.md` | ReviewProvider | A2-TRUST | REFERENCE |
| `CONTRACT_CONFIG_003.md` | .receipts/providers.yaml | A2-TRUST | EXCLUDED |
| `CONTRACT_OVERRIDE_001.md` | Override / Waiver semantics | A2-TRUST | CONSUMED |

All 21 are **1.0.0 FROZEN** at `CONTRACT_FREEZE_SHA`. Every contract has exactly one owner and no contract is multiply owned — verified across all five managers.

## Context discipline for delegation

When you issue an A3 task, you pass down a **subset** of this manifest, not the manifest itself: one atomic task, the architecture sections it needs, the frozen contracts it consumes, the source files it touches. An A3 that receives your whole context will produce work sized to your whole context.

Everything an A3 or A4 needs must be reachable from repository artifacts at a named SHA. No downstream agent may be expected to rely on a prior conversation, and no context class in this manifest is satisfied by recollection.
