# D12 remote policy compatibility

The frozen wire vocabulary stays `LOCAL_ONLY`, `PUSH_ON_ACCEPT`,
`PUSH_ALWAYS`. An explicit trusted project mode narrows branch scope; it does
not remove the wire policy's timing requirement.

| Wire value | Required project mode | Eligible scope and timing |
| --- | --- | --- |
| `LOCAL_ONLY` | `LOCAL_ONLY` | No remote contact |
| `PUSH_ON_ACCEPT` | `PUSH_A2_BRANCHES` | Accepted workstream branches only |
| `PUSH_ON_ACCEPT` | `PUSH_ACCEPTED_A3` | Accepted workstream and task branches |
| `PUSH_ALWAYS` | `PUSH_ALL_CHECKPOINTS` | Eligible checkpoint branches, including rejected attempts |

The trusted orchestrator supplies project configuration and exact acceptance
for the branch/commit where required. Worker claims are not acceptance. Missing
or conflicting modes, unknown branch classes and unknown wire values refuse;
there is no implicit workstream default for an ambiguous wire value.

Publication uses only `refs/heads/<branch>:refs/heads/<branch>`. Force flags,
force-with-lease and `+` refspecs are forbidden. Protected `main` needs the
integration gate under every mode. Pushed rejected evidence is retained;
pruning cannot override that retention. Remote failure is recorded without
undoing local progress. Review does not depend on successful publication.

This ADR fixes shared interpretation. Publication, retry and pruning execution
belong to the Workspace publication feature.

D12 durable publication uses `publish_durable` at the trusted composition
boundary and State v25. Its immutable operation identity binds project,
canonical repository and owned file remote, full equal source/destination
refs, commit, both policy vocabularies, branch class and applicable acceptance
artifact reference. Composition must verify acceptance before supplying it.
The lower-level `publish` remains a transport helper and provides no durability.

Intent without an outcome derives `Uncertain`. Before pushing, durable
publication appends `Uncertain`; every retry reads the exact destination ref.
Only an exact matching observation records `Published`. Failed readback stays
`Uncertain`; a successful differing/absent observation can record `RemoteFailed`.
Outcomes retain ordered history, trusted timestamps and bounded evidence refs;
no transport output or credentials are persisted. Reads confer no acceptance.
File remotes are the supported transport; production transports, pruning,
D4 restart authority and automatic cleanup acknowledgement remain open.
