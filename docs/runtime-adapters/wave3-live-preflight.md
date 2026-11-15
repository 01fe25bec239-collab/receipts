# Wave 3 Runtime preflight (offline preparation only)

Baseline: f657d655acf0562cd21947875147087d92be4dc1. This supplements
[live-proof-plan.md](live-proof-plan.md); it does not grant execution authority.
No live requests, logins, credential inspection or credential copies were made.
Scope: a standalone direct HTTP diagnostic, its offline controls, and a manual
synthetic Ubuntu workflow reusing the existing Linux diagnostic unchanged.
No adapter, routing, credential policy or product export changes.

## First: synthetic Ubuntu

After A1 inspection, independent A3 review and USER-authorized publication of
the exact reviewed commit on `build/wave3-runtime-live-preflight`, make the
manual workflow available through the repository's normal reviewed workflow
publication process (GitHub requires workflow_dispatch on the default branch).
Do not merge or publish merely to bypass that requirement.

Operator commands, only after that authorization:

    gh workflow run runtime-synthetic-preflight.yml --ref build/wave3-runtime-live-preflight
    gh run list --workflow runtime-synthetic-preflight.yml --limit 1
    gh run view <run-id> --log
    gh run download <run-id> --name runtime-synthetic-preflight-<run-id> --dir <new-private-directory>

Verify run head SHA equals the reviewed SHA, attempt=1, ubuntu-24.04,
checksums, PASS, setup/test/cleanup statuses all zero and tests_completed=2.
The workflow has read-only repository permission, pinned actions, no secrets,
no provider CLI and no provider traffic. Build is unprivileged; only the
existing hosted custody phase uses root on the disposable GitHub VM. Retained
or unproven custody fails. Never run hosted on a workstation. No automatic
retry. A failing run remains evidence, not qualification. Actual GitHub-hosted
execution is still outstanding; local shell self-checks do not prove Linux.

## Direct OpenRouter HTTP smoke

Secret NAME: `OPENROUTER_API_KEY` (operator injection only, never a value in
commands, evidence or source). Endpoint is fixed:
`https://openrouter.ai/api/v1/chat/completions`.
Success model IDs: `openai/gpt-4.1-mini` only, for both nonstreaming strict schema
and streaming strict schema. Negative model ID: `receipts/invalid-preflight-model`
(deliberately nonexistent). No aliases, automatic substitution or model list.
The only accepted returned identity is the literal `openai/gpt-4.1-mini`, matching
this approved request ID. No canonical/datestamped alternatives are accepted:
there is no offline evidence here establishing an equivalent identity. A future
alias mapping needs documented provider evidence and fresh review, not an
implicit relaxation after a failed live run. JSON must name this model; SSE
must contain model evidence in at least one chunk, and every model-bearing
chunk must match. PASS records include only the allowlisted `observed_model`;
unrecognized model strings and provider messages are never printed.

Offline command (does not read the key or use the network):

    python3 -B .github/scripts/runtime-live-preflight/openrouter.py
    python3 -B .github/scripts/runtime-live-preflight/test_openrouter.py -v

After separate provider/account/model/spend/runner authorization, operator:
1. Use the exact reviewed clean commit on a disposable Linux host with Python3
   and trusted system TLS roots. Record SHA, UTC, OS, Python version and script
   hash in an operator-owned private directory outside this repository.
2. Inject the named eligible, spend-limited OpenRouter secret using the approved
   secret mechanism. Disable shell tracing. Do not use subscription credentials.
   Set an account/key spending ceiling (suggested maximum USD 0.10); token caps
   alone are not a dollar guarantee. Confirm model availability and accepted
   provider/data policy before running; stop on drift rather than substitute.
3. Execute exactly once, retaining sanitized stdout and exit status privately:

       RECEIPTS_OPENROUTER_SMOKE=1 python3 .github/scripts/runtime-live-preflight/openrouter.py --live

4. Require exit 0 and two model PASS records (including observed_model) plus
   invalid-model error PASS. Exit 2 with `invalid-model qualification INCONCLUSIVE`
   is not acceptance: preserve the sanitized verdict and stop for disposition.
   Other failures exit 1. No failure or inconclusive case is automatically retried.
   Remove the injected secret using
   its operator-managed lifecycle. Preserve failed evidence. Do not upload raw
   responses, request headers, environment dumps or authentication artifacts.

Bounds: at most three requests, two successes with max_tokens=64 each and one
invalid-model request; 20s socket timeout, 75s whole-process deadline, 64KiB
response cap per request. TLS verification stays enabled. Ambient proxies and
redirects are disabled. SSE comments are ignored; missing DONE, provider errors,
non-stop termination, invalid JSON and wrong schema fail. Only choice index 0
is accepted, with at most one choice per event. Its complete answer must end
with a `stop` terminal choice followed by `[DONE]`. Before DONE, one usage
record may repeat choice 0 with a null or `stop` finish reason and a delta
containing only null fields, empty content, or the assistant role. This
record and empty-choice usage records supply no answer or termination evidence.
Data events after DONE fail.
Only sanitized fixed verdicts are emitted; raw bodies are not retained. This deliberately limits
failure diagnostics. HTTP auth/rate/server errors and midstream failure handling
are exercised offline; live negative case tests only invalid-model handling.
Invalid-model PASS requires HTTP 400/404, a matching integer error.code and the
exact diagnostic `receipts/invalid-preflight-model is not a valid model ID`.
This is a conservative synthetic recognition rule, not a claim that the live
provider currently emits that wording. Empty objects, unrelated errors,
different model names or unrecognized wording produce INCONCLUSIVE (exit 2),
never invalid-model proof. Unexpected HTTP status or oversize bodies fail
(exit 1). No raw error text is emitted for either result.

This is HTTP compatibility evidence only, not Receipts adapter qualification.
Vercel remains an optional separate gateway with a separately reviewed harness,
endpoint, secret and model selection; this script cannot redirect to it. Any
gateway result qualifies only that gateway and tested model, never direct
OpenRouter, Codex CLI, Claude CLI or all upstream providers.

## Codex subscription boundary

Passive local observation: `/Users/omkar/.local/bin/codex --version` reports
`codex-cli 0.167.2`; `login --help` advertises `--device-auth`. Neither command
logs in. Linux CLI installation/version and actual account eligibility remain
unverified. No login status, auth file, config values or keychain was inspected.

[Official OpenAI authentication documentation](https://learn.chatgpt.com/docs/auth)
(accessed 2026-11-12) describes headless device login and file/keyring storage.
That establishes a technical mechanism, not eligibility under the frozen
external-worker policy. The Mac executable and its saved sign-in cannot be
assumed usable by a Linux worker with a distinct UID/home. The current synthetic
workflow explicitly clears the environment and provides no saved-login context.
`qualification_tests::env` selects ApiKey, and `codex_case` uses that helper;
there is no subscription qualification selection today. Merely setting
CODEX_HOME in the parent workflow is therefore not a reviewed solution.

A future subscription-backed harness requires separate A1/USER disposition of
OpenAI consumer subscription external-worker eligibility, a pinned Linux Codex
binary and dependencies accessible to the isolated UID, and an operator-owned
login context provisioned on that runner. It must select Subscription through
ChildEnvPolicy (no API key names), pass only admitted HOME/CODEX_HOME names,
and prove the isolated worker can access exactly its authorized credential
store while other worker UIDs cannot. Review must cover refresh writes,
account/workspace restrictions, session files, private logs, expiry/revocation,
cleanup ownership and absence of credential material from public artifacts.
Use synthetic sentinel credentials first, including unavailable store and
cross-UID denial controls. Preserve protected launch, bounded capture, exact
write-denial evidence and custody release checks. Never loosen custody or
copy a workstation auth cache as an implicit fix. No fundamental CLI sign-in
incompatibility is established; the existing harness/provisioning is incomplete.
Claude CLI qualification stays open separately under the merged plan and its
eligible API-key context; neither this smoke nor Codex sign-in qualifies Claude.

References: [OpenRouter structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs),
[streaming](https://openrouter.ai/docs/api_reference/streaming),
[model](https://openrouter.ai/openai/gpt-4.1-mini), accessed 2026-11-12.

## Offline validation scope

Python default self-check retains the bounded JSON/SSE and HTTP error controls.
`test_openrouter.py` adds regression controls for all three review findings,
missing/inconsistent model evidence, mixed choices and invalid terminal order,
plus mocked transport bounds, proxy/redirect refusal, opt-in refusal, immediate
failure stop, three-request success and top-level redaction. All credentials and
responses in these controls are synthetic; they do not perform network calls.

The failing-before and passing-after logs and exact source snapshots are local
review evidence, outside the product repository. Existing synthetic Linux
workflow and diagnostic remain unchanged by this repair. Their prior shell
self-check is harness evidence only; actual hosted execution remains pending.
Rust fmt/Clippy/full suite are not rerun for these Python/runbook-only repairs.
