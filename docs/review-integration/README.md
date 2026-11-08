# Review integration (Wave 2)

Review owns what "accepted" means. Source: `src/review/**`. This page covers the Wave 2 package: RI-W2-1..5 and RI-WIRE.

## Flow

1. Orchestration's ReviewPort adapter calls `ReviewController::on_attempt_finished` when an implementation attempt finishes. No user action is involved.
2. `build_review_capsule` turns the `ReviewRequest` into a `ReviewCapsule` for the exact subject. It reads the commit range and the diff digest itself through Workspace's hardened Git evidence API. Its only inputs are the request, the repository and the worker's mechanical checks. There is no input for the implementer's conversation, summary, notes, questions or self-assessment (compile-fail doctests).
3. The controller asks `ReviewerRoutingPort` for a reviewer. Independence can only be raised: the assurance profile sets the floor and the request may raise it. A resumed, same-session or excluded-session reviewer is refused. `REQUIRED` refuses a reviewer from the implementer's provider.
4. It provisions a separate, task-bound review worktree at the implementation SHA (`receipts/review/<hex>`, Workspace decision D-9; not a detached HEAD). It dispatches once through `ReviewerRuntime`.
5. After the run it checks that the worktree did not move or get dirty. It reproduces every deterministic criterion and worker check with the Workspace argv runner. Only executables the composition admitted can run: no shell, no PATH lookup. It recomputes write scope. Then it parses the verdict strictly (below).
6. It proposes immutable records through `ReviewRecordSink`: the A4Review decision record, which State's shape validator checks, plus REVIEW_DISPATCHED, REVIEW_PASSED/REJECTED and FINDING_RAISED proposals with their payload bytes. Review holds no State writer. The trusted composition persists through `CoreStateWriter`.

`ReviewTicket::passed()` is true only for a passing verdict that the controller attested, persisted and cleaned up after.

## Never PASS

- Missing, truncated, ambiguous output (zero or several JSON documents), schema-invalid output, output for another review/task/SHA, `independence_attested=false`, PASS with findings in the wrong array, REJECT without a blocking finding, or a collection that was not protected.
- A reviewer that wrote to or committed in its worktree.
- A reproduction that failed, timed out or could not run. The controller adds a blocking UNREPRODUCIBLE_EVIDENCE finding and the verdict becomes REJECT. Changes outside `allowed_write_paths` add a WRITE_SCOPE_VIOLATION finding the same way.
- `PolicyBlocked` from routing or from the runtime. The result is a SafetyInterruption (POLICY_BLOCKED → HUMAN_REQUIRED) and nothing is dispatched to another provider.

The controller replaces the reviewer-asserted `reviewer`, `reproduction`, `reviewed_at` and `unauthorized_file_changes` with values it observed itself.

## Runtime boundary

`ReviewerRuntime` is implemented over the frozen `RuntimeAdapter::start` with `RuntimeCapsuleFamily::Review` and a read-only, protected-launch policy. `reviewer_output_from_claude` and `reviewer_output_from_codex` interpret the collected results. The Review crate has no `receipts-runtime-bindings` edge (manifests are fixed), so the bridge that names `RuntimeCapsuleFamily` lives in the composition. `crates/receipts-e2e/tests/review_runtime_boundary.rs` has one for both real adapters. It proves that an unprotected or writable Review policy is refused before any process starts, and that the controller does not shop for another provider.

## Acceptance gate (nine checks)

`AcceptanceGate::evaluate` runs all nine checks every time and observes or re-executes each fact at evaluation time. IntegrationRequest attestations are never read.

| # | Check | Evidence | Failure |
|---|---|---|---|
| 1 | Reviewed SHA = implementation SHA, commit on the task branch, request names exactly it | records + `review_commit_range` to the live task HEAD | BLOCKED |
| 2 | No commit after review | live task HEAD via checkpoint capture; commits since review; clean | BLOCKED |
| 3 | Deterministic criteria re-run and pass; no unmet criterion reported | fresh review worktree + argv runner | REPAIR |
| 4 | PASS verdict, blocking findings closed, repair history valid | existing `ExactShaAcceptanceGate` | REPAIR / BLOCKED / HUMAN_REQUIRED when the bound is exhausted |
| 5 | Checks the effective profile requires, at the SHA, right channel, PASS, not timed out | records; profile floor is raise-only | BLOCKED; HUMAN_REQUIRED for the broker (RI-W7-1) or the security pipeline (RI-W5-3), which do not exist yet |
| 6 | Diff within `allowed_write_paths`, no forbidden match | `verify_write_scope` | REPAIR |
| 7 | Dependencies accepted, no contradictions, not reverted | `StateReader::provenance_chain` + ancestry | BLOCKED |
| 8 | Candidate contains the current merge target | live target HEAD + ancestry | REPAIR |
| 9 | Component suite passes on the post-merge tree | `provision_temporary_merge` + argv runner | REPAIR (BLOCKED if it cannot run) |

Outcome precedence: HUMAN_REQUIRED > BLOCKED > REPAIR > ACCEPT. The gate returns an IntegrationDecision and an ACCEPTANCE_EVALUATED proposal. It holds no writer, edits nothing and merges nothing. Its temporary worktrees are owned and removed, and a removal failure is a failed check.

## Repairs

`RepairController::decide` uses the existing bounded cycle core (3 repairs, exact SHA and attested independence per pair). Repair 1 runs at the same tier, repairs 2–3 at the frontier floor, and after that the task needs a human. A SafetyInterruption is never turned into a repair. A `RepairRequest` lists blocking findings first, then policy-floor findings, then the rest, and carries every rejected SHA in order. Rejected A4Review records are immutable in State. Building the Orchestration RepairCapsule from a RepairRequest is later composition.

## Codecs (RI-WIRE)

`receipts_review_integration::wire` has strict decode/encode for A3Handoff, A4Review, ReviewCapsule, ReviewRequest, IntegrationRequest, IntegrationDecision and SafetyInterruption. It uses its own small JSON reader:

- exact number tokens, so `1.0`, `1e3` and `-0` are integers;
- duplicate keys, lone surrogates and trailing data are refused;
- absent, `null` and `[]` stay distinct;
- carrier limits (`i64` exit codes, `u64` finding lines, 10,000-digit integers, `ReviewDateTimeV1` spellings) are reported as `UnsupportedValue`, never narrowed.

State's `DecisionRecordV1` validator acts as a second oracle for the four kinds State stores. The jsonschema oracle is not a dependency of this crate.

## CI

- `ci.yml`: `rust-validation` is unchanged. The new `macos-validation` job runs `cargo test --workspace --locked` on macOS 15.
- `security.yml` (`security-scan`): read-only token, no secrets, no `pull_request_target`. `.github/scripts/security/acquire.sh` downloads cargo-audit 0.22.2, Gitleaks 8.30.1 and actionlint 1.7.12 and checks each archive's SHA-256 against `pins.env`. It captures the RustSec dataset once and requires it to descend from the last Q-06-verified commit and to be under 14 days old. `run.sh` runs negative controls first: a generated planted secret, a known-vulnerable lockfile (RUSTSEC-2021-0003), an invalid workflow, and an unsafe workflow (trigger, write, secret, unpinned action, persisted credentials). Each control must detect its problem. Then it scans the tested commit: cargo-audit with the adopted profile, Gitleaks over the tracked snapshot, the workflow trust policy and actionlint. Any failure, timeout, missing tool, stale dataset or cleanup failure fails the job.

Both new jobs are advisory until A1 adds them to branch protection. No workflow changes required checks on a timer. Scanner results are components of a security review, not a defensive verdict.

## Not proven here

- A successful protected Review launch (needs the Linux protected launcher), and any real provider review or provider eligibility.
- Hosted runs of `macos-validation` and `security-scan`, and actionlint locally.
- Reviewer quality floor: Routing applies it. The candidate carries no tier the controller could check.
- Persisting one proposal with several records is not one transaction across records. A replayed attempt dispatches again before the core refuses its duplicate records.
- FINDING_DISPOSITIONED has no producer in this package.
