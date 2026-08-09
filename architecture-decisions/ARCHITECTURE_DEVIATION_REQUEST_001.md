# ARCHITECTURE_DEVIATION_REQUEST-001 — WorktreeCreate Observation Conflict

**ID:** ADR-001  
**Status:** **APPROVED**  
**Raised by:** A1-RECEIPTS  
**Raised during:** contract freeze, after the A1 build-control bootstrap package was produced  
**Date raised:** 2026-08-09  
**Date approved:** 2026-08-09  
**Architecture authority approval:** APPROVED by the architecture authority; preferred minimal correction adopted  
**Disposition:** retained permanently as historical architecture evidence; not deleted on resolution

## Architecture section affected

`Receipts_Final_Architecture` §O hook mapping and §T exact MVP, specifically `WorktreeCreate` (and, by the approved verification instruction, `WorktreeRemove`).

## Documented assumption / design

The architecture maps `WorktreeCreate` to workspace identity binding and states that the handler should be trivial, catch-all, and always exit 0 so a broker bug does not break worktree creation. It treats Receipts as observing/binding a workspace, not creating worktrees.

## Current verified reality

Current official Claude Code Hooks documentation states that configuring a `WorktreeCreate` hook **replaces Claude Code's default Git worktree creation**. The configured hook must create the working copy and return its path on stdout; a missing path or a hook failure causes creation to fail. `WorktreeCreate` is additionally the one event where **any** non-zero exit code aborts the action, rather than only exit code 2.

## Exact implementation impact

Installing the architecture's observational handler as written would suppress Claude Code's built-in Git worktree creation. A no-op always-exit-0 handler cannot satisfy the current hook contract because a created worktree and its path are required.

Taking over Git worktree creation inside Receipts would expand product responsibility into workspace creation and contradict the architecture's scope and non-orchestration intent (invariants 12 and 14).

## Approved correction

**Adopted: the preferred minimal correction.**

1. Receipts **MUST NOT** install a `WorktreeCreate` hook in MVP.
2. Receipts **does not own worktree creation**. Claude Code and Git remain responsible for normal Git worktree creation.
3. Receipts **MUST NOT** replace Claude Code's default Git worktree behavior, and **MUST NOT** implement custom worktree creation.
4. Workspace identity is bound **observationally**, from:
   - `SessionStart` and the current `cwd`;
   - repository identity (`CONTRACT-CORE-001`);
   - read-only Git worktree metadata discovered by the broker;
   - normal broker invocations from the active working directory.
5. `WorktreeRemove` is **also omitted** from the MVP installed hook set (see the verification section below). Workspace cleanup remains Claude Code's / Git's responsibility, and workspace-binding invalidation is lazy at the next session start, consistent with existing architecture philosophy.
6. No hook is retained for symmetry.

## Reason

Receipts is an evidence and admission layer, not a workspace orchestrator. The only mechanism current Claude Code offers for `WorktreeCreate` is total replacement of default behavior. Accepting that mechanism would either break worktree creation (no-op handler) or make Receipts responsible for creating workspaces (full handler). Both outcomes are worse than removing the hook, because workspace identity is fully recoverable observationally from `cwd`, repository identity, and read-only Git metadata, none of which require a hook.

## WorktreeRemove — current-documentation verification (2026-08-09)

Re-checked against the current official Claude Code hooks reference before finalizing the plugin contracts.

Findings:

- `WorktreeRemove` fires when a worktree is being removed at session exit, when a subagent finishes, or when a background session is deleted.
- `WorktreeRemove` has **no decision control**. The current reference lists it among the events used only for side effects such as logging or cleanup.
- `WorktreeRemove` **cannot block**; its failures are logged in debug mode only.
- The current reference annotates **only** `WorktreeCreate` as replacing default Git behavior. It does not state that `WorktreeRemove` requires a paired custom `WorktreeCreate`.

Unresolved conflict, recorded honestly:

- Multiple independent third-party Claude Code worktree integrations implement `WorktreeRemove` handlers that perform the `git worktree remove` themselves, and describe both worktree hooks as replacing default Git behavior. If that behavior is real, a purely observational Receipts `WorktreeRemove` handler would suppress default cleanup and leak worktrees.
- This conflict cannot be settled from primary documentation alone. It requires a local version smoke test against an installed Claude Code build, which is out of scope for contract freeze.

Decision:

Under the minimality rule, Receipts does not install a hook whose worst case is silently taking over workspace lifecycle in exchange for a benefit Receipts does not need. No MVP requirement depends on early removal notification; stale workspace bindings are invalidated lazily at the next `SessionStart`. `WorktreeRemove` is therefore **omitted from the MVP installed hook set** — omitted on its own merits, not for symmetry with `WorktreeCreate`.

`OI-009` tracks post-MVP re-verification by local smoke test. Re-introducing either hook is a contract change, not an implementation detail.

## Alternative rejected

Have Receipts implement Git worktree creation and return the path. Rejected because it unnecessarily makes Receipts responsible for workspace orchestration and enlarges the failure and security scope, contradicting invariants 12 and 14.

## Invariants affected

**None.** The correction changes only the workspace-observation hook mapping.

Explicitly unchanged: `CLAIM → EVIDENCE → POLICY → ADMISSION`; `CodeStateFingerprint`; evidence authority; staleness; `VerificationRecipe`; `ExecutionReceipt`; `ReviewProvider`; the L1 `TaskCompleted` gate; the L2 `PreToolUse` gate; the CLI → SQLite broker topology; the security/trust model; and the evaluation architecture.

## Affected hook mapping

| Hook event | Before ADR-001 | After ADR-001 (approved) |
|---|---|---|
| `WorktreeCreate` | Observational workspace identity binding; trivial handler always exits 0 | **Not installed in MVP.** Absent from `hooks.json`; absent from the `HookEvent` union. |
| `WorktreeRemove` | Observation / cleanup notification | **Not installed in MVP.** Cleanup remains Claude Code / Git responsibility. |
| `SessionStart` | Session context and provenance | Unchanged transport, plus the observational workspace-binding role previously assigned to `WorktreeCreate`. |

Broker-side workspace discovery is read-only Git metadata inspection from the invocation `cwd`. It is not a hook and creates no new authority path.

## Affected contracts

| Contract | Impact | Resulting state |
|---|---|---|
| `CONTRACT-PLUGIN-001` (HookInputNormalization) | `WorktreeCreate` / `WorktreeRemove` removed from the MVP `HookEvent` union and payload set; `SessionStart` clause extended with observational workspace binding | **1.0.0 FROZEN** |
| `CONTRACT-PLUGIN-002` (HookDecision / CLI exit codes) | `WorktreeCreate` "not frozen" clause removed; `WorktreeRemove` mapping removed | **1.0.0 FROZEN** |
| All other contracts | No change | Remain 1.0.0 FROZEN |

`M0`–`M2` implementation was never blocked by this issue. `M3` Claude integration proceeds with no worktree hook mapping.

## Migration / backward-compatibility impact

None. No implementation exists; no repository, branch, worktree, or source file was created before this correction. Nothing has to be migrated.

## Security / trust impact

Net reduction in surface area. Receipts installs one fewer class of hook, does not gain workspace-creation authority, and adds no new write path. Read-only Git worktree metadata discovery is already within the broker's existing repository-inspection authority under `CONTRACT-PROCESS-001`.

## Evaluation impact

None. No benchmark task, oracle, arm, or metric depends on worktree hook behavior.

## A1 recommendation

Adopt the preferred minimal correction; do not implement custom worktree creation; omit both worktree hooks from the MVP installed set.

## Approval

**APPROVED** by the architecture authority on 2026-08-09. The preferred minimal correction was adopted without modification, with the additional instruction to re-verify `WorktreeRemove` against current official documentation and to omit it rather than retain it for symmetry. That verification was performed and is recorded above.

## Primary source

- Claude Code Hooks reference — `https://code.claude.com/docs/en/hooks` — accessed 2026-08-09.
  - `WorktreeCreate`: fires when a worktree is created via `--worktree`, `isolation: "worktree"`, or for a background session; replaces default Git behavior; decision pattern is a path return; hook failure or missing path fails creation; any non-zero exit code aborts creation.
  - `WorktreeRemove`: fires when a worktree is removed at session exit, when a subagent finishes, or when a background session is deleted; no decision control; cannot block; failures are logged in debug mode only.
