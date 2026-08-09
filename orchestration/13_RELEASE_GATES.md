# 13 — Release Gates

| Gate | Pass condition |
|---|---|
| RG-1 Architecture fidelity | All architecture invariants preserved; no unapproved deviation. |
| RG-2 MVP scope | No daemon/MCP/server/router expansion; deferred features remain deferred unless separately approved. |
| RG-3 Contract completeness | All public/internal cross-component contracts versioned and tested. |
| RG-4 Security/trust | Broker-only evidence writes, protected config, override semantics, read-only reviewer, honest enforcement scope all tested. |
| RG-5 Hook behavior | L1/L2 behavior verified on supported Claude version; installed hook set contains no `WorktreeCreate` and no `WorktreeRemove` entry, so Receipts cannot break or displace Claude Code's default Git worktree creation or cleanup under any broker error (ADR-001). |
| RG-6 Ledger/export | verify-ledger detects mutation; projections rebuild; export independently verifies. |
| RG-7 Evaluation integrity | M6 raw outputs present; report only measured values; clean/defective split; no invalid significance language. |
| RG-8 Documentation truthfulness | README contains exact honest scope sentence and prominent enforcement limitations; receipt proof/non-proof documented. |
| RG-9 Name adoption | Collision check recorded before final product/package naming. |
| RG-10 Demo/install | Fresh installation and exact 3-minute demo flow can be reproduced from documented setup. |

Release is BLOCKED if any overridden task is rendered as verified, if any evaluation number lacks M6 provenance, or if documentation claims enforcement beyond Claude-Code-mediated actions.
