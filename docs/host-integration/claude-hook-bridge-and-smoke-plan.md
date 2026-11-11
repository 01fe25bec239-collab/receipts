# Claude hook bridge and installed-host proof plan

The hook-only CLI accepts bounded JSON on stdin. Observer failures exit zero; WorktreeCreate prints one absolute path on success and exits one on refusal. The launcher preserves this policy for missing binaries and execution failures.

Hook, environment, plugin-list and configuration data are observations. None establishes positive effective enablement, admin permission or task authority. Known disableAllHooks, disabled Receipts entries and managed-only restrictions refute permission even when a hook claims to run. Without an admitted effective-permission supplier, capability selection stays conservative. Vendor-wide coverage is separate from the eleven required lifecycle hooks; recognized registrations establish at most PARTIAL vendor coverage.

SessionStart locks plugin-data storage and atomically persists FAILED/SUPERVISED currency before parsing or probing. A successful refresh replaces it; a failed refresh retains it. Unsafe or inaccessible storage is preserved with an explicit warning that prior currency may remain. Cache reuse requires equality with fresh selection, including its fingerprint. Timestamps are local observations, not authenticated provenance.

Host storage walks directory descriptors without following links, checks ownership and permissions before creating entries, and checks opened file identity. The project must match the hook cwd and Git repository. Worktree parents are validated before Workspace invocation. Spool append holds a cross-process flock through tail/size checks, append and sync. An incomplete tail retains every byte and refuses further append. Storage assumes trusted same-user custody; it does not confine hostile processes.

FileChanged, SubagentStart/Stop and TaskCompleted are registered translation requests. Raw input emits no Worker/Core fact. HostCompositionPort permits independently admitted suppliers to provide matching events, task context and plain cleanup policy. The CLI has no live trusted supplier: task creation uses the plain fallback and removal retains data. The port must never be implemented by promoting raw hook fields.

A fresh plain creation can return OwnedPlainWorktree, which retains identity descriptors across clones. Attach cannot adopt ownership. Trusted composition may retain this proof and later request cleanup. Repository, path, inode, Git pointer bytes, registration, branch, HEAD, task-binding absence and cleanliness are rechecked before non-force removal. Refusals retain data; branches remain. Task/protected cleanup is excluded. WORKSPACE_REMOVED is emitted only after verified removal. Git operations retain Workspace's sequential same-user custody assumptions.

## PREP-SMOKE-2 / Q02 (prepared, not executed)

A separate USER grant is required for installed-host execution. Prior evidence describes Claude 2.1.246; static inspection recorded 2.1.319 and is not hook execution proof. At the authorized run, record actual executable/version/digest and recheck hook support before loading the plugin.

1. Build the checked receipts CLI and record its source identity and binary hash. Assign one private temporary root T, then create distinct T/plugin, T/repo and T/data paths. Copy the plugin to T/plugin and the binary to T/plugin/bin/receipts. Seed T/repo with a local commit.
2. Specify the absolute host executable and identical environment for both launches, including RECEIPTS_BIN and RECEIPTS_CLAUDE_EXECUTABLE. Record actual host-assigned plugin-data storage; do not assume it equals T/data. Do not change global or managed settings.
3. Launch interactively from T/repo with --plugin-dir T/plugin; send no model prompt and exit. Repeat with the version's documented worktree entry and the same executable/environment. Keep a ten-minute total cap and the registered per-hook timeouts.
4. Retain exact stdout, stderr, exits, spool types/confidence/session, capability currency, worktree path/branch/registration and observed removal behavior. Expect conservative permission and retained worktrees when the live trusted suppliers are absent. Record only this run's child process IDs/handles; broad process-name matching proves no ownership.
5. Cleanup only verified run-owned clean worktrees through non-force Git removal. Preserve dirty, replaced, ambiguous or foreign targets. Delete temporary data only after exact root and run ownership checks; preserve unknown/outside plugin-data paths for USER disposition. Record child exit and cleanup results separately.

Current installed-host Q02, native Linux validation, live trusted composition, independent A3 and applicable Q06 remain separate proof boundaries. No State database writer exists in the Host/CLI path.
