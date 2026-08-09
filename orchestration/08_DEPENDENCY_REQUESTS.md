# 08 — Dependency Requests

| ID | Requester | Provider | Required item | Needed by | Status |
|---|---|---|---|---|---|
| DR-001 | A2-LEDGER | A2-CORE | Final TypeScript representation/serialization appendix for RepositoryIdentity/Fingerprint | Before M0 A3 | OPEN |
| DR-002 | A2-CORE | A2-LEDGER | Event append/projection API shape implementing LEDGER-001/STORAGE-001 | Before M2 A3 | OPEN |
| DR-003 | A2-RUNNER | A2-CORE + A2-LEDGER | Fingerprint read + ledger append interfaces for receipts | Before M1 A3 | OPEN |
| DR-004 | A2-CLAUDE-INTEGRATION | A2-CORE | Stable `admit`/status query facade and ADMISSION-001 rendering data | Before M3 A3 | OPEN |
| DR-005 | A2-CLAUDE-INTEGRATION | A2-INTEGRITY-SECURITY | Exact deny/fail-open/fail-closed security requirements and negative fixtures | Before M3 permission A3 | OPEN |
| DR-006 | A2-REVIEW | A2-CORE + A2-LEDGER | Review persistence/admission consumer interface | Before M4 A3 | OPEN |
| DR-007 | A2-REVIEW | A2-CLAUDE-INTEGRATION | Claude fallback launch environment / hook-recursion constraints | Before Claude fallback A3 | OPEN |
| DR-008 | A2-INTEGRITY-SECURITY | A2-RUNNER | Test-glob and parsed test-count inputs for integrity signals | Before M5 A3 | OPEN |
| DR-009 | A2-INTEGRITY-SECURITY | A2-LEDGER | Export/override ledger representation | Before M5 A3 | OPEN |
| DR-010 | A2-EVALUATION | All product A2s | Stable CLI + fixtures + reset interface | Before M6 runs | OPEN |
| DR-011 | A2-DOCS-RELEASE | A2-EVALUATION | Measured result bundle with provenance; no prose-only claims | Before EVALUATION.md results | OPEN |

A dependency request carries contract references, not informal prose. The providing A2 may reject a request that would violate its ownership boundary and escalate to A1.
