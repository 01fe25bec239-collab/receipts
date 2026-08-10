<!--
Receipts — A2 Bootstrap Handoff Template (V2)
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
Install path: build-control/a2/A2_BOOTSTRAP_HANDOFF_TEMPLATE.md
-->

# A2_BOOTSTRAP_HANDOFF_TEMPLATE

**Purpose.** This is the only sanctioned way to initialize an A2 component manager. The currently active A1 completes one copy per manager and issues it alongside that manager's folder. A manager that receives anything less is not initializable.

**Why it exists.** An A2 manager must be able to start with no access to any prior conversation, under any capable runtime, at any point in the project's life. Every fact it needs to establish where it is, what it owns, and what it may do must arrive either from a committed repository artifact or from this handoff. Nothing else counts as authority.

## Template

Copy verbatim, fill every field, resolve every placeholder.

```yaml
# ---------- A2 BOOTSTRAP HANDOFF ----------
project:                       Receipts

repository:                    01fe25bec239-collab/receipts
remote_name:                   origin
remote_url:                    https://github.com/01fe25bec239-collab/receipts.git

active_a1_id:                  <A1_BOOTSTRAP | A1_RUNTIME>

manager_id:                    <A2-FOUNDATION | A2-VERIFICATION | A2-CLAUDE-INTEGRATION | A2-TRUST | A2-QUALITY-RELEASE>

manager_branch:                <a2/foundation | a2/verification | a2/claude-integration | a2/trust | a2/quality-release>
manager_worktree_path:         <MANAGER_WORKTREE_PATH_NOT_YET_ASSIGNED>

contract_freeze_sha:           2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221
agent_system_freeze_sha:       <AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>
a2_start_sha:                  <A2_START_SHA_NOT_YET_ASSIGNED>

working_tree_expected_clean:   true
a3_implementation_authorized:  false

issued_by:                     <A1_BOOTSTRAP | A1_RUNTIME>
issued_at:                     <ISO-8601 UTC timestamp>
# ---------- END HANDOFF ----------
```

## Field reference

| Field | Meaning | Rules |
|---|---|---|
| `project` | Always `Receipts`. | Fixed. |
| `repository` | `owner/name` on the remote host. | Must match `git remote get-url`. |
| `remote_name` / `remote_url` | Expected remote identity. | Verified at initialization; mismatch is a hard stop. |
| `active_a1_id` | Which A1 currently holds authority. | Exactly one A1 is ever active. `A1_BOOTSTRAP` during planning and freezing; `A1_RUNTIME` after formal authority transfer. Never both. |
| `manager_id` | One of the five approved managers. | No sixth manager exists. |
| `manager_branch` | The A2 **integration** branch. | Convention `a2/<slug>`. Created or validated by the active A1, not by the manager. |
| `manager_worktree_path` | Absolute path of the A2 **integration** worktree. | **Never fabricated.** Placeholder until the active A1 provisions it. |
| `contract_freeze_sha` | Immutable semantic baseline. | Known and fixed. Must be an **ancestor of HEAD** at initialization. |
| `agent_system_freeze_sha` | `main` commit holding the complete frozen agent operating system. | **Not yet assigned.** Supplied by the human operator after the final planning freeze. Must be an **ancestor of or equal to** `a2_start_sha`. |
| `a2_start_sha` | The accepted `main` commit this manager starts from. | Must equal HEAD at initialization. Expected to equal `agent_system_freeze_sha` at initial startup, but **not permanently coupled to it** — a later initialization or re-initialization may legitimately start from a newer accepted `main` commit. |
| `working_tree_expected_clean` | Always `true`. | A dirty tree at initialization is a hard stop. |
| `a3_implementation_authorized` | Whether the manager's implementation wave is open. | **Defaults to `false`.** Set to `true` only by an explicit wave authorization from the active A1. Holding a verified integration worktree does not imply this. |
| `issued_by` / `issued_at` | Provenance of the handoff. | Must match `active_a1_id` and be a real timestamp. |

## Placeholder rules

- `<AGENT_SYSTEM_FREEZE_SHA_NOT_YET_ASSIGNED>` — the agent-system freeze commit does not exist yet. It will be created after all remaining orchestration work is committed and pushed to `main`, and its value is supplied by the human operator.
- `<A2_START_SHA_NOT_YET_ASSIGNED>` — assigned by the active A1 at the moment of initialization.
- `<MANAGER_WORKTREE_PATH_NOT_YET_ASSIGNED>` — assigned when the active A1 provisions the integration worktree.

**No agent may invent, guess, derive, or substitute any of these values.** A handoff carrying an unresolved placeholder in a required field is not a valid handoff, and the receiving manager must refuse to initialize and report.

## Verification performed by the receiving manager

```
git rev-parse HEAD                              # == a2_start_sha
git status --porcelain                          # empty
git remote get-url <remote_name>                # == remote_url
git merge-base --is-ancestor <contract_freeze_sha> HEAD
git merge-base --is-ancestor <agent_system_freeze_sha> <a2_start_sha>
git rev-parse --abbrev-ref HEAD                 # == manager_branch
git rev-parse --show-toplevel                   # == manager_worktree_path
```

Plus a preceding check that the handoff itself is complete, giving eight steps in total: (1) handoff complete with no unresolved placeholder; (2) HEAD == `a2_start_sha`; (3) working tree clean; (4) remote matches; (5) `CONTRACT_FREEZE_SHA` is an ancestor of HEAD; (6) `AGENT_SYSTEM_FREEZE_SHA` is an ancestor of or equal to `A2_START_SHA`; (7) branch == `manager_branch`; (8) worktree path == `manager_worktree_path`.

All must pass. **If any check fails: STOP and report to the currently active A1.** Do not repair, re-clone, reset, re-checkout, or proceed on a best-effort basis.

`CONTRACT_FREEZE_SHA` is a historical semantic baseline. It is **never** expected to equal HEAD, and no check compares the two for equality.

## Worktree authorization semantics

| | A2 integration worktree | A3 implementation worktree |
|---|---|---|
| Lifetime | Long-lived, spans the manager's tenure | Short-lived, one bounded task |
| Branch | `a2/<slug>` | `a3/<slug>/<task-id>` |
| Provisioned by | The currently active A1 | The manager, **after** wave authorization |
| Created from | `a2_start_sha` | The manager's integration branch head |
| Implies implementation authority | **No** | Yes, for that task only |

These are ordinary **development-process** Git worktrees used by the operator and the agent system. Under ADR-001, the **Receipts product** installs no `WorktreeCreate` and no `WorktreeRemove` hook and does not own worktree creation. Development-process worktrees and product runtime behavior must never be conflated in any artifact.

Worktrees are workspace isolation, never security isolation (invariant 12).

## Current state of this template

| Item | State |
|---|---|
| `CONTRACT_FREEZE_SHA` | Known: `2d2dbc2cc0ff1a320d8d0e0c22eefe71a66ee221` |
| `AGENT_SYSTEM_FREEZE_SHA` | **NOT YET ASSIGNED** |
| `A2_START_SHA` | **NOT YET ASSIGNED** |
| `manager_worktree_path` (×5) | **NOT YET ASSIGNED** |
| Active A1 | `A1-BOOTSTRAP` |
| `A1-RUNTIME` | **NOT YET INITIALIZED** |
| Handoffs issued | **NONE** |
