# Protected launch for the concrete adapters

`ClaudeAdapter` and `CodexAdapter` run either ordinary or Workspace-protected
attempts through the same `RuntimeAdapter` start/events/collect/cancel path,
directly or through `registry::DynAdapter`.

## Requesting protection

Only trusted composition holding an admitted launcher can ask for it:

```rust
let launcher = ProtectedLauncher::new(config)?; // Workspace platform admission
let policy = RuntimeExecutionPolicy::new(timeout, read_only)
    .with_protected_launch(ProtectedLaunch::new(launcher, repository_root));
let handle = adapter.start(&capsule, &workspace, &policy)?;
```

Protection is never inferred from Claude's permission mode or Codex's
`--sandbox` value. A protected policy calls Workspace `admit` and `launch`
with the exact request the ordinary path would run (same argv, stdin bytes and
environment). Every refusal returns an error; it never falls back to the
ordinary runner. Refusals before any worker exists are `POLICY_BLOCKED`.
Unsupported platforms refuse in `ProtectedLauncher::new`, so no protected
policy can be built there.

Before launch the adapter also requires the capsule's task, worktree, branch
and start revision to match the workspace handle. Workspace then binds the
admission to that handle, its repository and the launch request's root.

## Ownership

Workspace alone admits custody, creates the worker, applies the deadline,
collects, cancels, kills and proves the cgroup empty, and releases custody.
Runtime adds no timeout loop, signals, PIDs or privilege.

- `handle.protected()` / `result.protected()` label the attempt. Ordinary
  attempts are non-authoritative: no custody, no controller channel.
- `handle.controller_channel()` gives the trusted owning caller the typed
  channel: the query-only reviewer channel for read-only policies, the worker
  channel otherwise. Its token never enters the prompt, child environment,
  events or diagnostics. This in-process accessor is not a worker transport;
  worker-facing IPC remains Host-owned later work.
- `handle.release_custody()` releases after collection. Workspace refuses
  until collection proved cleanup, and a refusal keeps custody.
- Dropping a handle cancels, joins collection, then asks Workspace to release.
  Unproven cleanup stays quarantined; nothing becomes reusable silently.

Plan mode and read-only sandbox selection remain reduced isolation:
protection guards Git metadata custody and the worker identity, not reviewer
payload writes.

## Native Linux diagnostic

`protected_launch_tests::hosted_*` are `#[ignore]`d and run only in
`.github/workflows/ra-w1-completion-linux.yml` on a disposable GitHub-hosted
Ubuntu 24.04 x64 VM, through `.github/scripts/ra-w1-completion-linux/run.sh`.
That script is the reviewed Workspace diagnostic with only the binary, test
list, cgroup, fixture glob and environment name changed, and it sources the
unchanged Workspace `custody.sh`. The tests cover, for both adapters:
collection, async event wakeup, ordinary-versus-protected stdin/argv/env
equality, default-empty and explicit environments, worker identity, controller
identity/status/stage, replay/forged/cross-task/reviewer refusals, refused
early release, release and post-release refusal, accepted cancellation,
uncollected Drop, and refused admissions that never start a worker.

The worker is a synthetic shell CLI. A pass is Runtime integration evidence on
that image only, not provider, other-platform or whole-system assurance.
Publication and execution of this workflow need A1 disposition and
independent review first.
