# Wave 3 Host surfaces and qualification limits

`HeadlessAdapter` implements the frozen HostAdapter interface without hooks or
process execution. It forwards goal requests through `GoalEntrypointPort`;
composition owns the implementation. Both inferred and CLI-supplied events
retain INFERRED confidence. The explicit emission method validates the frozen
semantic source class and returns the supplied provenance. Provenance is a
caller receipt, not persisted authority. Events go only to the untrusted spool.
Input prompts report unavailable; CI cannot silently supply user approval.

Claude, Codex and headless share the existing CoreView template. `EconomyRenderer`
holds only `&GraphSnapshot`, counts exact states, and displays version/digest.
`render_graph_tree` accepts only the verified `ReadOnlyGraphViewV1` projection
and groups nodes by the projection's read-only `node_workstreams()` map.
Every validated node is present in that map, including explicit `None` for
unassigned nodes. The unassigned group is first, followed by workstream IDs in
UTF-8 byte order; nodes retain snapshot node-ID order. Dependencies display
predecessor state even across groups. Workstream text uses the same escaping as
node IDs. The projection still verifies the original graph digest; no graph
wire or hash format changes.

All three adapters expose `present_graph(&ReadOnlyGraphViewV1)` and delegate to
the same deterministic tree renderer. Economy rendering uses that projection's
`snapshot()` and has no mutable access or State handle. Both
renderers reuse display escaping, including bidi controls. The old snapshot
dump remains for compatibility with existing CLI consumers.

## Open contract dependencies

`CodexHostAdapter` provides supervised delivery through the same spool and
CoreView template. Construction refuses foreign-host and native-mode reports;
emission refuses foreign-host and OBSERVED events while native qualification
is pending. `normalize_codex_supervised_event` stamps Codex/INFERRED on typed
composition input. Semantic source compatibility is checked by the existing
validated emission bridge; neither this adapter nor a source-class label grants
authority.

`codex::hook_map::normalize_codex_hook` covers fixture-backed SessionStart,
SessionEnd, PostToolUse, PreCompact and PostCompact observer mappings. It reuses
the bounded shared hook shape/parser, changes only the canonical host identity,
and drops unrelated fields. It refuses task/workspace/subagent and elicitation
claims from raw hook input. This is a mapping fixture, not proof of installed
Codex payload compatibility or native delivery.

Codex source files can now refute enablement (`features.hooks=false`) and local
managed requirements can refute plugin permission (`allow_managed_hooks_only=true`).
Positive flags, failed observations and missing fields leave trust/permission
UNKNOWN. The existing observation composition selects conservative SUPERVISED;
no separate Codex policy or discovery authority was introduced.

Installed-host Q01 qualified five native observer paths with Codex 0.169.0 on
macOS: `SessionStart`, `PostToolUse`, `PreCompact`, `PostCompact`, and
`SessionEnd`. Each reached the Receipts inbox as `OBSERVED`. Both compaction
events used the manual trigger. The untrusted control produced no inbox, and
the disabled-hooks control preserved the existing inbox hash. See
[Codex native observer delivery](native-delivery-status.md) for enrollment,
trust and custody requirements.

The installed qualification used an isolated plugin registration. Broader
product discovery, automatic compaction, managed policy, Linux execution,
verification that global settings were unchanged, and `EMBEDDED` readiness
remain open. Positive effective enablement, trust and admin facts remain
`UNKNOWN` without an admitted supplier. Local files and hook invocation claims
cannot establish those facts. Supervised delivery remains available. Full
product command composition remains Wave 4.
