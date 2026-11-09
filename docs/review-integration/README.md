# Review integration (Wave 2)

Review owns what "accepted" means. Source: `src/review/**`. This page covers the Wave 2 package: RI-W2-1..5 and RI-WIRE.

## Flow

1. Orchestration's ReviewPort adapter calls `ReviewController::on_attempt_finished` when an implementation attempt finishes. No user action is involved.
2. `build_review_capsule` turns the `ReviewRequest` into a `ReviewCapsule` for the exact subject. It reads the commit range and the diff digest itself through Workspace's hardened Git evidence API. Its only inputs are the request, the repository and the worker's mechanical checks. There is no input for the implementer's conversation, summary, notes, questions or self-assessment (compile-fail doctests).
3. Before anything runs, the controller takes a durable claim from the sink for a typed scope (see "Claim identity" and "Replay and persistence"). Then it asks `ReviewerRoutingPort` for a reviewer. Independence can only be raised: the assurance profile sets the floor and the request may raise it. The reviewer quality floor is derived the same way (BALANCED for LIGHT, FRONTIER otherwise, raised but never lowered by the request's `reviewer_floor`), and the candidate must carry quality evidence (tier plus a basis reference) that meets it. Missing evidence never counts as meeting a floor. The controller checks the tier and that a basis is named; it cannot authenticate the basis, which is Routing's evidence. A resumed, same-session or excluded-session reviewer is refused. `REQUIRED` refuses a reviewer from the implementer's provider.
4. It provisions a separate, task-bound review worktree at the implementation SHA (`receipts/review/<hex>`, Workspace decision D-9; not a detached HEAD). It dispatches once through `ReviewerRuntime`.
5. After the run it checks that the worktree did not move or get dirty. It reproduces every deterministic criterion and worker check with the Workspace argv runner. Only executables the composition admitted can run: no shell, no PATH lookup. It recomputes write scope. Then it parses the verdict strictly (below).
6. It seals one immutable package through `ReviewRecordSink` and then publishes it: the A4Review decision record, which State's shape validator checks, plus REVIEW_DISPATCHED, REVIEW_PASSED/REJECTED and one FINDING_RAISED event per finding (each carrying that finding), with their payload bytes. Review holds no State writer. The trusted composition persists through `CoreStateWriter`.

`ReviewTicket::passed()` is true only for a PASS that the controller attested, persisted and cleaned up after. PASS_WITH_NONBLOCKING_FINDINGS is a distinct verdict: it stays pending until every nonblocking finding has an explicit disposition (below), and REJECT never passes whatever its finding arrays hold.

## Claim identity

Every unit of Review work that reaches the trusted core is one claim with an explicit, typed identity. Nothing is parsed back out of a hash and no in-memory map is authority.

- `ReviewClaimScope` is project, task, optional subject SHA, request SHA-256 and a positive generation: exactly what the core's `CoreReviewUnitV1::new` needs. The constructor refuses a zero generation, a SHA that is not 40 lowercase hex digits, a digest that is not 64, and unbounded ids.
- The request SHA-256 covers the actual validated semantic request under a deterministic encoding and the `ReviewOperation` (`REVIEW`, `REPAIR`, `DISPOSITION`, `ACCEPTANCE`), length-prefixed and domain-separated. A request id alone never identifies work. The operation is folded into this digest because the core's claim id has no operation field, so a review, a repair decision, a disposition set and an acceptance decision for one task and SHA are four claims.
  - Review: the request's canonical encoding without `requested_at` (when, not what: a redelivered callback names the same work), the implementer identity and every worker check.
  - Repair: the same request, every retained review and handoff, the completed repair count and any interruption.
  - Disposition: the request, the review and every disposition in finding order.
  - Acceptance: the review request, the integration request and the decision id.
- `ReviewClaimBinding` is operation plus scope plus the opaque claim key, derived from both. `ReviewClaimBinding::from_parts` accepts a key only if it re-derives, so a binding rebuilt from sealed bytes cannot drift from its scope or operation.
- `generation` comes from the trusted composition: `AttemptFinished::generation`, `RepairInput::generation`, `AcceptanceInput::generation`, the `generation` argument of `disposition_proposal`. Review never increments it. A new generation is the explicit way to run abandoned work again.
- A scope that cannot be formed (for example generation 0) refuses before any claim, routing or dispatch (`ReviewRefusal::Scope`). A sealed package whose own binding is not the claim's is never published (`ReviewRefusal::SealedScopeMismatch`).

## Replay and persistence

The sink contract (documented on `ReviewRecordSink`) names every call by the typed binding: `claim` is an atomic, durable compare-and-set; `seal` stores the exact package durably before anything is published, refuses another scope's package and never replaces a seal; `publish` applies the sealed package completely and idempotently, refuses a package that is not the sealed one, and can be repeated after any failure or restart. `claim` answers `Acquired` (the only answer that permits execution), `InFlight`, `Sealed`, `Terminal` or `Abandoned`.

The controller acts on that contract. A repeated or concurrent call never routes, provisions, dispatches or reproduces again. It recovers the sealed disposition and finishes any unfinished publication, or refuses: `ClaimInFlight` when a claim exists without a sealed result (the earlier run may have reached the reviewer), `ClaimTerminal` and `ClaimAbandoned` for finished or abandoned claims. A failure to claim executes nothing. A failure to seal or publish never leads to a second execution. POLICY_BLOCKED therefore survives repeated callbacks. Rejected records are never replaced.

The sealed recovery bytes are `SealedReview::to_bytes()` (State kind `REVIEW_SEALED_V1`): outcome label, cleanup flag, dispatch count and the package journal with its digest. `SealedReview::from_bytes` rebuilds the package through State's validators and refuses it unless the binding re-derives, the outcome label belongs to the binding's operation, the package holds the decision records that outcome needs, the record is for the claim's task and subject, and each record equals the event payload that names it. `settlement()` derives the claim disposition from that package alone: `REVIEWED` completes with the verdict (`PASS`, `PASS_WITH_NONBLOCKING_FINDINGS`, `REJECT`), the review id and the record digest; `INVALID_OUTPUT`, the repair, disposition and acceptance outcomes complete under their label; `POLICY_BLOCKED` and `REFUSED` are retained terminal refusals (`Refused`).

Repair, disposition and acceptance results are pure proposals; they reach State through the same sink with `RepairOutcome::record`, `record_dispositions` and `GateOutcome::record` (all `record_proposal`: claim, seal, publish). A repair that is not needed holds nothing and claims nothing. State refuses to publish an identical event a second time, so a new generation reruns work that never published; it cannot duplicate a published fact (the second publication is refused with nothing written and its claim stays sealed and unfinished).

## Real composition (Review → Core → State)

`receipts_e2e::CoreReviewSink` is the library-integration binding of `ReviewRecordSink` to `CoreStateWriter` and real SQLite State (`crates/receipts-e2e/src/review_core_sink.rs`). The Review crate does not depend on Orchestration; the dependency is the other way, in the integration layer. It keeps no durable state of its own.

- `claim` builds `CoreReviewUnitV1` from the binding's scope and calls `claim_review`. A sealed claim is returned from State's stored bytes, decoded and re-validated against the requested binding.
- `seal` passes the exact `SealedReview::to_bytes()` bytes to `seal_review` with `ReviewSealedCodec` for that binding. The codec is Review's `CoreRecoveryCodecV1`: it decodes the bytes into `CorePublicationInputV1` (decision records, events with their stable keys, no availability facts) and the claim settlement, and refuses any other scope, operation, project or malformed bytes. Core resolves every event payload through `SealedContent`, hashes it against the digest the event names, and State stores the bytes before anything is published.
- `publish` takes no package from the caller. It reads the seal State holds, refuses a `proposal` that is not that package, and calls `publish_sealed_review`, which rebuilds the package from the seal and publishes records, events and the claim settlement in one State transaction. Live and after a restart it is the same call; the reviewer is never run. An exact retry replays the recorded receipt.
- `abandon` ends an unfinished claim under an authority the writer's admitted identity names (a grant minted by that identity for exactly that claim; a name alone, another identity or an unadmitted project is refused). `claim_id(binding)` is the id the composition needs to mint the grant. Abandonment runs nothing; the work then needs a new generation.

Event ids are derived by the core from the publication and the event key. State lists them in identity order, not publication order, and every event of a package carries the trusted clock's timestamp: consumers compare event sets, not their listing order. The acceptance gate does.

Controls (real temporary SQLite stores; a restart is a fresh `CoreStateWriter` over the same file):

- `review_core_persistence.rs`: the full path with the claim, seal and settlement read back from State; faults at the sink and inside State's write path (the trusted clock failing during claim, seal and publish) with restart and retry and no second execution; the unsealed-claim path (in flight, across a restart, wrong-identity and wrong-claim grants refused, authorized abandonment, `ClaimAbandoned`, a new generation running anew); a claim that cannot be taken executing nothing; a REJECT preserved through a fault; POLICY_BLOCKED and REFUSED retained and never re-routed; six concurrent callers executing once, and a sink without exclusion that the same harness catches; conflicting scope, package and label refused with nothing written, and the first seal standing; tampered, truncated and foreign recovery bytes and wrong content refused at the core boundary.
- `review_core_paths.rs`: repair decisions (issued and escalated) and finding dispositions through the same sink, with a fault, restart and replay.
- `review_core_acceptance.rs`: the gate reads the Core-published A4Review and REVIEW_DISPATCHED event, accepts on all nine checks, and its IntegrationDecision publishes through the sink; a review whose publication was lost is refused by the gate.
- `review_runtime_boundary.rs`: the real Runtime adapters' policy refusal is persisted through the same sink.

## Scheduler against the real matcher

`crates/receipts-e2e/tests/scheduler_real_matcher.rs` calls `schedule_dispatch_v1` with `WorkspaceWriteScopeMatcher` (Workspace's `WritePathPattern` and `intersects`) and all-pass D11 admission inputs built from the public types: an in-flight overlap defers with a witness both patterns accept; a disjoint candidate is admitted; same-round overlap admits the first and defers the second against it; same-round disjoint writers are both admitted; an invalid candidate scope is `WriteScopeUnvalidated`, even without peers; an invalid in-flight scope remains `WriteOverlapUndetermined`; 400 x 400 disjoint literals exhaust the engine's work limit and defer the later writer both against in-flight work and between two candidates, with a small-size control that is admitted.

Every eligible candidate is validated through `WriteScopePort::validate` before admission. The real adapter parses its complete nonempty scope with Workspace. Invalid or unvalidated scopes defer without consuming capacity or becoming writers. Validation is separate from pairwise intersection: valid large scopes can still produce `WriteOverlapUndetermined` when the intersection engine exhausts its work limit. Controls cover lone valid/invalid candidates, mixed scopes, and an invalid first candidate followed by valid work; a Core control gives that pair only one worker, one cost unit and one concurrency slot.

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

Outcome precedence: HUMAN_REQUIRED > BLOCKED > REPAIR > ACCEPT. The gate returns an IntegrationDecision and an ACCEPTANCE_EVALUATED proposal (`GateOutcome::record` publishes it through the sink). It holds no writer, edits nothing and merges nothing. Its temporary worktrees are owned and removed, and a removal failure is a failed check.

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
- Library composition only: no production Host dispatch, live provider or privileged execution, and no composition-root wiring of `CoreReviewSink` (the later composition task owns it).
- A process that crashes between the external reviewer run and the seal commit leaves the claim in flight; this is not resolved automatically because an external execution and a SQLite commit are not one transaction. It needs authorized abandonment and a new generation.
- Fault injection inside State's transaction is State's own (its in-crate boundary controls). Here faults are injected at the sink and through State's trusted clock, which every State write samples first: a clock failure refuses that write with nothing stored, and the controls then check the stored image and recover.
- The `Terminal` claim state (a plain settlement with nothing sealed) is produced by the core's `settle_review_refusal`, which this sink never calls; if another writer produces it, the controller refuses (`ClaimTerminal`) and runs nothing (covered).
- The scheduler admits a lone invalid write pattern (see above).
- Worker-check origin is the dispatched capsule: A3Handoff is not a State record, and no execution of worker checks is independently proven.
