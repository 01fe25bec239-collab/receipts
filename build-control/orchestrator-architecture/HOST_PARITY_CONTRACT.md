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

# HOST_PARITY_CONTRACT

## The invariant

> Claude Code and Codex are **both first-class product hosts**. A developer on either host has equivalent access to every orchestration capability.

Rejected architecture: *Claude = real host, Codex = secondary client.* If any capability below works on one host and not the other, the parity suite fails and the release is blocked.

## Parity is behavioural, not mechanical

**[HISTORICAL] V1.3.1 correction note.** The 2026-08-13 observation about Codex lifecycle hooks is superseded and now false — registry claim `C-02` (self-fetched, 2026-08-15) confirms native Codex lifecycle hooks and plugin-bundled hook configuration.

Parity is nonetheless still defined behaviourally rather than mechanically, for a better reason than capability asymmetry: hook coverage differs by host version, plugin hooks require user trust before running (`C-02a`), and hooks can be disabled entirely (`C-02b`). A parity contract anchored to any host's mechanism would break the first time that mechanism changed or was switched off.

So parity is defined as an **observable capability contract**. Both adapters must satisfy it; how they satisfy it is theirs to decide.

## The parity capability set

Each row is independently testable on both hosts.

| # | Capability | Observable requirement |
|---|---|---|
| P-01 | `START_GOAL` | User can submit a goal or spec and receive a goal ID |
| P-02 | RUNTIME-A1 available | Goal produces a durable A1 logical role |
| P-03 | Workstream creation | RUNTIME-A2 roles are created and listed |
| P-04 | DAG visibility | User can inspect the task graph and dependency state |
| P-05 | Routing transparency | User can inspect the routing decision for any dispatch |
| P-06 | A3 implementation | Tasks execute in isolated workspaces |
| P-07 | Automatic A4 audit | Every completed A3 triggers a fresh audit without user action |
| P-08 | Repair loop | Rejection produces a bounded automatic repair cycle |
| P-09 | Model intelligence | Current candidate models are discoverable and inspectable |
| P-10 | Quota failover | Rate limits cause failover, not task loss |
| P-11 | Context rehydration | Manager context can be reloaded from authoritative sources |
| P-12 | Worktrees | Isolated workspaces are created, listed, and cleaned |
| P-13 | Checkpoint recovery | A crashed attempt is recoverable |
| P-14 | Exact-SHA review | Every review names the exact SHA audited |
| P-15 | Integration | Accepted work moves through the integration gate |
| P-16 | Status | Current project state is reportable |
| P-17 | Global goal evaluation | Completion is evaluated against the original spec |
| P-18 | Cross-host resume | A goal started on either host resumes on the other |
| P-19 | Graph visibility | `SHOW_GRAPH` renders the same graph and version on both hosts |
| P-20 | Capability catalog | `SHOW_CAPABILITIES` shows identical capability IDs and statuses |
| P-21 | Entitlement status | Same entitlement state resolved on both hosts on one machine |
| P-22 | Product login | Pro activation available from either host; activating once suffices |
| P-23 | Feature admission | A Pro capability invoked on either host yields the same admission outcome |
| P-24 | Admission bypass blocked | A host-specific direct call cannot bypass core admission |
| P-25 | Provider status | Auth, policy eligibility and availability shown as separate axes |

## Permitted asymmetry

| Dimension | Claude Code | Codex |
|---|---|---|
| Installation | Plugin (`.claude-plugin/`) | Native plugin (`.codex-plugin/`); config + companion process (fallback) |
| Event capture | Native hooks | Native hooks (fallback: process supervision + `codex exec` JSONL) |
| Command surface | Slash command / skill | Slash-style command or CLI wrapper (Q-01) |
| Sandbox | Native `sandbox.enabled` | `--sandbox` modes |
| In-session integration | Deep (native hooks primary) | Deep (native hooks primary); supervisor-mediated only when discovery selects the SUPERVISED fallback (`HOST_CAPABILITY_DISCOVERY.md`) |

Asymmetry is permitted only where it does not change an observable P-row. Latency and ergonomics may differ; capability may not.

## Conformance suite

A single test suite runs the same scenarios against both adapters through the normalized event interface. It is the release gate for host parity, and it must fail loudly rather than skip a host.

```
parity/
  p01_start_goal.spec        …  p25_provider_status.spec
  drivers/claude_driver.ts       drivers/codex_driver.ts
```

The suite covers every row P-01…P-25 defined above — `PARITY_CAPABILITY_COUNT` is derived from that table, and `PARITY_CONFORMANCE_COVERAGE_MISSING = 0` checks the scaffold names a spec file for each one.

Rules: no test may be marked host-specific; a skipped host counts as a failure; new capabilities land with a parity row or they do not land.

## Ownership

`BUILD-A2-HOST-INTEGRATION` owns this contract, both adapters, and the conformance suite. It may block a release on parity failure. It may not resolve a parity gap by narrowing the capability set without a recorded decision.

## Known risk

If Codex parity for a P-row proves impossible without an invasive wrapper, that is a **pivot signal**, recorded in `FAILURE_CRITERIA.md` — not a reason to quietly demote Codex.

## Plan-aware parity (V1.3)

Parity is required **per tier**, not merely overall.

**FREE parity:** installation · graph goal start · graph visibility · status · cache and selective retry · deterministic checks · Free capability catalog · Pro locked-feature visibility · entitlement status.

**PRO parity:** activation · distributed orchestration · routing · A3/A4 · repair · provider status · cross-host resume · advanced provenance · provider-policy explanation.

Different UX mechanics are permitted. **Different product capability is not.** A host where a Pro feature silently works while the other refuses it is a parity failure and a licensing defect simultaneously.
