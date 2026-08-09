# 14 — Agent Handoff Protocol

## Hierarchy

```text
A1 global orchestrator
   ↓ component authority + frozen contracts
A2 long-lived component manager
   ↓ one bounded implementation task
A3 implementation agent
   ↓ exact diff + implementation evidence
A4 independent review agent
   ↓ structured findings / approval
A2 accepts or returns
   ↓ accepted component artifact
A1 integration gate
```

## A1 → A2 initialization packet

Must contain:
- authoritative architecture and this control package;
- A2 component ownership/exclusions;
- contracts owned/consumed;
- milestone responsibilities;
- open issues and dependency requests;
- explicit instruction: do not issue A3 until required contracts and blocking issues are cleared;
- freshness rule for mutable external interfaces;
- architecture deviation protocol.

## A2 → A3 task packet

One task only. Must contain:
- task ID and milestone;
- exact files/paths allowed;
- frozen contracts and versions;
- required behavior and acceptance tests;
- prohibited behavior/scope;
- dependencies already satisfied;
- expected evidence to return;
- future branch/worktree identity when repository bootstrap exists.

An A3 may implement but may not change architecture, cross-component contracts, another A2's files, evaluation oracles, or release claims.

## A3 → A4 handoff

Must include:
- task ID;
- baseline and reviewed commit/diff;
- contract list;
- acceptance criteria;
- tests executed and results;
- unresolved limitations;
- security-sensitive surfaces changed.

A4 must be a different agent/session and must not modify the code under review. It returns structured findings and a verdict to A2.

## A4 → A2 outcome

- `ACCEPT`: no blocking finding; evidence sufficient.
- `REVISE`: blocking findings; A2 sends only bounded fixes back to an A3 task.
- `ESCALATE`: architecture/contract conflict; A2 sends to A1, not to A3 for improvisation.

## A2 ↔ A2 dependency protocol

No direct cross-component implementation. Use `08_DEPENDENCY_REQUESTS.md` with:
- requester/provider;
- contract;
- exact required artifact;
- deadline/milestone;
- reason it cannot be solved inside requester ownership.

## Future workspace convention

When repository bootstrap is separately authorized:
- A2 integration branch: `a2/<component>`
- A3 task branch: `a3/<component>/<task-id>`
- one worktree per active implementation task
- A4 reviews the immutable A3 commit/diff; A4 does not share A3's mutable working tree

These are workspace-isolation conventions only. They are not a security boundary.

## Current bootstrap restriction

This A1 phase **does not create** the repository, branches, worktrees, commits, PRs, or source files.
