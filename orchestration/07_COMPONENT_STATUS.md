# 07 — Component Status

| Component | Manager state | Implementation state | Authority input | Current blocker |
|---|---|---|---|---|
| A2-CORE | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | OI dependencies only; core semantics frozen |
| A2-LEDGER | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | OI-001/OI-002 before A3 |
| A2-RUNNER | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | OI-005 before approval-path A3 |
| A2-CLAUDE-INTEGRATION | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture; ADR-001 APPROVED | OI-004 before permission A3; ADR-001 no longer blocks (PLUGIN-001/002 are 1.0.0 FROZEN) |
| A2-REVIEW | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | OI-003 before Claude fallback A3 |
| A2-INTEGRITY-SECURITY | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | Security sign-offs on OI-003/004/005 |
| A2-EVALUATION | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | Wait for M5 before runs |
| A2-DOCS-RELEASE | READY_FOR_INITIALIZATION | No implementation authorized | A1 package + architecture | OI-006 before release |

## Global state

- Architecture: FROZEN, with one approved correction of record (ADR-001).
- Cross-component semantic contracts: FROZEN at A1 level.
- Contract freeze: READY / FROZEN. All contracts including CONTRACT-PLUGIN-001 and CONTRACT-PLUGIN-002 are at 1.0.0 FROZEN.
- Architecture deviations: one on record (ADR-001), APPROVED and reconciled; none pending.
- Repository: NOT CREATED by this phase.
- Branches/worktrees: NOT CREATED by this phase.
- Dependencies: NOT INSTALLED.
- A3 coding: BLOCKED.
- A4 review: NOT STARTED.
- A2 initialization: AUTHORIZED.
