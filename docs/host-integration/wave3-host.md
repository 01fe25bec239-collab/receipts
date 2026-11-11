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
and displays incoming edge classes, relations and predecessor states. Both
renderers reuse display escaping, including bidi controls. The old snapshot
dump remains for compatibility with existing CLI consumers.

## Open contract dependencies

The setup projection exposes snapshot node states and edges, but omits each
node's `workstream_id`. Consequently the topology display cannot group by
workstream. A1 must extend the shared Core projection before that requirement
can be completed; Host must not recover membership from summaries or State.

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

Codex discovery and plugin registration remain pending admitted Q01 inputs.
The supplied installed-host Q01 procedure records qualification as pending and
contains no admitted plugin-list response shape or literal namespaced skill
syntax. Existing `plugin list --json` probing is Claude-specific; reusing that
argv or decoder for Codex would guess an unsupported vendor contract. The
existing observation authority correctly keeps positive effective enablement,
trust and admin facts UNKNOWN without an admitted supplier. Local files and
hook invocation claims cannot establish those facts.

Installed-host Q01: PENDING. No installation, registration, trust change,
provider execution or user settings mutation was performed. Full product
command composition remains Wave 4. This partial package needs A1 disposition
of the two contract dependencies before all HI-W3-1–3 obligations can close.
