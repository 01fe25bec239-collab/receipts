# Review integration (Wave 2)

Review owns what "accepted" means. Source: `src/review/**`. This page covers the Wave 2 package: RI-W2-1..5 and RI-WIRE.

## Flow

1. Orchestration's ReviewPort adapter calls `ReviewController::on_attempt_finished` when an implementation attempt finishes. No user action is involved.
2. `build_review_capsule` turns the `ReviewRequest` into a `ReviewCapsule` for the exact subject. It reads the commit range and the diff digest itself through Workspace's hardened Git evidence API. Its only inputs are the request, the repository and the worker's mechanical checks. There is no input for the implementer's conversation, summary, notes, questions or self-assessment (compile-fail doctests).
3. Before anything runs, the controller takes a durable claim from the sink (see "Replay and persistence"). Then it asks `ReviewerRoutingPort` for a reviewer. Independence can only be raised: the assurance profile sets the floor and the request may raise it. The reviewer quality floor is derived the same way (BALANCED for LIGHT, FRONTIER otherwise, raised but never lowered by the request's `reviewer_floor`), and the candidate must carry quality evidence (tier plus a basis reference) that meets it. Missing evidence never counts as meeting a floor. The controller checks the tier and that a basis is named; it cannot authenticate the basis, which is Routing's evidence. A resumed, same-session or excluded-session reviewer is refused. `REQUIRED` refuses a reviewer from the implementer's provider.
4. It provisions a separate, task-bound review worktree at the implementation SHA (`receipts/review/<hex>`, Workspace decision D-9; not a detached HEAD). It dispatches once through `ReviewerRuntime`.
5. After the run it checks that the worktree did not move or get dirty. It reproduces every deterministic criterion and worker check with the Workspace argv runner. Only executables the composition admitted can run: no shell, no PATH lookup. It recomputes write scope. Then it parses the verdict strictly (below).
6. It seals one immutable package through `ReviewRecordSink` and then publishes it: the A4Review decision record, which State's shape validator checks, plus REVIEW_DISPATCHED, REVIEW_PASSED/REJECTED and one FINDING_RAISED event per finding (each carrying that finding), with their payload bytes. Review holds no State writer. The trusted composition persists through `CoreStateWriter`.

`ReviewTicket::passed()` is true only for a PASS that the controller attested, persisted and cleaned up after. PASS_WITH_NONBLOCKING_FINDINGS is a distinct verdict: it stays pending until every nonblocking finding has an explicit disposition (below), and REJECT never passes whatever its finding arrays hold.

## Replay and persistence

The sink contract (documented on `ReviewRecordSink`) is: `claim` is an atomic, durable compare-and-set keyed by the attempt; `seal` stores the exact package durably before anything is published and never replaces it; `publish` applies the sealed package completely and idempotently (an identical stored record or event, found by its stable event key, counts as done; a conflicting one is an error; nothing is replaced) and can be repeated after any failure or restart.

The controller acts on that contract. A repeated or concurrent call for the same attempt never routes, provisions, dispatches or reproduces again. It recovers the sealed disposition and finishes any unfinished publication, or refuses with `ClaimInFlight` when a claim exists without a sealed result (the earlier run may have reached the reviewer; recovery is an operator decision). A failure to claim executes nothing. A failure to seal or publish never leads to a second execution. POLICY_BLOCKED therefore survives repeated callbacks. Rejected records are never replaced.

`crates/receipts-e2e/tests/review_core_persistence.rs` proves this against the real Core writer with a durable file journal: a fault at each publish boundary, restart and retry, a REJECT preserved through a fault, six concurrent callers executing once, and a deliberately racy claim that the same harness catches. State's own shared atomic/claim support is another owner's pending work and is not integrated here; the journal is a test composition, not the production one.

## Never PASS

- Missing, truncated, ambiguous output (zero or several JSON documents), schema-invalid output, output for another review/task/SHA, `independence_attested=false`, PASS with findings, a PASS-class verdict with blocking findings, an audit dimension left unassessed (all twelve must be assessed; NOT_APPLICABLE counts), or a collection that was not protected. A REJECT with empty finding arrays is admitted by the schema and kept as a REJECT; it is recorded and never passes.
- A finding in a mandatory category (the eight-category assurance floor, a missing required negative test, or a category the capsule's severity policy lists) is blocking whatever the reviewer flagged: the controller moves it to the blocking array and the verdict is REJECT, before any ticket or REVIEW_PASSED exists.
- A reviewer that wrote to or committed in its worktree.
- A reproduction that failed, timed out or could not run. The controller adds a blocking UNREPRODUCIBLE_EVIDENCE finding and the verdict becomes REJECT. Changes outside `allowed_write_paths` add a WRITE_SCOPE_VIOLATION finding the same way.
- `PolicyBlocked` from routing or from the runtime. The result is a SafetyInterruption (POLICY_BLOCKED → HUMAN_REQUIRED) and nothing is dispatched to another provider.

The controller replaces the reviewer-asserted `reviewer`, `reproduction`, `reviewed_at` and `unauthorized_file_changes` with values it observed itself.

## Runtime boundary

`ReviewerRuntime` is implemented over the frozen `RuntimeAdapter::start` with `RuntimeCapsuleFamily::Review` and a read-only, protected-launch policy. `reviewer_output_from_claude` and `reviewer_output_from_codex` interpret the collected results. The Review crate has no `receipts-runtime-bindings` edge (manifests are fixed), so the bridge that names `RuntimeCapsuleFamily` lives in the composition. `crates/receipts-e2e/tests/review_runtime_boundary.rs` has one for both real adapters. It proves that an unprotected or writable Review policy is refused before any process starts, and that the controller does not shop for another provider.

## Acceptance gate (nine checks)

`AcceptanceGate::evaluate` runs all nine checks every time. Git facts (checks 1, 2, 6, 7, 8) are observed and checks 3 and 9 are re-executed at evaluation time. Checks 4 and 5 consume recorded evidence, which is not re-executed: the A4Review must equal the record the trusted core persisted, the capsule must be the one a persisted REVIEW_DISPATCHED payload digest names, the handoff's worker checks must be checks that capsule carries, and a record that contradicts itself (PASS with a nonzero exit code) is refused. A State reader is therefore required. A3Handoff is not a State record kind, so worker-check origin is the dispatched capsule, not an independent execution. IntegrationRequest attestations are never read.

| # | Check | Evidence | Failure |
|---|---|---|---|
| 1 | Reviewed SHA = implementation SHA, commit on the task branch, request names exactly it | records + `review_commit_range` to the live task HEAD | BLOCKED |
| 2 | No commit after review | live task HEAD via checkpoint capture; commits since review; clean | BLOCKED |
| 3 | Deterministic criteria re-run and pass; no unmet criterion reported | fresh review worktree + argv runner | REPAIR |
| 4 | PASS or PASS_WITH_NONBLOCKING_FINDINGS (with an explicit disposition for every nonblocking finding), blocking findings closed, repair history valid, records bound to State | existing `ExactShaAcceptanceGate`, persisted A4Review, REVIEW_DISPATCHED digest | REPAIR / BLOCKED / HUMAN_REQUIRED when the bound is exhausted |
| 5 | Checks the effective profile requires, at the SHA, right channel, PASS with exit 0, not timed out, carried by the dispatched capsule | records; profile floor is raise-only | BLOCKED; HUMAN_REQUIRED for the broker (RI-W7-1) or the security pipeline (RI-W5-3), which do not exist yet |
| 6 | Diff within `allowed_write_paths`, no forbidden match | `verify_write_scope` | REPAIR |
| 7 | Dependencies accepted, no contradictions, and their effect still present | `StateReader::provenance_chain`, ancestry, and per-path comparison of the dependency's accepted change with the candidate | BLOCKED |
| 8 | Candidate contains the current merge target | live target HEAD + ancestry | REPAIR |
| 9 | Component suite passes on the post-merge tree | `provision_temporary_merge` + argv runner | REPAIR (BLOCKED if it cannot run) |

Check 7 does not treat ancestry as proof: `git revert` keeps it. For each path the dependency changed (`start..implementation`), the candidate must hold the dependency's accepted content, or the path needs adopted supersession evidence: another task State shows as accepted whose own range changed the path, which is in the candidate, with no later change to the path. Anything else (a revert, an overwrite, the candidate's own rewrite, unprovable evidence, a missing start SHA) is uncertainty and cannot ACCEPT. It is path-level, reads no commit message, and does not judge intent. State has no supersession record kind, so the evidence is supplied as `DependencySupersession` and verified against State acceptance and Git.

Outcome precedence: HUMAN_REQUIRED > BLOCKED > REPAIR > ACCEPT. The gate returns an IntegrationDecision and an ACCEPTANCE_EVALUATED proposal. It holds no writer, edits nothing and merges nothing. Its temporary worktrees are owned and removed, and a removal failure is a failed check.

## Repairs

`RepairController::decide` uses the existing bounded cycle core (3 repairs, exact SHA and attested independence per pair). Finding-specific mandatory policy is applied first and can only raise the ordinary ladder: a security-boundary finding makes the frontier floor mandatory from repair 1 and escalates `distinct_provider` to REQUIRED; a security, contract or architecture (invariant) finding bars the reviewer that raised it (its session and provider/model) from grading the repair; a contract or architecture finding prefers a different model; a finder with no recorded identity cannot be excluded, so the repair escalates. Which categories count as "invariant" is this package's reading (ARCHITECTURE_VIOLATION) and is for A1 to confirm. Otherwise repair 1 runs at the same tier, repairs 2–3 at the frontier floor, and after that ESCALATED_TO_A2 and HUMAN_REQUIRED are produced (the A1 escalation step is governance outside this package). A SafetyInterruption is never turned into a repair. A `RepairRequest` lists blocking findings first, then policy-floor findings, then the rest, and carries every rejected SHA in order. Rejected A4Review records are immutable in State. Building the Orchestration RepairCapsule from a RepairRequest is later composition.

## Finding dispositions

A PASS_WITH_NONBLOCKING_FINDINGS review is accepted only with exactly one explicit disposition per nonblocking finding: fix now (a new bounded task id), defer (issue id and revisit point) or reject (a reason), with a decider. `disposition_proposal` validates the set and produces one FINDING_DISPOSITIONED event per finding; the gate's check 4 requires the same coverage. Nothing is invented or dropped silently.

## Codecs (RI-WIRE)

`receipts_review_integration::wire` has strict decode/encode for A3Handoff, A4Review, ReviewCapsule, ReviewRequest, IntegrationRequest, IntegrationDecision and SafetyInterruption. It uses its own small JSON reader:

- exact number tokens, so `1.0`, `1e3` and `-0` are integers;
- duplicate keys, lone surrogates and trailing data are refused;
- absent, `null` and `[]` stay distinct;
- carrier limits (`i64` exit codes, `u64` finding lines, 10,000-digit integers, `ReviewDateTimeV1` spellings) are reported as `UnsupportedValue`, never narrowed.

State's `DecisionRecordV1` validator acts as a second oracle for the four kinds State stores. The jsonschema oracle is not a dependency of this crate.

## CI

- `ci.yml`: `rust-validation` is unchanged. The new `macos-validation` job runs `cargo test --workspace --locked` on macOS 15.
- `security.yml` (`security-scan`): read-only token, no secrets, no `pull_request_target`. It first runs `selftest.sh` (hermetic stub tools, no network), then `acquire.sh`, which downloads cargo-audit 0.22.2, Gitleaks 8.30.1 and actionlint 1.7.12 and checks each archive's SHA-256 against `pins.env`, and observes the RustSec dataset once: origin, floor ancestry, commit, tree, content digest and the observation time. The adopted Q-06 window (24 hours from that observation) governs execution; commit age is not a rule, a commit dated after its observation is refused, and nothing renews or substitutes the snapshot. `run.sh` checks tool and dataset identity and the window at the start, before and after every audit (a scan must start and finish inside it), then runs negative controls (a generated planted secret, a known-vulnerable lockfile, an invalid workflow, and fifteen YAML spellings of safe and unsafe workflows) and scans the tested commit: cargo-audit with the adopted profile, Gitleaks over a complete tracked snapshot (every index entry of HEAD, written with `checkout-index` so `export-ignore` cannot hide a file; submodule entries are unsupported content and fail), the workflow trust policy evaluated on parsed YAML (`workflow_policy.rb`: block and inline forms, quoted keys, anchors), and actionlint.
- Every external command is bounded: `timeout -k` (TERM, then KILL after 10 s) in its own process group, waited for, including fetches, extraction and version commands. A TERM/INT/HUP to a script reaps the in-flight command, removes the planted inputs and records the run as incomplete. A process that leaves its group on purpose (setsid) is outside this guarantee.
- Evidence is retained and uploaded even on failure: the frozen configurations, control definitions, raw reports and stderr, per-step argv/UTC/exit, tool and dataset identities, the tracked-snapshot manifest, a summary and SHA256SUMS. Planted secrets are never retained: the reports are redacted, and a retained planted value fails the run and removes the file. Removing the planted inputs is separate, verified, and an unremovable input fails the run.

Both new jobs are advisory until A1 adds them to branch protection. No workflow changes required checks on a timer. Scanner results are components of a security review, not a defensive verdict. The scripts are part of the PR under test, so they are not an independent authority.

## Not proven here

- A successful protected Review launch (needs the Linux protected launcher), and any real provider review or provider eligibility.
- Hosted runs of `macos-validation` and `security-scan`, real actionlint, and the download/extraction/clone acquisition paths. `selftest.sh` uses stub scanners: it proves the scripts' behaviour, not scanner detection.
- Reviewer quality: the controller checks the evidenced tier against the derived floor, but the evidence's basis is Routing's and is not authenticated here.
- State's shared atomic/claim support is not integrated; the durable journal used in the controls is a test composition.
- The scheduler/real-matcher integration with Orchestration is not done (dependency not admitted).
- Worker-check origin is the dispatched capsule: A3Handoff is not a State record, and no execution of worker checks is independently proven.
