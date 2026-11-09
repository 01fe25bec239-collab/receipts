//! RI-W2-2: the automatic A3→A4 review controller.
//!
//! [`ReviewController::on_attempt_finished`] is what Orchestration's
//! ReviewPort adapter calls when an implementation attempt finishes; no user
//! action is involved. It builds the exact-subject capsule, asks
//! [`ReviewerRoutingPort`] for a reviewer under the raised-only
//! independence policy, provisions a separate task-bound review worktree at
//! the implementation SHA, dispatches exactly once through
//! [`ReviewerRuntime`], verifies the worktree did not move, reproduces the
//! deterministic checks itself, parses the verdict strictly, and proposes
//! immutable records for the trusted core.
//!
//! Refusal rules:
//! - a resumed, same-session or excluded-session reviewer is refused;
//! - `distinct_provider = REQUIRED` refuses a same-provider reviewer;
//! - `PolicyBlocked` from routing or the runtime produces a
//!   SafetyInterruption with `HUMAN_REQUIRED` and is never re-dispatched to
//!   another provider (no provider-shopping);
//! - missing, truncated, unparseable or inconsistent output, a moved or
//!   dirty review worktree, or a non-protected collection is never PASS.
//!
//! The controller holds no State writer; records go through
//! [`ReviewRecordSink`]. Reviewer-asserted `reviewer`, `reproduction`,
//! `reviewed_at` and `unauthorized_file_changes` are replaced by values the
//! controller observed.
//!
//! Replay safety: a durable claim is taken from the sink BEFORE routing or
//! dispatch. Only the caller that acquires it executes; the result package is
//! sealed durably before any record is published, and publication is
//! idempotent and resumable. A repeated or concurrent call for the same
//! attempt recovers the recorded disposition (and finishes any unfinished
//! publication) or refuses; it never routes, provisions, dispatches or
//! reproduces again, and a persistence failure never leads to a second
//! execution.
//!
//! Policy is applied before any success is produced: the reviewer quality
//! floor is derived raise-only and must be met by Routing's evidence;
//! findings in a mandatory category are blocking whatever the reviewer
//! flagged (the verdict becomes REJECT); every audit dimension must be
//! assessed; a PASS_WITH_NONBLOCKING_FINDINGS ticket is not passing until
//! each nonblocking finding has an explicit disposition.

use crate::capsule_builder::{CapsuleBuildRefusal, ReviewCapsuleBuildInput, build_review_capsule};
use crate::ports::{
    ImplementerIdentity, ReviewClaim, ReviewRecordProposal, ReviewRecordSink, ReviewerCandidate,
    ReviewerRequirements, ReviewerRoutingPort, ReviewerRoutingRefusal, ReviewerRuntime,
    SealedReview, reviewer_tier_rank,
};
use crate::records::{ProposalBuilder, derived_id};
use crate::reproduction::{CheckRunStatus, CheckRunnerConfig, run_check};
use crate::review_parse::parse_reviewer_output;
use crate::wire::{
    decode_a4_review, encode_a4_review, encode_review_capsule, encode_safety_interruption,
};
use crate::{
    A4Review, A4ReviewFinding, A4ReviewFindingCategory, A4ReviewFindingConfidence,
    A4ReviewFindingSeverity, A4ReviewFindingSource, A4ReviewNonTemporalCore, A4ReviewReproduction,
    A4ReviewReproductionCheck, A4ReviewReproductionCheckResult, A4ReviewReproductionLimitation,
    A4ReviewReviewer, A4ReviewVerdict, AssuranceReviewerQualityFloor, DistinctProviderPolicy,
    ReviewCapsuleCheck, ReviewCapsuleCheckResult, ReviewCapsuleCriterionKind,
    ReviewCapsuleNonTemporalCore, ReviewDateTimeV1, ReviewRequest, ReviewRequestReviewerFloor,
    SafetyInterruption, SafetyInterruptionDetectionConfidence, SafetyInterruptionNonTemporalCore,
    SafetyInterruptionState, SafetyInterruptionTerminalOutcome,
    is_assurance_profile_blocking_floor,
};
use receipts_runtime_adapters::FailureClass;
use receipts_state::{DecisionRecordKind, EventType, SubjectKind};
use receipts_workspace_execution::{
    ReviewWorkspace, ReviewWorkspaceRequest, WorkspaceCheckpointCheckSource,
    provision_review_workspace, remove_review_workspace, verify_review_workspace,
    verify_write_scope,
};
use std::path::Path;

/// One finished implementation attempt, as Orchestration reports it.
#[derive(Debug, Clone, Copy)]
pub struct AttemptFinished<'a> {
    pub project_id: &'a str,
    pub request: &'a ReviewRequest,
    pub repository_root: &'a Path,
    pub implementer: &'a ImplementerIdentity,
    /// Mechanical worker check evidence (never conversation or summaries).
    pub worker_checks: &'a [ReviewCapsuleCheck],
    /// Fresh absolute path for the separate review worktree.
    pub review_worktree: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRefusal {
    Capsule(CapsuleBuildRefusal),
    NoEligibleReviewer,
    /// Routing offered a resumed session; reviews are always fresh.
    ResumeRefused,
    /// The reviewer session is the implementer's or an excluded one.
    SameSession,
    DistinctProviderRequired,
    /// Routing named no quality evidence for the selected reviewer: missing
    /// evidence never counts as meeting a floor.
    QualityEvidenceMissing,
    /// The evidenced tier is below the effective floor.
    QualityBelowFloor {
        required: ReviewRequestReviewerFloor,
        evidenced: ReviewRequestReviewerFloor,
    },
    UnknownRuntime(String),
    Workspace(String),
    /// The durable claim could not be taken or the package not persisted.
    Records(String),
    /// An earlier or concurrent run holds the claim without a sealed
    /// result; nothing is executed again (operator recovery).
    ClaimInFlight,
}

/// Terminal review disposition. Only `Reviewed` with a passing verdict can
/// feed acceptance; everything else routes to repair or a human.
#[derive(Debug, Clone)]
pub enum ReviewDisposition {
    Reviewed(A4Review),
    /// Dispatched but unusable output or reviewer-side violation.
    InvalidOutput(String),
    /// Policy refusal; never re-dispatched elsewhere.
    PolicyBlocked(SafetyInterruption),
    /// Nothing was dispatched.
    Refused(ReviewRefusal),
    /// An earlier run's sealed result, recovered without any execution.
    Replayed(Box<ReplayedReview>),
}

/// What an earlier, sealed run recorded.
#[derive(Debug, Clone)]
pub struct ReplayedReview {
    /// `REVIEWED`, `POLICY_BLOCKED`, `INVALID_OUTPUT` or `REFUSED`.
    pub outcome: String,
    /// The recorded review, when the original run produced one.
    pub review: Option<A4Review>,
    /// The original run's owned worktree cleanup succeeded.
    pub cleanup_ok: bool,
}

#[derive(Debug, Clone)]
pub struct ReviewTicket {
    pub capsule: Option<ReviewCapsuleNonTemporalCore>,
    pub disposition: ReviewDisposition,
    /// Reviewer dispatches performed: 0 or 1, never more.
    pub dispatches: usize,
    /// Owned review-worktree cleanup, when one was provisioned.
    pub cleanup: Option<Result<(), String>>,
    /// Sink result for the record proposal.
    pub persisted: Result<(), String>,
    pub proposal: Option<ReviewRecordProposal>,
}

impl ReviewTicket {
    /// The review this ticket speaks for: its own, or the recovered one.
    pub fn review(&self) -> Option<&A4Review> {
        match &self.disposition {
            ReviewDisposition::Reviewed(r) => Some(r),
            ReviewDisposition::Replayed(p) => p.review.as_ref(),
            _ => None,
        }
    }

    /// Nonblocking findings that still need an explicit disposition.
    pub fn pending_dispositions(&self) -> Vec<String> {
        match self.review() {
            Some(r) if r.core().verdict() == A4ReviewVerdict::PassWithNonblockingFindings => r
                .core()
                .nonblocking_findings()
                .iter()
                .map(|f| f.finding_id().to_owned())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// True only for a persisted, cleaned-up, controller-attested PASS.
    /// PASS_WITH_NONBLOCKING_FINDINGS is a distinct verdict that stays
    /// pending until [`crate::disposition_proposal`] records an explicit
    /// disposition for every finding (the acceptance gate enforces the same
    /// coverage), and REJECT never passes, whatever its finding arrays hold.
    pub fn passed(&self) -> bool {
        let clean = match &self.disposition {
            ReviewDisposition::Reviewed(_) => matches!(self.cleanup, Some(Ok(()))),
            ReviewDisposition::Replayed(p) => p.cleanup_ok,
            _ => false,
        };
        self.review().is_some_and(|r| {
            matches!(
                r.core().verdict(),
                A4ReviewVerdict::Pass | A4ReviewVerdict::PassWithNonblockingFindings
            )
        }) && self.pending_dispositions().is_empty()
            && self.persisted.is_ok()
            && clean
    }
}

pub struct ReviewController<'a> {
    pub routing: &'a dyn ReviewerRoutingPort,
    pub runtimes: &'a [&'a dyn ReviewerRuntime],
    pub checks: &'a CheckRunnerConfig,
    pub clock: &'a dyn Fn() -> ReviewDateTimeV1,
}

/// Raised-only independence: OFF < PREFERRED < REQUIRED.
pub fn effective_distinct_provider(request: &ReviewRequest) -> DistinctProviderPolicy {
    let floor = request
        .core()
        .assurance_profile()
        .requirements()
        .distinct_provider;
    match request
        .core()
        .review_policy()
        .and_then(|p| p.distinct_provider())
    {
        Some(asked) if asked.rank() > floor.rank() => asked,
        _ => floor,
    }
}

/// Raise-only reviewer quality floor: the assurance profile's minimum
/// (BALANCED for LIGHT, FRONTIER otherwise), raised but never lowered by the
/// request's own `reviewer_floor`.
pub fn effective_reviewer_floor(request: &ReviewRequest) -> ReviewRequestReviewerFloor {
    let profile = match request
        .core()
        .assurance_profile()
        .requirements()
        .reviewer_quality_floor
    {
        AssuranceReviewerQualityFloor::Balanced => ReviewRequestReviewerFloor::Balanced,
        AssuranceReviewerQualityFloor::Frontier => ReviewRequestReviewerFloor::Frontier,
    };
    match request
        .core()
        .review_policy()
        .and_then(|p| p.reviewer_floor())
    {
        Some(asked) if reviewer_tier_rank(asked) > reviewer_tier_rank(profile) => asked,
        _ => profile,
    }
}

fn requirements(attempt: &AttemptFinished<'_>) -> ReviewerRequirements {
    let distinct = effective_distinct_provider(attempt.request);
    let policy = attempt.request.core().review_policy();
    let mut excluded: Vec<String> = policy
        .and_then(|p| p.excluded_session_refs())
        .map(<[String]>::to_vec)
        .unwrap_or_default();
    excluded.extend(attempt.implementer.session_ref.clone());
    ReviewerRequirements {
        distinct_provider: distinct,
        reviewer_floor: effective_reviewer_floor(attempt.request),
        excluded_providers: if distinct == DistinctProviderPolicy::Required {
            vec![attempt.implementer.provider_id.clone()]
        } else {
            Vec::new()
        },
        excluded_session_refs: excluded,
    }
}

fn independence(
    candidate: &ReviewerCandidate,
    req: &ReviewerRequirements,
    implementer: &ImplementerIdentity,
) -> Result<(), ReviewRefusal> {
    if let Some(session) = &candidate.resume_session {
        return Err(if req.excluded_session_refs.contains(session) {
            ReviewRefusal::SameSession
        } else {
            ReviewRefusal::ResumeRefused
        });
    }
    if req.distinct_provider == DistinctProviderPolicy::Required
        && (candidate.provider_id == implementer.provider_id
            || req.excluded_providers.contains(&candidate.provider_id))
    {
        return Err(ReviewRefusal::DistinctProviderRequired);
    }
    match &candidate.quality {
        Some(q) if !q.basis_ref.trim().is_empty() => {
            if reviewer_tier_rank(q.tier) < reviewer_tier_rank(req.reviewer_floor) {
                return Err(ReviewRefusal::QualityBelowFloor {
                    required: req.reviewer_floor,
                    evidenced: q.tier,
                });
            }
        }
        _ => return Err(ReviewRefusal::QualityEvidenceMissing),
    }
    Ok(())
}

/// Durable identity of one finished attempt's review.
pub fn review_claim_key(attempt: &AttemptFinished<'_>) -> String {
    let core = attempt.request.core();
    derived_id(
        "review-claim",
        &[
            attempt.project_id,
            core.request_id(),
            core.task_id(),
            core.attempt_id(),
            core.implementation_sha().as_str(),
        ],
    )
}

fn refused_ticket(refusal: ReviewRefusal, persisted: Result<(), String>) -> ReviewTicket {
    ReviewTicket {
        capsule: None,
        disposition: ReviewDisposition::Refused(refusal),
        dispatches: 0,
        cleanup: None,
        persisted,
        proposal: None,
    }
}

impl ReviewController<'_> {
    pub fn on_attempt_finished(
        &self,
        attempt: AttemptFinished<'_>,
        sink: &mut dyn ReviewRecordSink,
    ) -> ReviewTicket {
        let key = review_claim_key(&attempt);
        match sink.claim(&key) {
            // No claim: nothing was executed, nothing is retried elsewhere.
            Err(e) => refused_ticket(ReviewRefusal::Records(e.clone()), Err(e)),
            Ok(ReviewClaim::InFlight) => refused_ticket(
                ReviewRefusal::ClaimInFlight,
                Err(
                    "a claim exists without a sealed result; recovery is an operator decision"
                        .into(),
                ),
            ),
            Ok(ReviewClaim::Sealed(sealed)) => self.recover(&key, &sealed, sink),
            Ok(ReviewClaim::Acquired) => self.execute(&key, attempt, sink),
        }
    }

    /// Completes an earlier run's publication from its sealed package and
    /// returns its recorded disposition. Executes nothing.
    fn recover(
        &self,
        key: &str,
        sealed: &SealedReview,
        sink: &mut dyn ReviewRecordSink,
    ) -> ReviewTicket {
        let rebuilt = sealed.proposal();
        let review = rebuilt.as_ref().ok().and_then(|p| {
            p.decision_records
                .iter()
                .find(|r| r.kind() == DecisionRecordKind::A4Review)
                .and_then(|r| decode_a4_review(r.record_json()).ok())
        });
        let persisted = match &rebuilt {
            Ok(p) => sink.publish(key, p),
            Err(e) => Err(e.clone()),
        };
        ReviewTicket {
            capsule: None,
            disposition: ReviewDisposition::Replayed(Box::new(ReplayedReview {
                outcome: sealed.outcome.clone(),
                review,
                cleanup_ok: sealed.cleanup_ok,
            })),
            dispatches: 0,
            cleanup: None,
            persisted,
            proposal: rebuilt.ok(),
        }
    }

    fn execute(
        &self,
        key: &str,
        attempt: AttemptFinished<'_>,
        sink: &mut dyn ReviewRecordSink,
    ) -> ReviewTicket {
        let mut ticket = ReviewTicket {
            capsule: None,
            disposition: ReviewDisposition::Refused(ReviewRefusal::NoEligibleReviewer),
            dispatches: 0,
            cleanup: None,
            persisted: Ok(()),
            proposal: None,
        };
        let capsule = match build_review_capsule(ReviewCapsuleBuildInput {
            request: attempt.request,
            repository_root: attempt.repository_root,
            worker_checks: attempt.worker_checks,
        }) {
            Ok(c) => c,
            Err(e) => {
                ticket.disposition = ReviewDisposition::Refused(ReviewRefusal::Capsule(e));
                return self.finish(ticket, &attempt, key, sink);
            }
        };
        ticket.capsule = Some(capsule.clone());
        let req = requirements(&attempt);
        let candidate = match self.routing.select_reviewer(&capsule, &req) {
            Ok(c) => c,
            Err(ReviewerRoutingRefusal::PolicyBlocked) => {
                // No provider was selected; the refusal is routing's.
                ticket.disposition = self.interruption(
                    &capsule,
                    "routing:no-provider-selected",
                    None,
                    SafetyInterruptionDetectionConfidence::Unknown,
                );
                return self.finish(ticket, &attempt, key, sink);
            }
            Err(ReviewerRoutingRefusal::NoEligibleReviewer) => {
                return self.finish(ticket, &attempt, key, sink);
            }
        };
        if let Err(refusal) = independence(&candidate, &req, attempt.implementer) {
            ticket.disposition = ReviewDisposition::Refused(refusal);
            return self.finish(ticket, &attempt, key, sink);
        }
        let Some(runtime) = self
            .runtimes
            .iter()
            .find(|r| r.runtime_id() == candidate.runtime_id)
        else {
            ticket.disposition = ReviewDisposition::Refused(ReviewRefusal::UnknownRuntime(
                candidate.runtime_id.clone(),
            ));
            return self.finish(ticket, &attempt, key, sink);
        };
        let workspace = match provision_review_workspace(&ReviewWorkspaceRequest {
            repository_root: attempt.repository_root.to_path_buf(),
            workspace_id: derived_id("review-ws", &[capsule.review_id()]),
            task_id: capsule.task_id().to_owned(),
            reviewed_sha: capsule.implementation_sha().clone(),
            worktree_path: attempt.review_worktree.to_path_buf(),
        }) {
            Ok(w) => w,
            Err(e) => {
                ticket.disposition =
                    ReviewDisposition::Refused(ReviewRefusal::Workspace(e.to_string()));
                return self.finish(ticket, &attempt, key, sink);
            }
        };
        ticket.dispatches = 1;
        ticket.disposition =
            self.review_in(&attempt, &capsule, &candidate, runtime, &req, &workspace);
        ticket.cleanup = Some(
            remove_review_workspace(attempt.repository_root, &workspace).map_err(|e| e.to_string()),
        );
        self.finish(ticket, &attempt, key, sink)
    }

    fn review_in(
        &self,
        attempt: &AttemptFinished<'_>,
        capsule: &ReviewCapsuleNonTemporalCore,
        candidate: &ReviewerCandidate,
        runtime: &&dyn ReviewerRuntime,
        req: &ReviewerRequirements,
        workspace: &ReviewWorkspace,
    ) -> ReviewDisposition {
        let output = match runtime.run_review(capsule, workspace.handle()) {
            Ok(o) => o,
            // PolicyBlocked may be Receipts' own refusal (e.g. no protected
            // launch), not a provider signal: confidence stays UNKNOWN.
            Err(FailureClass::PolicyBlocked) => {
                return self.interruption(
                    capsule,
                    &candidate.provider_id,
                    candidate.model_id.clone(),
                    SafetyInterruptionDetectionConfidence::Unknown,
                );
            }
            Err(other) => {
                return ReviewDisposition::InvalidOutput(format!(
                    "reviewer runtime failed: {other:?}"
                ));
            }
        };
        if !output.protected {
            return ReviewDisposition::InvalidOutput(
                "reviewer run was not a protected launch".into(),
            );
        }
        if let Some(session) = &output.session_ref
            && req.excluded_session_refs.contains(session)
        {
            return ReviewDisposition::InvalidOutput("reviewer ran in an excluded session".into());
        }
        if let Err(e) = verify_review_workspace(workspace) {
            return ReviewDisposition::InvalidOutput(format!("review worktree changed: {e}"));
        }
        let reported = match parse_reviewer_output(&output, capsule) {
            Ok(r) => r,
            Err(e) => return ReviewDisposition::InvalidOutput(format!("{e:?}")),
        };
        let (checks, mismatches) = self.reproduce(capsule, workspace);
        if let Err(e) = verify_review_workspace(workspace) {
            return ReviewDisposition::InvalidOutput(format!(
                "reproduction changed the review worktree: {e}"
            ));
        }
        let unauthorized = match verify_write_scope(
            attempt.repository_root,
            capsule.baseline_sha().as_str(),
            capsule.implementation_sha().as_str(),
            capsule.allowed_write_paths(),
            &[],
        ) {
            Ok(v) => v.unauthorized_paths().to_vec(),
            Err(e) => {
                return ReviewDisposition::InvalidOutput(format!(
                    "write-scope evidence unavailable: {e:?}"
                ));
            }
        };
        match self.attest(
            capsule,
            candidate,
            &output.session_ref,
            reported,
            checks,
            mismatches,
            unauthorized,
        ) {
            Ok(r) => ReviewDisposition::Reviewed(r),
            Err(e) => ReviewDisposition::InvalidOutput(e),
        }
    }

    /// Runs every deterministic criterion command and every worker check
    /// command once in the review worktree. Returns REVIEW_EXECUTION checks
    /// and the commands whose worker claim did not reproduce.
    fn reproduce(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        workspace: &ReviewWorkspace,
    ) -> (Vec<A4ReviewReproductionCheck>, Vec<String>) {
        let mut commands: Vec<Vec<String>> = Vec::new();
        for c in capsule.acceptance_criteria() {
            if c.kind() == ReviewCapsuleCriterionKind::Deterministic
                && let Some(cmd) = c.check_command()
                && !commands.iter().any(|x| x == cmd)
            {
                commands.push(cmd.to_vec());
            }
        }
        for check in capsule.checks().unwrap_or_default() {
            if !commands.iter().any(|x| x == check.command()) {
                commands.push(check.command().to_vec());
            }
        }
        let mut out = Vec::new();
        let mut mismatches = Vec::new();
        for cmd in commands {
            let run = run_check(self.checks, workspace.handle().worktree_path(), &cmd);
            let result = match run.status {
                CheckRunStatus::Passed => A4ReviewReproductionCheckResult::Pass,
                CheckRunStatus::Failed => A4ReviewReproductionCheckResult::Fail,
                CheckRunStatus::TimedOut | CheckRunStatus::NotRunnable => {
                    A4ReviewReproductionCheckResult::Error
                }
            };
            let claimed_pass = capsule.checks().unwrap_or_default().iter().any(|c| {
                c.command() == cmd.as_slice() && c.result() == Some(ReviewCapsuleCheckResult::Pass)
            });
            let criterion = capsule.acceptance_criteria().iter().any(|c| {
                c.kind() == ReviewCapsuleCriterionKind::Deterministic
                    && c.check_command() == Some(cmd.as_slice())
            });
            if result != A4ReviewReproductionCheckResult::Pass && (claimed_pass || criterion) {
                mismatches.push(cmd.join(" "));
            }
            if let Ok(check) = A4ReviewReproductionCheck::new(
                WorkspaceCheckpointCheckSource::ReviewExecution,
                cmd.clone(),
                run.exit_code.map_or(-1, i64::from),
                capsule.implementation_sha().as_str().to_owned(),
                Some(run.status == CheckRunStatus::TimedOut),
                None,
                Some(result),
            ) {
                out.push(check);
            } else {
                mismatches.push(cmd.join(" "));
            }
        }
        (out, mismatches)
    }

    #[allow(clippy::too_many_arguments)]
    fn attest(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        candidate: &ReviewerCandidate,
        session_ref: &Option<String>,
        reported: A4Review,
        checks: Vec<A4ReviewReproductionCheck>,
        mismatches: Vec<String>,
        unauthorized: Vec<String>,
    ) -> Result<A4Review, String> {
        let r = reported.core();
        let mut blocking = r.blocking_findings().to_vec();
        let mut verdict = r.verdict();
        // Mandatory-category floor: the capsule's blocking categories, the
        // eight-category assurance floor and a missing required negative
        // test are blocking whatever the reviewer flagged. Such a finding is
        // moved to the blocking array (flag set) and the verdict is REJECT.
        let mandatory = |category: A4ReviewFindingCategory| {
            is_assurance_profile_blocking_floor(category)
                || category == A4ReviewFindingCategory::MissingRequiredNegativeTest
                || capsule
                    .severity_policy()
                    .blocking_categories()
                    .iter()
                    .any(|c| c == category.as_str())
        };
        let mut nonblocking = Vec::new();
        let mut promoted = false;
        for f in r.nonblocking_findings() {
            if mandatory(f.category()) {
                blocking.push(
                    A4ReviewFinding::new(
                        f.finding_id().to_owned(),
                        f.severity(),
                        f.category(),
                        f.description().to_owned(),
                        true,
                        f.path().map(str::to_owned),
                        f.line(),
                        f.evidence_ref().cloned(),
                        f.confidence(),
                        f.source(),
                    )
                    .map_err(|e| format!("{e:?}"))?,
                );
                promoted = true;
            } else {
                nonblocking.push(f.clone());
            }
        }
        // A blocking-array entry in a mandatory category is already blocking.
        if promoted {
            verdict = A4ReviewVerdict::Reject;
        }
        for (n, command) in mismatches.iter().enumerate() {
            blocking.push(
                A4ReviewFinding::new(
                    format!("reproduction-{n}"),
                    A4ReviewFindingSeverity::High,
                    A4ReviewFindingCategory::UnreproducibleEvidence,
                    format!("controller reproduction did not pass: {command}"),
                    true,
                    None,
                    None,
                    None,
                    Some(A4ReviewFindingConfidence::High),
                    Some(A4ReviewFindingSource::Test),
                )
                .map_err(|e| format!("{e:?}"))?,
            );
        }
        if !unauthorized.is_empty() {
            blocking.push(
                A4ReviewFinding::new(
                    "write-scope".into(),
                    A4ReviewFindingSeverity::High,
                    A4ReviewFindingCategory::WriteScopeViolation,
                    format!(
                        "changes outside allowed_write_paths: {}",
                        unauthorized.join(", ")
                    ),
                    true,
                    None,
                    None,
                    None,
                    Some(A4ReviewFindingConfidence::High),
                    Some(A4ReviewFindingSource::ConfigCheck),
                )
                .map_err(|e| format!("{e:?}"))?,
            );
        }
        if blocking.len() > r.blocking_findings().len() {
            verdict = A4ReviewVerdict::Reject;
        }
        let reviewer = A4ReviewReviewer::new(
            Some(candidate.provider_id.clone()),
            candidate.model_id.clone(),
            Some(candidate.runtime_id.clone()),
            session_ref.clone(),
        )
        .map_err(|e| format!("{e:?}"))?;
        let performed = !checks.is_empty();
        let limitation = if performed {
            A4ReviewReproductionLimitation::Null
        } else {
            A4ReviewReproductionLimitation::Text("no deterministic checks to reproduce".into())
        };
        let core = A4ReviewNonTemporalCore::new(
            capsule.review_id().to_owned(),
            capsule.task_id().to_owned(),
            capsule.implementation_sha().as_str().to_owned(),
            verdict,
            true,
            blocking,
            nonblocking,
            r.dimensions().to_vec(),
            Some(reviewer),
            Some(A4ReviewReproduction::new(
                Some(performed),
                Some(checks),
                limitation,
            )),
            Some(unauthorized),
            r.recommended_action(),
            r.rationale().map(str::to_owned),
        )
        .map_err(|e| format!("{e:?}"))?;
        Ok(A4Review::new(core, Some((self.clock)())))
    }

    fn interruption(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        provider_id: &str,
        model_id: Option<String>,
        confidence: SafetyInterruptionDetectionConfidence,
    ) -> ReviewDisposition {
        match SafetyInterruptionNonTemporalCore::new(
            derived_id("interruption", &[capsule.review_id(), provider_id]),
            capsule.task_id().to_owned(),
            capsule.attempt_id().to_owned(),
            provider_id.to_owned(),
            model_id,
            SafetyInterruptionState::PolicyBlocked,
            Some(confidence),
            None,
            Some(false),
            Some(false),
            None,
            Some(false),
            Some(SafetyInterruptionTerminalOutcome::HumanRequired),
            None,
        ) {
            Ok(core) => {
                ReviewDisposition::PolicyBlocked(SafetyInterruption::new(core, (self.clock)()))
            }
            // An unrepresentable provider identity is still a refusal, never PASS.
            Err(e) => {
                ReviewDisposition::InvalidOutput(format!("policy refusal unrecordable: {e:?}"))
            }
        }
    }

    /// Seals the exact package durably, then publishes it. A failure at
    /// either step is reported and leaves the claim in place, so a retry
    /// finishes publication from the sealed package (or refuses) and never
    /// executes the review again.
    fn finish(
        &self,
        mut ticket: ReviewTicket,
        attempt: &AttemptFinished<'_>,
        key: &str,
        sink: &mut dyn ReviewRecordSink,
    ) -> ReviewTicket {
        match records(&ticket, attempt, key) {
            Ok(proposal) => {
                let cleanup_ok = matches!(ticket.cleanup, Some(Ok(())));
                let sealed = SealedReview::new(
                    &proposal,
                    outcome_label(&ticket.disposition),
                    cleanup_ok,
                    ticket.dispatches,
                );
                ticket.persisted = sink
                    .seal(key, &sealed)
                    .and_then(|()| sink.publish(key, &proposal));
                ticket.proposal = Some(proposal);
            }
            Err(e) => ticket.persisted = Err(e),
        }
        ticket
    }
}

fn outcome_label(d: &ReviewDisposition) -> &'static str {
    match d {
        ReviewDisposition::Reviewed(_) => "REVIEWED",
        ReviewDisposition::InvalidOutput(_) => "INVALID_OUTPUT",
        ReviewDisposition::PolicyBlocked(_) => "POLICY_BLOCKED",
        ReviewDisposition::Refused(_) => "REFUSED",
        ReviewDisposition::Replayed(_) => "REPLAYED",
    }
}

fn records(
    ticket: &ReviewTicket,
    attempt: &AttemptFinished<'_>,
    claim_key: &str,
) -> Result<ReviewRecordProposal, String> {
    let core = attempt.request.core();
    let mut b = ProposalBuilder::new(
        attempt.project_id,
        core.task_id(),
        core.context_epoch().decimal_digits(),
        claim_key,
    )?;
    let task = (SubjectKind::Task, core.task_id());
    if let Some(capsule) = &ticket.capsule
        && ticket.dispatches > 0
    {
        let json = encode_review_capsule(capsule);
        let payload = b.content(
            &format!("review:capsule:{}", capsule.review_id()),
            json.as_bytes(),
        );
        b.event(EventType::ReviewDispatched, task, payload)?;
    }
    match &ticket.disposition {
        ReviewDisposition::Reviewed(review) => {
            let json = encode_a4_review(review);
            let id = review.core().review_id();
            b.decision(DecisionRecordKind::A4Review, id, &json)?;
            let payload = b.content(&format!("review:a4:{id}"), json.as_bytes());
            // Explicit match: a verdict is never "passing" by exclusion.
            let passed = matches!(
                review.core().verdict(),
                A4ReviewVerdict::Pass | A4ReviewVerdict::PassWithNonblockingFindings
            );
            b.event(
                if passed {
                    EventType::ReviewPassed
                } else {
                    EventType::ReviewRejected
                },
                task,
                payload.clone(),
            )?;
            // One event per finding, each carrying that finding (not the
            // whole review), blocking findings first.
            for f in review
                .core()
                .blocking_findings()
                .iter()
                .chain(review.core().nonblocking_findings())
            {
                let body = serde_json::json!({
                    "review_id": id,
                    "task_id": review.core().task_id(),
                    "reviewed_sha": review.core().reviewed_sha(),
                    "finding_id": f.finding_id(),
                    "severity": f.severity().as_str(),
                    "category": f.category().as_str(),
                    "description": f.description(),
                    "blocking": f.blocking(),
                })
                .to_string();
                let payload = b.content(
                    &format!("review:finding:{id}:{}", f.finding_id()),
                    body.as_bytes(),
                );
                b.event(EventType::FindingRaised, (SubjectKind::Review, id), payload)?;
            }
        }
        ReviewDisposition::PolicyBlocked(interruption) => {
            let json = encode_safety_interruption(interruption);
            let payload = b.content(
                &format!(
                    "review:interruption:{}",
                    interruption.core().interruption_id()
                ),
                json.as_bytes(),
            );
            b.event(EventType::PolicyBlocked, task, payload.clone())?;
            b.event(EventType::HumanRequired, task, payload)?;
        }
        ReviewDisposition::InvalidOutput(reason) => {
            let body = format!(
                "{{\"task_id\":{},\"invalid_review_output\":{}}}",
                serde_json::Value::from(core.task_id()),
                serde_json::Value::from(reason.as_str())
            );
            let payload = b.content(
                &derived_id("review:invalid", &[core.request_id(), reason]),
                body.as_bytes(),
            );
            b.event(EventType::HumanRequired, task, payload)?;
        }
        // A replayed ticket is never re-recorded; its package already exists.
        ReviewDisposition::Replayed(_) => {}
        ReviewDisposition::Refused(refusal) => {
            let reason = format!("{refusal:?}");
            let body = format!(
                "{{\"task_id\":{},\"review_refused\":{}}}",
                serde_json::Value::from(core.task_id()),
                serde_json::Value::from(reason.as_str())
            );
            let payload = b.content(
                &derived_id("review:refused", &[core.request_id(), &reason]),
                body.as_bytes(),
            );
            b.event(EventType::HumanRequired, task, payload)?;
        }
    }
    Ok(b.finish())
}
