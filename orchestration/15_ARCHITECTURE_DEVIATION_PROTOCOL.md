# 15 — Architecture Deviation Protocol

## Trigger

Create an `ARCHITECTURE_DEVIATION_REQUEST` only when:
1. a binding architecture requirement is affected;
2. a current verified external capability or an unavoidable implementation fact materially prevents the documented requirement; and
3. the problem cannot be solved as an implementation detail while preserving semantics.

Do not use this process for library preference, code style, module naming, or ordinary defects.

## Required request

```text
ARCHITECTURE_DEVIATION_REQUEST

ID:
Date:
Requester:
Architecture section affected:
Documented assumption / decision:
Current verified reality:
Primary source:
Access date:
Local reproduction / version:
Exact implementation impact:
Why normal contract implementation cannot preserve the architecture:
Minimal proposed correction:
Invariants affected:
Alternatives rejected:
Migration/backward-compatibility impact:
Security/trust impact:
Evaluation impact:
A1 recommendation:
Approval:
```

## State machine

`PROPOSED → VERIFIED → A1_REVIEW → APPROVED | REJECTED`

Until APPROVED:
- architecture remains unchanged;
- affected A3 task is BLOCKED;
- no manager may silently implement the proposed redesign;
- dependent components are notified through a dependency request.

## Minimality rule

An approved deviation changes only what current reality makes impossible. It does not reopen unrelated architecture decisions.

## Deviation register

| ID | Title | Raised | State | Outcome |
|---|---|---|---|---|
| ADR-001 | WorktreeCreate observation conflict | Contract freeze, 9 August 2026 | **APPROVED** 9 August 2026 | Preferred minimal correction adopted. |

### ADR-001 summary

No deviation request existed at A1 bootstrap on 9 August 2026. `ADR-001` was raised later the same day, during contract freeze, when the plugin hook contracts were written against current official Claude Code documentation and it emerged that configuring a `WorktreeCreate` hook **replaces** Claude Code's default Git worktree creation rather than observing it.

The architecture authority **approved** the preferred minimal correction:

1. Receipts does not install a `WorktreeCreate` hook in MVP.
2. Receipts does not own worktree creation; Claude Code and Git remain responsible.
3. Receipts does not replace Claude Code's default Git worktree behavior and implements no custom worktree creation.
4. Workspace identity is bound observationally from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations from the active working directory.
5. `WorktreeRemove` was re-verified against current official documentation and is also omitted from the MVP installed hook set, on its own merits rather than for symmetry. Workspace cleanup remains Claude Code's / Git's responsibility.
6. No other architecture semantics changed.

Affected contracts: `CONTRACT-PLUGIN-001` and `CONTRACT-PLUGIN-002`, both now **1.0.0 FROZEN**. Affected build-control statements were reconciled across this package on the same date. The full record, including the one unresolved third-party conflict about `WorktreeRemove`, is `ARCHITECTURE_DEVIATION_REQUEST_001.md` in the contract-freeze package; `OI-009` tracks its post-MVP re-verification.

ADR-001 is retained permanently. A resolved deviation is not deleted.

## Current status

**One deviation on record: ADR-001, APPROVED and reconciled. No deviation is pending.**
