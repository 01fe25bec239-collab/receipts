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
boundary and State v26. Its immutable operation identity binds project,
canonical repository and credential-free transport endpoint, full equal source/destination
refs, commit, both policy vocabularies, branch class and applicable acceptance
artifact reference. Composition must verify acceptance before supplying it.
The lower-level `publish` remains a transport helper and provides no durability.

Intent without an outcome derives `Uncertain`. Before pushing, durable
publication appends `Uncertain`; every retry reads the exact destination ref.
Only an exact matching observation records `Published`. Failed readback stays
`Uncertain`; a successful differing/absent observation can record `RemoteFailed`.
Outcomes retain ordered history, trusted timestamps and bounded evidence refs;
no transport output or credentials are persisted. Reads confer no acceptance.
File remotes remain compatible. `publish_durable_with_transport` admits HTTPS
and SSH through explicitly trusted configuration. Endpoint identities use
`https://host[:port]/path` or `ssh://[login@]host[:port]/path`; SSH login is
preserved. The conservative ASCII grammar refuses userinfo on HTTPS, SSH
passwords, URL escapes, queries, fragments and dot path components. IPv6
literal endpoints are currently unsupported. Credential helpers, SSH commands,
TLS settings, HOME and agent socket paths come from trusted composition outside
State; no ambient configuration or environment is inherited. Configuration and
credential stores must remain outside worker write authority. Trusted config
must use existing credential mechanisms, never embed credentials in helper
arguments. Repository transport overrides, remote subsections shadowing endpoint URLs,
and all URL rewriting refuse.
Transport diagnostics are discarded and never persisted. Tests use controlled
transport fixtures; live credentials, TLS and SSH interoperability require
separate qualification.

Branches remain by default. The store holder verifies explicit user direction
before `authorize_remote_deletion_v1`, which durably consumes its unique ID and
returns a non-cloneable capability. The intent binds project, repository,
endpoint, full ref and expected commit. Existing file intent JSON omits the new
optional authorization field, preserving replay bytes. The forward v26 version
barrier prevents old writers from
bypassing deletion retention. Historical SQL stays unchanged. State reads complete ref history, retains unresolved publication and
all PUSH_ALWAYS evidence, and excludes unfinished deletion from new publication
or replay. This deliberately retains accepted checkpoints too until a richer
acceptance/retention contract exists. Main/master always refuse; trusted workflow
also supplies its protected refs to deletion.

The exclusive State writer is held throughout remote execution and readback;
mutable store access serializes these workflows. A Git ref lock also excludes
cooperating local publication while deletion runs. These are trusted workflow
and cooperative filesystem boundaries, not exclusion of hostile same-user code
or writers using unrelated State stores. Use durable entry points for workflow
writes; `publish` remains a lower-level compatibility helper.

Exact readback requests symbolic-ref metadata and refuses aliases, preventing
deleted aliases from deleting their protected targets. Deletion compares the remote SHA, then uses a deletion-only exact lease at the
receive side to refuse movement after readback. Publication never uses force or
lease flags. Deletion affects the remote ref only; local branches and evidence
are retained. Lost responses stay Uncertain until readback. Local policy,
configuration or ref-lock refusals during reconciliation record their reason
and preserve a started deletion's Uncertain outcome, keeping competing
publication and deletion blocked until remote evidence resolves it. Reopening permits
readback only and never repeats the destructive command; another deletion
attempt requires new user direction. An absent ref proves deletion only after
a durable delete-started marker. D4 restart authority and automatic cleanup
acknowledgement remain separate work.
