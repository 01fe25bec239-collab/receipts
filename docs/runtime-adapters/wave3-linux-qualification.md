# Wave 3 independent Runtime Linux preparation

No live requests, login-cache inspection/copies or provider credentials are
needed for these controls. Use the reviewed `codex/wave3-live-qualification`
SHA/tree independently of D4. Existing direct macOS OpenRouter and synthetic
Ubuntu evidence remain distinct from any new Linux provider proof.

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

## Subscription eligibility and existing auth selection

Checked against official documentation on 2026-11-17; account-specific terms,
plan/workspace controls and execution authority remain operator inputs. No
account status or stored credential was read.

* [OpenAI authentication](https://learn.chatgpt.com/docs/auth) documents ChatGPT
  subscription sign-in and runner-side device-code login, and directs
  programmatic CI workflows to API-key authentication. This is technical support,
  not approval of this external-worker subscription context. Enterprise access
  tokens are a separate entitlement and are not implied by a consumer plan.
  Eligibility remains NEEDS_REVIEW under the frozen external-worker policy.
  Do not use the documented cache-copy fallback: this task explicitly forbids it.
* [Anthropic authentication/credential terms](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use)
  distinguish ordinary native subscription use from third-party product use and
  prohibit intermediating subscription credentials. The same current page
  discusses unmodified, end-user-authenticated hosted Claude Code under commercial
  conditions; that does not by itself admit Receipts' frozen worker context.
  Preserve the existing DISALLOWED disposition for that context until A1/USER
  explicitly settles any revised applicability. Never collect OAuth tokens or
  treat a Claude subscription as an Anthropic API key.

Code inspection: `qualification_tests::env` selects `ChildEnvAuthMode::ApiKey`
for both qualification paths. `live_tests` selects `Subscription` for ordinary
success/cancel tests; these are separate, ordinary launches, not protected Linux
qualification. Workspace `ChildEnvPolicy` supports distinct modes; an environment
variable alone does not change the qualification test's hardcoded mode.
Neither this HTTP job nor a successful sentinel test changes that selection.

If an eligible subscription path is later admitted, it still needs a separately
reviewed harness selecting Subscription explicitly, no API-key names, a pinned
installed Linux CLI accessible to the isolated UID, and direct on-runner login
into an operator-owned store. Test actual refresh, expiry/revocation, workspace
restrictions, cross-UID denial, protected launch and owned cleanup without copying
workstation caches or uploading session/login logs. Those product/harness changes
are not silently included here. Existing API-key qualification paths instead need
separately eligible `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` credentials, which USER's
OpenRouter key and subscriptions do not supply.
