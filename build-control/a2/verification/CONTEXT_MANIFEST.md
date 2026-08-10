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

# CONTEXT_MANIFEST — A2-VERIFICATION

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

- `Receipts_Final_Architecture.md` — §I.2, §T, and the sections cited by RUNNER-001/002
- `architecture-decisions/ARCHITECTURE_DEVIATION_REQUEST_001.md`
- `orchestration/01_ARCHITECTURE_AUTHORITY.md`, `05_MILESTONE_PLAN.md`, `10_OPEN_ISSUES.md`, `11_INTEGRATION_GATES.md`, `12_EVIDENCE_REQUIREMENTS.md`
- `contracts/CONTRACT_INDEX.md`
- **All three owned contracts, in full**
- `contracts/CONTRACT_CORE_001.md`, `CONTRACT_EVIDENCE_001.md` — you cannot specify a receipt without them
- `schemas/SCHEMA_PLAN.md` — `recipe.schema.json` and `receipt.schema.json`
- `A2_CONSOLIDATION_DECISION.md`, `A2_OWNERSHIP_REMAP.md`
- This manager file, `CONTEXT_MANIFEST.md`, `OWNERSHIP_MANIFEST.md`, `FIRST_MANAGER_TASK.md`

## CONSUMED

- `contracts/CONTRACT_CORE_003.md` — claim identity and `AgentIdentity` provenance
- `contracts/CONTRACT_LEDGER_001.md`, `CONTRACT_LEDGER_002.md` — receipt persistence and raw-log referencing
- `contracts/CONTRACT_CLI_001.md` — command surface, typed errors, exit codes
- `contracts/CONTRACT_OVERRIDE_001.md` — approval is not override
- `orchestration/03_IMPLEMENTATION_DEPENDENCY_GRAPH.md`, `08_DEPENDENCY_REQUESTS.md`, `14_AGENT_HANDOFF_PROTOCOL.md`

## REFERENCE

- `contracts/CONTRACT_CORE_002.md`, `CONTRACT_POLICY_001.md`, `CONTRACT_POLICY_002.md` — how your receipts become claim status and admission
- `contracts/CONTRACT_PLUGIN_002.md` — how verification results surface at the gate
- `contracts/CONTRACT_EXPORT_001.md`
- `orchestration/00`, `02`, `06`, `07`, `09`, `13`, `15`

## EXCLUDED / FOREIGN OWNERSHIP

- `contracts/CONTRACT_REVIEW_001.md`, `CONTRACT_REVIEW_002.md`, `CONTRACT_REVIEW_003.md`, `CONTRACT_CONFIG_003.md` — probabilistic review is a different evidence family and a different manager
- `contracts/CONTRACT_CONFIG_002.md`, `CONTRACT_PLUGIN_001.md`
- All foreign source trees and schemas in `OWNERSHIP_MANIFEST.md`

Visible, never yours. Do not modify, do not redefine, do not assume ownership.

## Contract classification — all 21 frozen contracts

| Contract | Subject | Owner | Your class |
|---|---|---|---|
| `CONTRACT_CORE_001.md` | CodeStateFingerprint | A2-FOUNDATION | CONSUMED |
| `CONTRACT_CORE_002.md` | Task + TaskState | A2-FOUNDATION | REFERENCE |
| `CONTRACT_CORE_003.md` | Claim + ClaimStatus | A2-FOUNDATION | CONSUMED |
| `CONTRACT_EVIDENCE_001.md` | Evidence families + Evidence | A2-FOUNDATION | CONSUMED |
| `CONTRACT_POLICY_001.md` | VerificationPolicy | A2-FOUNDATION | REFERENCE |
| `CONTRACT_POLICY_002.md` | Admission + AdmissionDecision | A2-FOUNDATION | REFERENCE |
| `CONTRACT_CONFIG_002.md` | .receipts/policy.yaml | A2-FOUNDATION | EXCLUDED |
| `CONTRACT_LEDGER_001.md` | LedgerEvent | A2-FOUNDATION | CONSUMED |
| `CONTRACT_LEDGER_002.md` | Append-only event / projection invariants | A2-FOUNDATION | CONSUMED |
| `CONTRACT_EXPORT_001.md` | Portable ledger export | A2-FOUNDATION | REFERENCE |
| `CONTRACT_RUNNER_001.md` | VerificationRecipe | A2-VERIFICATION | **OWNED** |
| `CONTRACT_RUNNER_002.md` | ExecutionReceipt | A2-VERIFICATION | **OWNED** |
| `CONTRACT_CONFIG_001.md` | .receipts/recipes.yaml | A2-VERIFICATION | **OWNED** |
| `CONTRACT_PLUGIN_001.md` | Normalized hook -> broker request | A2-CLAUDE-INTEGRATION | EXCLUDED |
| `CONTRACT_PLUGIN_002.md` | Broker -> hook decision / error response | A2-CLAUDE-INTEGRATION | REFERENCE |
| `CONTRACT_CLI_001.md` | receipts CLI semantics + exit codes | A2-CLAUDE-INTEGRATION | CONSUMED |
| `CONTRACT_REVIEW_001.md` | ReviewRequest | A2-TRUST | EXCLUDED |
| `CONTRACT_REVIEW_002.md` | ReviewResult + ReviewFinding | A2-TRUST | EXCLUDED |
| `CONTRACT_REVIEW_003.md` | ReviewProvider | A2-TRUST | EXCLUDED |
| `CONTRACT_CONFIG_003.md` | .receipts/providers.yaml | A2-TRUST | EXCLUDED |
| `CONTRACT_OVERRIDE_001.md` | Override / Waiver semantics | A2-TRUST | CONSUMED |

All 21 are **1.0.0 FROZEN** at `CONTRACT_FREEZE_SHA`. Every contract has exactly one owner and no contract is multiply owned — verified across all five managers.

## Context discipline for delegation

When you issue an A3 task, you pass down a **subset** of this manifest, not the manifest itself: one atomic task, the architecture sections it needs, the frozen contracts it consumes, the source files it touches. An A3 that receives your whole context will produce work sized to your whole context.

Everything an A3 or A4 needs must be reachable from repository artifacts at a named SHA. No downstream agent may be expected to rely on a prior conversation, and no context class in this manifest is satisfied by recollection.
