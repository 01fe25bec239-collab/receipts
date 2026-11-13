# Codex native observer delivery

The production plugin registers SessionStart, SessionEnd, PostToolUse,
PreCompact and PostCompact. `receipts hook codex <Event>` accepts bounded
observer input only after checking an explicit local enrollment and live OS
parent identity. Direct calls, unknown wrappers, trust-bypass launches,
changed executable/plugin bytes, malformed input, event/project mismatch and
missing/mismatched session bindings are refused with no inbox append.

Enrollment is an operator action, separate from Codex hook trust. Supply
canonical absolute paths:

```
receipts codex-native-enroll PRIVATE_DIRECTORY CODEX_EXECUTABLE PROJECT INSTALLED_PLUGIN
```

Create PRIVATE_DIRECTORY outside the project and installed plugin, owned by
the current user with mode 0700. Enrollment pins the reviewed Codex executable
and installed manifest, hook definitions and launcher. Re-enroll after updates.
Export RECEIPTS_CODEX_ENROLLMENT to this directory and RECEIPTS_BIN to the
absolute installed Receipts binary before starting Codex. Review and trust
only the installed Receipts hooks through Codex's normal UI. Never bypass hook
trust. Codex enforces effective hook trust, enablement and managed policy;
settings and capability-report JSON never grant delivery authority.

Receipts accepts the enrolled executable as the live parent, optionally through
its fixed command-hook /bin/sh (or /bin/dash) invocation. Unknown process
metadata or unsupported platforms fail closed. macOS and Linux have native
ancestry implementations. The process birth identity prevents PID reuse from
reusing a session binding. SessionStart establishes the session; later events
must match. Only the native delivery method appends OBSERVED events; generic
CodexHostAdapter emission continues to require INFERRED supervised events.
The capability report remains conservative: five observer hooks do not prove
full frozen coverage or EMBEDDED readiness.

This follows Host spool custody: the owning OS user is trusted. A hostile
same-user process can alter enrollment or invoke enrolled programs; this is
not OS confinement. Executable hashes detect drift, not an immutable execution
object. Missing enrollment, identity drift or unavailable delivery preserves
supervised behavior. Receipts does not change host policy, approve its own
hooks, open State or read credentials. Enroll only the actual reviewed Codex
executable and installed plugin, never arbitrary helper programs.

Codex's trust semantics are documented at https://learn.chatgpt.com/docs/hooks
and plugin packaging at https://developers.openai.com/plugins/build/plugins.

Qualification uses a fresh private root, isolated CODEX_HOME and disposable Git
repository. Install the actual plugin, verify installed hashes and sign in
directly if needed. Complete an untrusted no-tool turn and verify no inbox;
trust only Receipts hooks, start fresh, complete a shell turn, invoke /compact,
confirm completion and exit normally. Inspect sanitized normalized event types,
compaction phases and actual trigger. In a fresh --disable hooks session,
complete another no-tool turn and verify the inbox hash/count is unchanged.
Record automatic compaction and isolated managed policy separately when tested.
Retain only sanitized counts, hashes, UTC and outcomes; remove isolated plugin
and marketplace registrations and verify residue before disposable cleanup.
Synthetic fixtures are parser tests, never installed delivery evidence.
