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

# RUNTIME_A3_LIFECYCLE

**RUNTIME-A3 is deliberately disposable.** One bounded task, one fresh session, then destruction.

## Lifecycle

```
CREATE FRESH SESSION      ← new session per attempt, always
      ↓
LOAD TASK CAPSULE         ← the only context it receives
      ↓
VERIFY WORKSPACE          ← branch, worktree, start SHA, clean tree
      ↓
IMPLEMENT                 ← within allowed_write_paths only
      ↓
RUN / CAPTURE CHECKS      ← verification_plan; results recorded verbatim
      ↓
COMMIT / CHECKPOINT       ← periodic checkpoints; final commit is the review anchor
      ↓
HANDOFF                   ← structured A3_HANDOFF
      ↓
TERMINATE SESSION         ← unconditional
```

## Fresh session, always

A fresh session is created for each initial implementation **and each repair attempt**. Resuming a long prior A3 conversation is not the default path.

Three reasons, all load-bearing:

1. **Failover becomes trivial.** If the original provider is unavailable, a different one picks up the Repair Capsule with no conversational dependency (`SESSION_FAILOVER_ARCHITECTURE.md`).
2. **Context stays honest.** A long A3 accumulates its own earlier reasoning, including its mistakes. A repair driven by the reviewer's findings and the current code beats one driven by the author's original mental model.
3. **Cost is bounded.** Session length is capped by task size rather than by project age.

An adapter may support resume; the architecture does not require it (§46).

## Workspace verification (pre-flight)

```
git rev-parse HEAD                # == capsule.start_sha
git status --porcelain            # empty
git rev-parse --abbrev-ref HEAD   # == capsule.branch
git rev-parse --show-toplevel     # == capsule.worktree
```

Any failure: **stop and report**. Never reset, re-clone, or continue on a best-effort basis. An A3 that repairs its own environment is an A3 whose evidence no longer describes a known starting point.

## Write-scope enforcement

`allowed_write_paths` and `forbidden_write_paths` are enforced in **three layers**, because prompt instruction alone is not a boundary:

1. **Capsule instruction** — the agent is told.
2. **Sandbox** — host/runtime sandbox restricts filesystem writes where available (A-07, A-12).
3. **Post-hoc verification** — the diff is checked against the allowed set at handoff. A violation is a blocking finding regardless of merit.

Layer 3 is the one that must never be skipped: per A-08, path permission rules do not constrain subprocesses, so detection at handoff is the reliable check.

## Evidence capture

Recorded for every check executed: worker identity, runtime, model, code SHA, exact command, exit code, result, timing, and output reference. Under LIGHT and STANDARD this is accepted as evidence (§69) — competent workers are not assumed to be lying.

## Handoff labels

`IMPLEMENTED` / `TESTED` / `NOT_TESTED` / `BLOCKED` / `ASSUMED`.

Overstating a label is the most damaging thing an A3 can do, because every downstream gate trusts it. A4 checks labels against reality, and a mismatch is a finding in its own right.

## Stop conditions

Stop and report rather than improvise when: pre-flight fails; the objective is ambiguous or contradicts the architecture; completion would require a forbidden path; an acceptance criterion is impossible as written; a dependency's output is missing or off-contract; or additional work is required beyond scope (→ `SUBTASK_REQUEST`).

Stopping with a clear report is a success. Guessing past a contradiction is the failure.

## Prohibited

Spawning agents; broadening scope silently; editing outside allowed paths; self-approving; merging; claiming unrun checks; weakening or deleting tests to make a build pass.

## Termination

Unconditional after handoff — including on failure, on block, and on crash detection. The session ends; the state persists.
