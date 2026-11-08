# Host probe composition and PREP-SMOKE-1 plan

## What changed

- Host no longer launches any process. `HostProbeProcessPort` (Host-owned) carries only data: the read-only probe argv (`--version`, `plugin list --json`), project directory, deadline and stdout/stderr bounds, plus the composition's executable identity.
- `receipts-cli::WorkspaceHostProbeRunner` is the admitted composition: an absolute path resolved to a canonical regular executable with its SHA-256, an explicit `ChildEnvPolicy`, and Workspace's bounded runner (cleared environment, process-group termination at the deadline, bounded capture). Non-zero exit, timeout, oversize stdout/stderr and cleanup failure are typed errors; partial output is never evidence. The old PATH-resolved `claude` spawn with direct-child-only cleanup is removed, not kept as a fallback.
- `HostObservationFingerprintV1` binds one observation to the executable path and content digest, the exact `--version` and plugin-list bytes, every settings source's exact bytes or typed absence/error, the host and the project root. Session activation still always re-observes; a cached report is reusable only when the fresh fingerprint is complete and byte-identical to the one recorded with it. Errors other than definite absence keep the fingerprint incomplete, so no reuse. Caller validity flags remain refused.

Synthetic controls (run on this Mac): admission refusals, exact stdout and policy-only child environment, non-zero/oversize refusals, timeout with descendant termination, fingerprint stability and drift on settings and executable content. No installed host, global settings or provider was used.

## PREP-SMOKE-1 (prepared, NOT RUN)

Scope: one bounded, read-only installed-host probe. It needs its own concrete disposition before it is run. It is not the approved architectural D13.

1. Operator resolves the installed binary once and records it: `command -v claude`, then `realpath` of the result, `shasum -a 256 <realpath>` and `ls -l <realpath>`.
2. Create a temporary project: `mktemp -d /tmp/receipts-prep-smoke-1.XXXXXX`. No `.claude/` directory is created inside it.
3. Run a one-off test binary or harness that builds `WorkspaceHostProbeRunner::new(<realpath>, ChildEnvPolicy::new(Claude, Subscription))` and calls `refresh_host_capability_report` twice for `HostId::ClaudeCode` with that project root. Deadline 10 s per probe, 1 MiB stdout/stderr bounds, total cap 60 s.
4. Record, without payload logging: exit/typed outcome of each probe, byte lengths and SHA-256 of `--version` and plugin-list stdout, each settings source's typed result (path class only), `is_complete()`, equality of the two fingerprints, wall time, and absence of leftover child processes (`pgrep -f <realpath>` before and after).
5. Cleanup: `rm -rf` the temporary project only. Nothing is written to `~/.claude`, managed settings or plugin state. No model prompt and no provider request is made (`--version` and `plugin list --json` only).

Out of scope here: WorktreeCreate/WorktreeRemove hook smoke (needs the HI-W2 hook bridge and plugin), any `claude -p` or provider execution, and Codex probes.
