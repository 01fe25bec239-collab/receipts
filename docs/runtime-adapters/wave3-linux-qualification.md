# Wave 3 Runtime Linux qualification

Use the exact independently reviewed SHA and tree. Keep credential-free controls,
the OpenRouter HTTP witness and each native CLI result in separate records.

## Run synthetic first

    python3 -B .github/scripts/runtime-live-preflight/openrouter.py
    python3 -B .github/scripts/runtime-live-preflight/test_openrouter.py -v
    python3 -B .github/scripts/wave3-qualification/test_openrouter_job.py -v
    GITHUB_WORKSPACE="$(pwd -P)" python3 -I .github/scripts/wave3-qualification/openrouter_job.py

Run from the checkout root. The launcher requires the canonical absolute
checkout path in `GITHUB_WORKSPACE` even when running offline.

On a disposable Linux runner only, as root:

    env -i PATH=/usr/bin:/bin /usr/bin/python3 -I .github/scripts/wave3-qualification/custody.py

The manual `wave3-runtime-controls.yml` job runs these without any secret.
After authorized publication, dispatch once with `--ref
codex/wave3-live-qualification`. UIDs 63171/63172 must be unused; the fixture
uses only a new private temporary directory. It proves owner-only sentinel
read, denied cross-UID read, a refresh-like write, missing/revoked store refusal,
no API-key environment names in that child, and owned-directory cleanup.
It does not qualify a provider credential format, OS keyring, CLI refresh,
protected Runtime launch or hostile same-user confinement. The existing
Runtime/Workspace custody self-checks and synthetic adapter tests remain needed.

## Manually dispatched bounded OpenRouter job

The live workflow candidate is `.github/workflows/wave3-openrouter-live.yml`.
The security policy permits the secret reference only for that workflow
at the exact reviewed workflow and executed-source digests. Every other
secret-consuming workflow remains subject to the general refusal. The
documentation template is retained as preparation history. A1 must verify
the deployed environment settings and authorize the live run separately.

Operator inputs before live execution: exact independently reviewed published
SHA, an environment named `wave3-openrouter` restricted to the exact branch,
required operator approval (and no self-approval where available), an
**environment-scoped** `OPENROUTER_API_KEY`, explicit account/provider/data
policy approval and a key/account spending ceiling (suggested USD 0.10).
Confirm `openai/gpt-4.1-mini` availability and exact returned identity before
execution; do not substitute after a refusal. No workflow_dispatch input accepts
a key. Do not use a repository-wide secret or export any login cache.

Once policy, publication and live authority are settled, dispatch the activated
workflow once from the reviewed branch. Secret exposure is restricted to one
probe step; tests and artifact upload receive no secret expression. The wrapper
passes only a private temporary HOME, PATH, LANG, the opt-in and this API key to
the unchanged probe. No retries. It enforces an 85s outer deadline around the
probe's existing 75s deadline, three-request/64-token success bounds, 20s socket
limits, 64KiB captures, fixed HTTPS endpoint, TLS verification and proxy/redirect
refusal. Only allowlisted success evidence is emitted; errors/inconclusive cases
fail with fixed diagnostics. Raw responses, stderr and credentials are not
uploaded. Retain provenance, fixed verdict, artifact checksums and job outcome;
absence of a complete matching PASS packet is not qualification. The ephemeral
VM is discarded after the job; revoke/delete the environment key separately by
its owner. No login artifact is created or retained by this wrapper.

A successful run proves direct OpenRouter JSON/SSE/schema/model/error-path
compatibility on that Linux image. It proves neither a Receipts Runtime adapter
nor Claude/Codex protected CLI conformance. See the existing
[bounded probe contract](wave3-live-preflight.md) for exact accepted responses.

## Personal local native CLI checklist

A1 reviews the harness and obtains exact-subject independent technical and
applicable Q06 review before any live use. USER personally signs in with their
own eligible accounts on their disposable VM. Confirm the account/workspace
controls, approved data and run budget separately for both vendors. This scope
keeps the frozen external-worker eligibility dispositions and routing intact.
Use fresh stores created by the vendor CLIs on the runner. Keep workstation
stores, extracted tokens, account sharing and rate-limit workarounds out of the
run. Stop on rate limits and preserve the first outcome.

### 1. Bind the guest, tools and case identities

- On the host, record `multipass info receipts-wave3`. In the guest record
  `uname -srm`, `/etc/os-release`, `id -u` and `id -g`. The recorded guest
  is Ubuntu 24.04 ARM64. Ordinary attempts use the **same unprivileged guest
  UID/GID** as the test process and direct login. Resolve and record the numbers
  in the actual guest shell. There is no protected controller or worker UID
  allocation on ARM64.
- Verify a clean checkout of the admitted commit and tree before and after.
  Record UTC, kernel and OS/image, toolchain, `Cargo.lock`, the test binary,
  Runtime/Workspace source and helper hashes, binaries and effective nonsecret
  configuration. Stage immutable, official per-architecture binaries by
  absolute path. Verify release archive/manifest, installed executable and
  any helper separately. Check `--version`, resolved paths and hashes
  immediately before use. Include Git, the actual Bubblewrap binary and
  package or release version, any required code-mode host, and the effective
  AppArmor/sandbox configuration. Stop on drift, unverified executable or
  launcher paths, or a required missing helper. Never disable the CLI sandbox
  to get a result.
- ARM64 Codex candidate from the prior guest witness is the official
  [rust-v0.169.0 release](https://github.com/openai/codex/releases/tag/rust-v0.169.0).
  Match the archived ARM64 pins below. The witness used Ubuntu's actual
  `/usr/bin/bwrap`, package `0.9.0-1ubuntu0.3`, with a guest path-scoped
  AppArmor allowance. Compare that effective policy and binary separately
  before using it. A downloaded release Bubblewrap archive has its own pin.
  Previous guest launch paths and settings may have been removed.
- Claude Code `2.1.322` is the prior macOS observation and a candidate
  version. A1 must pin the official Linux ARM64 artifact's release provenance,
  manifest digest and verified binary SHA-256 (and any resolved helpers) before
  authorizing live Claude. No Linux Claude artifact or authenticated install is
  attested here. An admitted x86_64 run likewise needs separate official
  x86_64 pins.

| Codex ARM64 reference | SHA-256 |
| --- | --- |
| Codex release archive | `97b97d87eea075c5cb7709c7f6156a451622a7667953ecda40e0276637196f3f` |
| Extracted Codex | `3bb41c9a80187773e2a59278d4fc355b1a163c31bb5a79581e37d4fe428af047` |
| Code-mode host release archive | `059397e4d5beda7a7acc7441a6a56457d0f79e1001d129976ac0d26b0e6ee04c` |
| Extracted code-mode host | `66874c7881c4004f1c155584e1325cad17795bdd91b886995ad8a043809c1019` |
| Bubblewrap release archive | `bea7f64d75eb93931535542c61b18da9ec70c35db044d4d43ec6d34027f53fb0` |
| Ubuntu `/usr/bin/bwrap` in prior guest witness | `cd6b143283b464baf078ab09ca19dadc5bcc2423b833c4f9dd7e83f9ba1b640b` |

### 2. Build and run synthetic controls before sign-in

As the unprivileged guest operator, use Bash with `umask 077`. Set `private` to
a fresh absolute operator-owned directory outside the checkout, mode 0700.
Bind the executable path from the locked build output and save build status
and hashes. Clear all live/qualification opt-ins and use a credential-free
build/test environment.

```bash
cargo test --locked -p receipts-runtime-adapters --lib --no-run \
  --message-format=json >"$private/build.jsonl" 2>"$private/build.stderr"
```

Require exit 0. Set `bin` to the one executable in the matching
`compiler-artifact` record with target `receipts_runtime_adapters` and
`profile.test == true`. Record its resolved absolute path and SHA-256.
Run `timeout --kill-after=10s 240s "$bin" --test-threads=1` with no
`--ignored`. Require exit 0. Run the credential-free Python controls above.
The Linux UID sentinel control needs separately authorized root and unused
UIDs 63171/63172. Its labels for refresh and revocation describe synthetic
file operations. Use the x86_64 native diagnostic only under its own supported
guest, administration and admission conditions.

### 3. Check stores and sign in directly

- For each provider make a separate, fresh per-run `HOME` and config
  directory, 0700 and owned by the execution UID/GID, with private temp/cache
  locations. Use `CODEX_HOME` or `CLAUDE_CONFIG_DIR` explicitly and retain
  only owner/mode/access facts. Use an official file-backed store when the CLI
  requires a choice, with no desktop keyring or shared login session fallback.
  Record configuration hashes for reviewed **nonsecret** settings only.
- Before login, with all secret environment sources cleared, in the same
  UID/HOME/config context use `timeout --kill-after=5s 30s "$codex" login status`
  or `timeout --kill-after=5s 30s "$claude" auth status`. Discard raw auth
  output after local observation and record only exit and whether auth was
  reported unavailable. Timed out, unrecognized or already logged-in status is
  inconclusive. A provider-auth failure observed during any run remains a
  failure. An empty-store status result alone does not prove a rejected model
  request.
- Run direct vendor login interactively in that same context, for example
  `timeout --foreground --kill-after=10s 600s "$codex" login` or
  `timeout --foreground --kill-after=10s 600s "$claude" auth login`. Use the
  official sign-in flow, with an approved device flow only when that CLI and
  account support it. No terminal recording, cache copy, token collection or
  account identifiers in evidence. Repeat the bounded status check and record
  connected status separately from the missing-store observation.
- Verify owner-only access and a distinct nonroot, unprivileged UID's denial
  using a known 0600 **sentinel** and private directory traversal. Run the
  checks as each UID with cleared supplementary groups. Verify the expected
  owner can read the sentinel, the foreign UID cannot read it or traverse the
  0700 store, and the real store owner and mode match using metadata only.
  Do not open credential files. Retain UIDs and booleans. If a keyring or
  ownership assumption cannot be independently checked, mark the boundary
  unverified and stop that case.

### 4. Run ordinary success/cancel and Claude qualification

After sign-in and before launch, use the exact test binary with an explicit
`RECEIPTS_LIVE_AUTH_MODE=subscription`. The alternative exact selection is
`api-key`, for a separately admitted API-key run. An unset selector retains
the old defaults (ordinary Subscription, qualification ApiKey). Empty,
combined, unrecognized or non-UTF-8 values refuse. No mode is inferred from
present credentials. Subscription strips both known provider API keys via
`ChildEnvPolicy`; ApiKey admits only the selected provider's key if present.
Use isolated config in either mode. The printed inventory carries names only.
Each ignored test also requires its own opt-in set to `1`.

Set `codex` and `claude` to pinned absolute executables and provide
`codex_home`, `claude_home`, their independent `*_config` and private
`*_tmp` paths. Set `cli_path` to a verified absolute-entry PATH for the tools
and helpers. The following uses new case directories and preserves the outer
exit (including timeout) and UTC window. Invoke each named case once. Keep
terminal/log contents on the guest until the export step.

```bash
run_case() {
  local name=$1 bound=$2 test=$3 rc=0
  shift 3
  local dest="$private/$name"
  mkdir -m 0700 -- "$dest" || return
  date -u +%FT%TZ >"$dest/start.utc"
  timeout --kill-after=10s "$bound" env -i PATH="$cli_path" \
    RECEIPTS_LIVE_AUTH_MODE=subscription RECEIPTS_LIVE_EVIDENCE_DIR="$dest" \
    "$@" "$bin" "$test" --ignored --exact --nocapture --test-threads=1 \
    </dev/null >"$dest/harness.log" 2>&1 || rc=$?
  printf '%s\n' "$rc" >"$dest/exit"
  date -u +%FT%TZ >"$dest/end.utc"
  return "$rc"
}

run_case codex-ordinary 420s live_tests::live_codex_ordinary_success_and_cancel \
  HOME="$codex_home" CODEX_HOME="$codex_config" TMPDIR="$codex_tmp" \
  RECEIPTS_LIVE_CODEX=1 RECEIPTS_LIVE_CODEX_EXE="$codex" &&

run_case claude-ordinary 420s live_tests::live_claude_ordinary_success_and_cancel \
  HOME="$claude_home" CLAUDE_CONFIG_DIR="$claude_config" TMPDIR="$claude_tmp" \
  RECEIPTS_LIVE_CLAUDE=1 RECEIPTS_LIVE_CLAUDE_EXE="$claude" &&

run_case claude-qualification 1400s qualification_tests::live_claude_qualification \
  HOME="$claude_home" CLAUDE_CONFIG_DIR="$claude_config" TMPDIR="$claude_tmp" \
  RECEIPTS_QUALIFY_CLAUDE=1 RECEIPTS_LIVE_CLAUDE_EXE="$claude"
```

Require an outer exit of 0 and exactly one executed test for each invocation.
The process cap per attempt is 180s plus 5s grace and Workspace's bounded
cleanup checks. Ordinary L1 requires completed process, exit 0, valid success
protocol, unchanged worktree and fixture removal. L2 uses a 30s first-event
bound and requires a recognized Codex progress event or Claude initialization,
accepted `UserRequested` cancellation and `USER_CANCELLED` classification with
verified collection and removal.
Intentional cancellation can end before a provider terminal result.
The harness stops launching subcases after a failed verdict and retains the
completed attempt's evidence. A Claude rate-limit retry or rejection fails
the attempt even if the CLI later reports success. A failed invocation stops this
command sequence. Preserve the output and review it before any further runs.

Record Claude plan, schema/turn limit (exact schema and 1–2 turns) and
FIRST/SECOND resume separately. The resume case also requires read-only and
used-ticket refusal. Claude results carry `protected=false` and
`reduced_isolation=true`; they do not prove denial of reviewer writes.
Require complete captures and unchanged tracked bytes, HEAD and Git status.
Record CLI exit and protocol facts, cancel acceptance/reason, first event
presence, truncation, worktree checks and cleanup. Missing or incomplete
evidence fails the affected case.

### 5. Codex protected reviewer — blocked on this ARM64 guest

The launch chain is `qualification_tests::codex_case` →
`protected_launch_tests::launcher` → `ProtectedLauncher::new` →
`protected_linux::probe`. The non-x86_64 implementation explicitly refuses.
No cgroup, anchor, login or UID setup can admit protected execution on
`receipts-wave3`. The hosted script's `hosted` phase also explicitly checks
x86_64 and drives synthetic executables only. On ARM64, record
`BLOCKED_UNSUPPORTED_ARCH` for this case and use the ordinary results above
only for their stated claims.

A1 can choose a separately admitted disposable **x86_64** Ubuntu 24.04 guest.
The fixture's test launcher runs as EUID **0**. Workspace controller Git
subprocesses run as **61001:61001**. The provider worker uses allocated UID
**62000–62063** and GID **61002**, with cleared supplementary groups and
dropped capabilities. A new launcher scans upward starting at 62000 and skips
UIDs in use. Prove actual worker UID/GID with the synthetic native worker
under the admitted setup, then verify the entire reserved identity range is
idle before the one live invocation. Protect the range from concurrent use.
Use **62000:61002** for direct Codex login only after fixing and proving that
allocation in this isolated guest. Exit the login shell and every process
under that UID before launch. If the lease cannot be bound to the store owner,
stop. Never use the root controller's or another UID's store.

- Administer a new dedicated cgroup v2 root with `cgroup.procs`,
  `cgroup.kill` and `cgroup.events`; verify Landlock ABI ≥ 3, seccomp,
  protected hardlink/symlink sysctls and the VM's initial namespaces.
  Use the verified host setup pattern from
  `.github/scripts/we-w1-completion-linux/custody.sh`: root-owned
  `/var/lib/receipts` (0755), `protected-custody` (0700), single-link lock
  and boot/mount identity files (0600), and initial PID namespace anchor.
  Record device/inode identities and restorability. Refuse preexisting,
  unknown or occupied fixtures. Compile the test binary and native probe
  unprivileged and pin their bytes for privileged execution. The hosted
  diagnostic requires separate admission for any local use.
- The privileged harness Git setup needs a trusted root-owned HOME and
  config. Pass the worker-owned fresh store only as `CODEX_HOME` to the
  protected test. `ChildEnvPolicy` snapshots the parent environment and
  Workspace changes UID/GID without rewriting HOME/config paths or admitting
  a desktop keyring. Verify that the pinned Codex recognizes the same store
  in this exact worker context. Use separate trusted controller paths and
  worker paths that are traversable where needed. If the CLI needs a
  worker-writable HOME or another service absent from this launch, that
  credential composition is unverified. Do not loosen store access or point
  privileged Git at a worker-controlled HOME.
- Workspace's x86_64 worker seccomp filter refuses `unshare` and
  namespace-creating `clone`. A Codex sandbox/helper that needs those calls
  can fail before the required write attempt. Establish compatible behavior
  for the pinned official CLI and helpers under the actual protected path.
  A helper setup error or a denied namespace syscall cannot stand in for
  read-only filesystem denial. Preserve the boundary if the combination is
  unsupported, and report it to A1 for disposition.
- Once those gates are verified, use the Bash function above in the admitted
  root environment with independent private evidence and the exact x86_64
  test binary. Use the system sticky `/tmp` for fixtures, and record matching
  fixture directory membership before and after.

```bash
run_case codex-protected 240s qualification_tests::live_codex_protected_reviewer \
  HOME="$trusted_controller_home" CODEX_HOME="$worker_codex_config" \
  RECEIPTS_RA_HOSTED_CGROUP="$dedicated_cgroup" \
  RECEIPTS_QUALIFY_CODEX=1 RECEIPTS_LIVE_CODEX_EXE="$codex"
```

Require collected protected launch and completed Codex JSONL with exit 0.
The actual command execution record must name the exact bounded target write,
including when Codex displays it in supported shell argv, and report nonzero
exit with a filesystem denial for that target. A refusal to try,
unrelated error or unrecognized capture shape fails. Require custody release,
then verify restored ownership, unchanged Git HEAD/status and the tracked
bytes. Require fixture removal and observe the owned cgroup proven empty and
removed, no live worker or controller identity processes, and host-anchor
cleanup. Retain owned resources with uncertain custody or process state.
Ordinary Codex success supplies no substitute reviewer verdict.

### 6. Close the run and export only evidence fields

- Preserve the first failure, timeouts and cleanup uncertainties. Reconcile
  all attempted cases with their recorded exit, expected summaries and UTC
  window. Record worktree and process/cgroup cleanup separately from
  credential cleanup. For a failed or interrupted run retain the private
  directory and observe/reap only owned processes. Stop if ownership is
  uncertain.
- Only USER should log out or revoke the fresh login created for the run,
  after all dependent cases complete. Use the vendor CLI under the same UID
  and isolated store or the vendor's session-specific control if one is
  available. Never revoke an unrelated sign-in. Record observed post-logout
  auth status versus any separately observed provider-side revocation. Leave
  actual refresh and server-side revocation **UNVERIFIED** when unobserved.
  Remove only owned disposable stores, logs and settings after the private
  verdict is assembled. Verify absence and restored host policy separately.
- Export a new private, payload-free packet with exact source, archive,
  tool, helper and nonsecret config identities; UID/access and auth-status
  booleans; bounds, UTC, sanitized exit/protocol/verdict facts and cleanup.
  Validate field membership and scan the packet before transfer. Seal the
  export with `sha256sum` and independently verify after transfer. Do not
  export raw stdout/stderr fragments, auth/session data, login terminal output,
  secrets, identifiers or unreviewed settings. Keep private raw captures
  outside the repository. A1 adjudicates each case and the remaining Wave 3
  evidence separately.
