# 11 — Integration Gates

| Gate | When | Pass condition | Authority |
|---|---|---|---|
| IG-0 Architecture | Before any A2 implementation planning | Architecture authority acknowledged; no **unapproved** deviation pending; manager ownership fixed. ADR-001 is APPROVED and reconciled across this package, so IG-0 passes. | A1 |
| IG-1 Contract | Before each A3 prompt | Every consumed contract FROZEN; schema/fixture version attached; blocking OIs cleared. | A1 + owning A2 |
| IG-2 Workspace | Before A3 writes | Bounded task, declared paths, isolated future branch/worktree, no cross-component write authority. | Owning A2 |
| IG-3 Implementation evidence | Before A4 | Diff confined to task paths; tests/fixtures recorded; no architecture file silently changed. | Owning A2 |
| IG-4 Independent review | Before component acceptance | A4 is distinct session; reviews architecture, contract, acceptance criteria, security, tests. | A4 + owning A2 |
| IG-5 Component acceptance | Before A1 integration | A2 resolves/rejects all A4 findings; contract tests pass; evidence complete. | Owning A2 |
| IG-6 Milestone integration | Before next milestone | All contributing components accepted; cross-component integration tests pass; no invariant regression. | A1 |
| IG-7 Evaluation | Before public metric claims | M6 raw runs reproducible; >=3 runs/task/arm; defective vs clean split; no invented significance. | A2-EVALUATION + A1 |
| IG-8 Release | Before tag/package | Docs truthful; name check done; security/release smoke passes; EVALUATION only contains measured outputs. | A2-DOCS-RELEASE + A1 |

## Failure behavior

A failed gate returns the work to the owning A2 with a concrete unmet-evidence list. A1 does not waive architecture invariants to keep sequence moving. Any requested invariant change is an architecture deviation, not a normal bug fix.
