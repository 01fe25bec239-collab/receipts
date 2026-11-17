# Owned HTTPS and SSH qualification

Run separately from D4, using only merged interfaces at the reviewed
`codex/wave3-live-qualification` SHA. No external repository, service account,
provider account or real credential is needed. The manual
`wave3-workspace-transports.yml` job uses an ephemeral Ubuntu 24.04 runner,
read-only checkout, generated keys/password, a loopback TLS smart-HTTP Git
backend and a loopback OpenSSH server. It creates no protected refs.

After review and separately authorized publication, dispatch once:

    gh workflow run wave3-workspace-transports.yml --ref codex/wave3-live-qualification

Require exact reviewed run SHA/tree, attempt 1, successful build, both transport
records, shutdown/cleanup PASS and matching artifact checksums. GitHub dispatch
also requires the workflow's normal default-branch publication; do not merge or
change the ref guard to bypass that gate. Nothing in this preparation dispatches
or publishes a workflow.

For a disposable Linux host, build with `cargo test --locked -p
receipts-workspace-execution --lib --no-run --message-format=json`. Select the
single executable compiler-artifact with `profile.test=true`, then run:

    python3 -B .github/scripts/wave3-qualification/workspace.py /absolute/test-binary

Requires Python3, OpenSSL with `req -addext`, Git smart HTTP backend, OpenSSH
client/server and the server's platform prerequisites. Run as the unprivileged
runner; the hosted workflow prepares `/run/sshd` on its disposable VM. Fixture
UID must be eligible for public-key SSH, and no unrelated account is modified.
The generated private root is a new directory under the runner home, so
OpenSSH StrictModes can verify every ancestor. No existing home file is changed.
The private root and services are owned by this invocation. Listener
ports bind only 127.0.0.1. Do not run this fixture against external endpoints.
`--tls-only` is a partial local control and proves no SSH behavior.

The ignored Rust check invokes `publish_durable_with_transport`, reads the
exact destination SHA independently from the owned bare repository, reopens
State and reconciles, refuses a different TLS CA / SSH host key, refuses missing
HTTPS auth / a different SSH client key, refuses missing acceptance, and refuses
an unrelated non-fast-forward commit while retaining the published SHA. Trust
and auth failures must remain Uncertain, never Published. Good settings pin TLS
verification/CA and strict SSH known-host identity, with ambient config, agents,
proxies and prompts excluded by the existing hardened execution path.

Cleanup stops owned services before deleting the unique private root, including
its bare repository and generated credentials. The one fixture ref is removed
with an exact expected-SHA comparison between protocols; this is owned fixture
teardown, not product branch-pruning qualification. State is disposable; real
publication/deletion retention controls remain covered by existing focused tests.
Only fixed verdict records and source/tool hashes are uploaded, never generated
credentials, transport diagnostics, auth headers or repository contents. A
failed/interrupted job is not qualified even if a partial artifact exists.

This proves actual Git/TLS/SSH interoperability through the durable path for
this fixture and installed tools. It does not prove external account access,
public CA deployment, internet routing, production acceptance provenance,
malicious same-user confinement or D4 recovery.
