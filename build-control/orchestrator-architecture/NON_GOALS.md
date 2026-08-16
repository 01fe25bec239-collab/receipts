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

# NON_GOALS

Each entry states what the product is not, and *why the boundary exists* — a non-goal without a reason gets quietly violated later.

| Not this | Why the boundary matters |
|---|---|
| A test runner | Running tests is a capability the workers already have. Value is in binding results to code state and to an integration decision. |
| A claim checker | The old product's thesis. Abandoned: the system must be valuable when workers are competent. |
| A code-review wrapper | Single-shot review is commoditised. The product is the *loop* — audit, repair, re-audit, integrate — not the review call. |
| A generic model proxy / API aggregator | Proxies route tokens. This routes *work* against capability evidence and expected cost to an accepted result. |
| A worktree UI / dashboard for parallel shells | Commoditised (Claude Squad, Conductor, Crystal, vibe-kanban, amux). Building this is building a solved thing. |
| A generic conversational multi-agent framework | Agents here are bound to repositories, workspaces, SHAs, and an integration gate — not to a conversation graph. |
| A single-provider agent team | Vendor-native teams exist (Claude Code Agent Teams, Kimi Agent Swarm, Grok subagents). Cross-provider is the point. |
| A system that blindly chooses the cheapest model | Optimising per-call price maximises repair cost. The objective is expected cost to an *accepted* result. |
| A system that assumes Anthropic/OpenAI are permanently superior | Capability ranking is empirical and volatile. Hard-coding vendor superiority guarantees the architecture ages badly. |
| A system that trusts model-capability knowledge from A1/A2 training data | A model released after the executor's cutoff must be usable without an architecture change. Training memory is never sole authority. |
| An uncontrolled recursive swarm | A3 cannot spawn A3. Discovered work becomes a `SUBTASK_REQUEST` to A2. The authoritative DAG stays centrally controlled. |
| A safety-policy bypass router | Provider-shopping to evade a safety refusal is prohibited. Provider nationality is never a routing heuristic. |
| A proof that generated code is correct | The system binds evidence to code state. It does not prove correctness, and no output may imply otherwise. |

## Adjacent non-goals worth stating

- **Not a hosted service in MVP.** The orchestrator runs locally against the developer's own provider connections. `PRODUCT_TEAM_MODE` is designed for, not built first.
- **Not a replacement for CI.** Integration gating here governs entry into a workstream or integration branch; project CI remains the user's.
- **Not an IDE.** The hosts are Claude Code and Codex. There is no editor surface to own.
- **Not a benchmark authority.** Local calibration informs routing; it is not published as a model leaderboard and small samples are never reported as significant.
