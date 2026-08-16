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

# REMOTE_BRANCH_POLICY

## Modes (§64)

| Policy | Pushes | Use |
|---|---|---|
| `LOCAL_ONLY` | nothing | Experimentation, private exploration |
| **`PUSH_A2_BRANCHES`** | workstream branches only | **Recommended default** |
| `PUSH_ACCEPTED_A3` | accepted task branches + workstream | Teams wanting task-level visibility |
| `PUSH_ALL_CHECKPOINTS` | everything, including rejected attempts | Debugging, audit-heavy environments |

## Why `PUSH_A2_BRANCHES` by default

It gives durability and collaborator visibility at the workstream level without polluting the remote with every transient repair attempt. A goal with three workstreams and forty attempts produces three remote branches instead of forty-plus.

`PUSH_ALL_CHECKPOINTS` is genuinely useful when diagnosing why a model keeps failing — but as a default it makes the remote unusable for humans.

## Rules

- Policy is set per project by A1/user configuration, never chosen by a worker.
- Force-push is prohibited on every branch under every policy. History is evidence.
- `main` is pushed only through the A1 integration gate.
- A rejected attempt is never deleted from the remote once pushed.
- Push failure never blocks local progress; it is recorded and retried.

## Interaction with review

A4 needs the exact commit. Under `LOCAL_ONLY` and `PUSH_A2_BRANCHES`, the reviewer runs locally against the local repository. Under remote-heavy policies it may fetch. **Review never depends on a push having succeeded** — otherwise a network failure would silently degrade the assurance chain.

## Credentials

Push uses the user's existing git credentials. The orchestrator never manages git remotes' authentication and never stores a token for it.
