//! RI-W2-3: the executable nine-check A2 acceptance gate (H1, L4b).
//!
//! Git facts (checks 1, 2, 6, 7, 8) and executed facts (checks 3 and 9) are
//! observed or re-executed through Workspace APIs at evaluation time. The
//! recorded evidence of checks 4 and 5 is NOT re-executed: the A4Review
//! record and the review capsule (which carries the worker checks) are bound
//! to State (the persisted A4Review and the REVIEW_DISPATCHED payload
//! digest), the handoff's checks must equal the capsule's, and a record that
//! contradicts itself (PASS with a nonzero exit) is refused. A3Handoff is
//! not a State record kind, so worker-check origin is the capsule the
//! controller dispatched, not an independent execution. Caller attestations
//! (IntegrationRequest `attestations`) are never read. All nine checks always
//! run and are all reported. The gate takes only shared references and a read-only
//! [`StateReader`]: it holds no State writer, edits no source, moves no
//! branch and merges nothing. Temporary review/merge worktrees it creates
//! are owned and removed; a removal failure is a failed check.
//!
//! Outcome precedence: HUMAN_REQUIRED > BLOCKED > REPAIR > ACCEPT.

use crate::capsule_builder::review_id;
use crate::dispositions::{FindingDisposition, validate_dispositions};
use crate::records::{ProposalBuilder, derived_id};
use crate::reproduction::{CheckRunStatus, CheckRunnerConfig, run_check};
use crate::wire::{decode_a4_review, encode_integration_decision, encode_review_capsule};
use crate::{
    A3HandoffCheckResult, A3HandoffNonTemporalCore, A4ReviewFindingCategory,
    A4ReviewNonTemporalCore, A4ReviewReproductionCheckResult, AssuranceProfile,
    ExactShaAcceptanceGate, ExactShaAcceptanceGateError, ExactShaAcceptanceGateInput,
    IntegrationDecision, IntegrationDecisionCheck, IntegrationDecisionCheckResult,
    IntegrationDecisionNonTemporalCore, IntegrationDecisionNullableString,
    IntegrationDecisionOutcome, IntegrationDecisionProvenance, IntegrationRequestGateLevel,
    IntegrationRequestNonTemporalCore, RepairCycleDisposition, ReviewCapsuleCriterionKind,
    ReviewCapsuleNonTemporalCore, ReviewCapsuleReviewScope, ReviewDateTimeV1, ReviewRequest,
    SecurityPipelinePolicy, ports::ReviewRecordProposal,
};
use receipts_state::{
    DecisionRecordKind, EventQuery, EventSubject, EventType, ProvenanceLinkKind, StateReader,
    SubjectKind,
};
use receipts_workspace_execution::{
    CommitSha, ReviewWorkspace, ReviewWorkspaceRequest, WorkspaceCheckpointCheckSource,
    WorkspaceCheckpointEvidenceCaptureRequest, WorkspaceCheckpointKind,
    WriteScopeVerificationStatus, capture_workspace_checkpoint_evidence,
    provision_review_workspace, provision_temporary_merge, remove_review_workspace,
    review_commit_range, review_diff, verify_review_workspace, verify_write_scope,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The nine A2 acceptance checks, in contract order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceCheckId {
    ExactReviewedHead,
    NoCommitAfterReview,
    CriteriaReproduced,
    VerdictAndFindings,
    RequiredChecks,
    WriteScope,
    Dependencies,
    Freshness,
    PostMergeTests,
}

impl AcceptanceCheckId {
    pub const ALL: [Self; 9] = [
        Self::ExactReviewedHead,
        Self::NoCommitAfterReview,
        Self::CriteriaReproduced,
        Self::VerdictAndFindings,
        Self::RequiredChecks,
        Self::WriteScope,
        Self::Dependencies,
        Self::Freshness,
        Self::PostMergeTests,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactReviewedHead => "A2_CHECK_1_EXACT_REVIEWED_HEAD",
            Self::NoCommitAfterReview => "A2_CHECK_2_NO_COMMIT_AFTER_REVIEW",
            Self::CriteriaReproduced => "A2_CHECK_3_CRITERIA_REPRODUCED",
            Self::VerdictAndFindings => "A2_CHECK_4_VERDICT_AND_FINDINGS",
            Self::RequiredChecks => "A2_CHECK_5_REQUIRED_CHECKS",
            Self::WriteScope => "A2_CHECK_6_WRITE_SCOPE",
            Self::Dependencies => "A2_CHECK_7_DEPENDENCIES",
            Self::Freshness => "A2_CHECK_8_FRESHNESS",
            Self::PostMergeTests => "A2_CHECK_9_POST_MERGE_TESTS",
        }
    }
}

/// What a failed check requires next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureSeverity {
    /// The implementation can be repaired.
    Repair,
    /// Evidence integrity or a precondition is broken; no repair helps yet.
    Blocked,
    /// Only a human can resolve it.
    HumanRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceCheckResult {
    pub id: AcceptanceCheckId,
    /// `None` when the check passed.
    pub failure: Option<FailureSeverity>,
    pub detail: String,
}

/// Adopted supersession evidence for one dependency path: another accepted
/// task replaced that path's content after the dependency was accepted. The
/// gate verifies the claim against State acceptance and Git; a claim it
/// cannot verify is uncertainty, which cannot ACCEPT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySupersession {
    pub dependency_task_id: String,
    pub path: String,
    pub superseding_task_id: String,
}

/// Inputs. Paths are observed live; records are immutable evidence.
pub struct AcceptanceInput<'a> {
    pub project_id: &'a str,
    /// The A2_ACCEPTANCE request naming this one task.
    pub integration_request: &'a IntegrationRequestNonTemporalCore,
    pub review_request: &'a ReviewRequest,
    /// Task-category floor; the effective profile is only ever raised.
    pub profile_floor: AssuranceProfile,
    pub capsule: &'a ReviewCapsuleNonTemporalCore,
    /// Initial..current, as retained; the last pair is the current attempt.
    pub handoffs: &'a [A3HandoffNonTemporalCore],
    pub reviews: &'a [A4ReviewNonTemporalCore],
    pub completed_repair_attempts: usize,
    pub repository_root: &'a Path,
    /// The task's checkout, observed for HEAD and cleanliness.
    pub task_worktree: &'a Path,
    /// A checkout of the merge target, observed for its HEAD.
    pub merge_target_worktree: &'a Path,
    pub forbidden_write_paths: &'a [String],
    /// Tasks this task depends on (same project).
    pub dependencies: &'a [String],
    /// Required: the review record and capsule are bound to State, and
    /// dependency acceptance is read from it.
    pub state: Option<&'a StateReader>,
    /// Supersession evidence for dependency paths that changed after the
    /// dependency was accepted (see [`DependencySupersession`]).
    pub supersessions: &'a [DependencySupersession],
    /// Explicit dispositions for a PASS_WITH_NONBLOCKING_FINDINGS review.
    pub finding_dispositions: &'a [FindingDisposition],
    pub checks: &'a CheckRunnerConfig,
    /// Argv of the component suite run on the post-merge tree.
    pub component_suite: &'a [String],
    /// Empty directory for owned temporary worktrees.
    pub scratch: &'a Path,
    pub clock: &'a dyn Fn() -> ReviewDateTimeV1,
}

#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub outcome: IntegrationDecisionOutcome,
    pub checks: Vec<AcceptanceCheckResult>,
    pub decision: IntegrationDecision,
    /// Records for the trusted core; the gate persists nothing itself.
    pub proposal: Result<ReviewRecordProposal, String>,
}

impl GateOutcome {
    pub fn failed(&self) -> Vec<AcceptanceCheckId> {
        self.checks
            .iter()
            .filter(|c| c.failure.is_some())
            .map(|c| c.id)
            .collect()
    }
}

pub struct AcceptanceGate;

type Verdict = Result<String, (FailureSeverity, String)>;

fn fail<T>(s: FailureSeverity, d: impl Into<String>) -> Result<T, (FailureSeverity, String)> {
    Err((s, d.into()))
}

use FailureSeverity::{Blocked, HumanRequired, Repair};

/// Live HEAD and cleanliness of a checkout, via Workspace evidence capture.
fn observe(dir: &Path, what: &str) -> Result<(CommitSha, bool), (FailureSeverity, String)> {
    let core = capture_workspace_checkpoint_evidence(WorkspaceCheckpointEvidenceCaptureRequest {
        directory: dir,
        checkpoint_id: format!("acceptance-observation-{what}"),
        workspace_id: format!("acceptance-{what}"),
        task_id: None,
        attempt_id: None,
        kind: WorkspaceCheckpointKind::Progress,
        base_sha: None,
        dirty_diff_ref: None,
        executed_checks: Vec::new(),
    })
    .map_err(|e| (Blocked, format!("{what} checkout unobservable: {e:?}")))?;
    let clean = core.modified_files().is_empty() && core.untracked_files().is_empty();
    Ok((core.head_sha().clone(), clean))
}

struct Observed {
    task_head: Result<(CommitSha, bool), (FailureSeverity, String)>,
    target_head: Result<(CommitSha, bool), (FailureSeverity, String)>,
}

impl AcceptanceGate {
    pub fn evaluate(input: &AcceptanceInput<'_>) -> GateOutcome {
        let observed = Observed {
            task_head: observe(input.task_worktree, "task"),
            target_head: observe(input.merge_target_worktree, "target"),
        };
        let profile = input
            .review_request
            .core()
            .assurance_profile()
            .raise_to_at_least(input.profile_floor);
        let results: Vec<(AcceptanceCheckId, Verdict)> = vec![
            (
                AcceptanceCheckId::ExactReviewedHead,
                check1(input, &observed),
            ),
            (
                AcceptanceCheckId::NoCommitAfterReview,
                check2(input, &observed),
            ),
            (AcceptanceCheckId::CriteriaReproduced, check3(input)),
            (AcceptanceCheckId::VerdictAndFindings, check4(input)),
            (AcceptanceCheckId::RequiredChecks, check5(input, profile)),
            (AcceptanceCheckId::WriteScope, check6(input)),
            (AcceptanceCheckId::Dependencies, check7(input)),
            (AcceptanceCheckId::Freshness, check8(input, &observed)),
            (AcceptanceCheckId::PostMergeTests, check9(input, &observed)),
        ];
        let checks: Vec<AcceptanceCheckResult> = results
            .into_iter()
            .map(|(id, v)| match v {
                Ok(detail) => AcceptanceCheckResult {
                    id,
                    failure: None,
                    detail,
                },
                Err((s, detail)) => AcceptanceCheckResult {
                    id,
                    failure: Some(s),
                    detail,
                },
            })
            .collect();
        let outcome = match checks.iter().filter_map(|c| c.failure).max() {
            None => IntegrationDecisionOutcome::Accept,
            Some(Repair) => IntegrationDecisionOutcome::Repair,
            Some(Blocked) => IntegrationDecisionOutcome::Blocked,
            Some(HumanRequired) => IntegrationDecisionOutcome::HumanRequired,
        };
        let decision = decision(input, outcome, &checks);
        let proposal = proposal(input, &decision);
        GateOutcome {
            outcome,
            checks,
            decision,
            proposal,
        }
    }
}

fn current<'a, T>(items: &'a [T], what: &str) -> Result<&'a T, (FailureSeverity, String)> {
    items.last().ok_or((Blocked, format!("no {what}")))
}

/// Check 1: the reviewed SHA is the implementation SHA, the commit exists
/// and is on the observed task branch, and the request names exactly it.
fn check1(i: &AcceptanceInput<'_>, o: &Observed) -> Verdict {
    let implementation = i.capsule.implementation_sha();
    let review = current(i.reviews, "A4 review")?;
    let handoff = current(i.handoffs, "A3 handoff")?;
    if review.reviewed_sha() != implementation.as_str() {
        return fail(Blocked, "review_sha differs from implementation_sha");
    }
    if handoff.final_sha() != implementation {
        return fail(Blocked, "handoff final_sha differs from implementation_sha");
    }
    if review.review_id() != i.capsule.review_id()
        || i.capsule.review_id() != review_id(i.review_request)
    {
        return fail(Blocked, "review is not bound to this request's capsule");
    }
    if i.review_request.core().implementation_sha() != implementation {
        return fail(Blocked, "review request names another implementation");
    }
    let r = i.integration_request;
    let tasks = r.tasks_included();
    if r.gate_level() != IntegrationRequestGateLevel::A2Acceptance
        || tasks.len() != 1
        || tasks[0].task_id() != i.capsule.task_id()
        || tasks[0].implementation_sha() != implementation
        || tasks[0].review_sha() != implementation
        || r.source_sha() != implementation
    {
        return fail(
            Blocked,
            "integration request does not name exactly this task at this SHA",
        );
    }
    review_commit_range(i.repository_root, i.capsule.baseline_sha(), implementation)
        .map_err(|e| (Blocked, format!("implementation commit unavailable: {e}")))?;
    let (head, _) = o.task_head.clone()?;
    review_commit_range(i.repository_root, implementation, &head).map_err(|e| {
        (
            Blocked,
            format!("implementation is not on the task branch: {e}"),
        )
    })?;
    Ok(format!(
        "reviewed {} on the task branch",
        implementation.as_str()
    ))
}

/// Check 2: no commit after review; observed HEAD equals the reviewed SHA
/// with nothing uncommitted.
fn check2(i: &AcceptanceInput<'_>, o: &Observed) -> Verdict {
    let review = current(i.reviews, "A4 review")?;
    let reviewed = CommitSha::parse(review.reviewed_sha())
        .map_err(|_| (Blocked, "malformed reviewed SHA".to_string()))?;
    let (head, clean) = o.task_head.clone()?;
    if head != reviewed {
        let after = review_commit_range(i.repository_root, &reviewed, &head)
            .map(|r| format!("{} commit(s) after review", r.commits.len()))
            .unwrap_or_else(|_| "branch moved off the reviewed commit".into());
        return fail(Blocked, after);
    }
    if !clean {
        return fail(Blocked, "uncommitted changes after review");
    }
    Ok("task HEAD is the reviewed commit, clean".into())
}

struct Owned<'a> {
    root: &'a Path,
    workspace: Option<ReviewWorkspace>,
}

impl Owned<'_> {
    /// Removes the owned worktree; failure is reported, never hidden.
    fn release(mut self) -> Result<(), (FailureSeverity, String)> {
        match self.workspace.take() {
            Some(w) => remove_review_workspace(self.root, &w)
                .map_err(|e| (HumanRequired, format!("owned worktree cleanup failed: {e}"))),
            None => Ok(()),
        }
    }
}

fn scratch_path(i: &AcceptanceInput<'_>, name: &str) -> PathBuf {
    i.scratch.join(derived_id(name, &[i.capsule.review_id()]))
}

/// Check 3: every deterministic criterion re-runs and passes in a fresh
/// review worktree at the exact SHA; no semantic criterion is reported unmet.
fn check3(i: &AcceptanceInput<'_>) -> Verdict {
    let review = current(i.reviews, "A4 review")?;
    if review
        .blocking_findings()
        .iter()
        .chain(review.nonblocking_findings())
        .any(|f| f.category() == A4ReviewFindingCategory::AcceptanceCriterionUnmet)
    {
        return fail(Repair, "the review reports an unmet acceptance criterion");
    }
    let workspace = provision_review_workspace(&ReviewWorkspaceRequest {
        repository_root: i.repository_root.to_path_buf(),
        workspace_id: derived_id("acceptance-ws", &[i.capsule.review_id()]),
        task_id: i.capsule.task_id().to_owned(),
        reviewed_sha: i.capsule.implementation_sha().clone(),
        worktree_path: scratch_path(i, "criteria"),
    })
    .map_err(|e| {
        (
            HumanRequired,
            format!("cannot provision reproduction worktree: {e}"),
        )
    })?;
    let owned = Owned {
        root: i.repository_root,
        workspace: Some(workspace),
    };
    let wt = owned
        .workspace
        .as_ref()
        .map(|w| w.handle().worktree_path().to_path_buf());
    let mut failures = Vec::new();
    let mut ran = 0;
    for c in i.capsule.acceptance_criteria() {
        if c.kind() != ReviewCapsuleCriterionKind::Deterministic {
            continue;
        }
        let Some(cmd) = c.check_command().filter(|c| !c.is_empty()) else {
            failures.push(format!("{}: no argv", c.id()));
            continue;
        };
        ran += 1;
        let run = run_check(i.checks, wt.as_deref().unwrap_or(i.scratch), cmd);
        if run.status != CheckRunStatus::Passed {
            failures.push(format!("{}: {:?}", c.id(), run.status));
        }
    }
    let integrity = owned
        .workspace
        .as_ref()
        .map(|w| verify_review_workspace(w).map(|_| ()));
    owned.release()?;
    if let Some(Err(e)) = integrity {
        return fail(Blocked, format!("criteria run changed the tree: {e}"));
    }
    if !failures.is_empty() {
        return fail(Repair, format!("criteria not met: {}", failures.join("; ")));
    }
    Ok(format!("{ran} deterministic criteria re-run and passed"))
}

/// The hex SHA-256 State records as a payload digest of `bytes`.
fn payload_digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// Binds the supplied records to the admitted origin: the current A4Review
/// must equal the A4Review record the trusted core persisted for its id, and
/// the capsule must be exactly the one named by a persisted REVIEW_DISPATCHED
/// payload digest for this task. Caller-built records cannot pass.
fn bind_origin(i: &AcceptanceInput<'_>) -> Result<(), (FailureSeverity, String)> {
    let state = i.state.ok_or((
        Blocked,
        "evidence origin cannot be verified without a State reader".to_string(),
    ))?;
    let review = current(i.reviews, "A4 review")?;
    let stored = state
        .find_decision_record(
            i.project_id,
            DecisionRecordKind::A4Review,
            review.review_id(),
        )
        .map_err(|e| (Blocked, format!("A4Review record unreadable: {e}")))?
        .ok_or((
            Blocked,
            "the A4Review is not a persisted record (unadmitted origin)".to_string(),
        ))?;
    let persisted = decode_a4_review(stored.record_json())
        .map_err(|e| (Blocked, format!("persisted A4Review undecodable: {e}")))?;
    if persisted.core() != review {
        return fail(
            Blocked,
            "the supplied A4Review differs from the persisted record",
        );
    }
    let want = payload_digest(encode_review_capsule(i.capsule).as_bytes());
    let dispatched = state
        .list_events(&EventQuery {
            project_id: i.project_id.to_owned(),
            subject: Some(EventSubject {
                kind: SubjectKind::Task,
                id: i.capsule.task_id().to_owned(),
            }),
            event_types: Some(vec![EventType::ReviewDispatched]),
            limit: 1000,
        })
        .map_err(|e| (Blocked, format!("dispatch events unreadable: {e}")))?;
    if !dispatched.iter().any(|e| e.payload.digest == want) {
        return fail(
            Blocked,
            "the review capsule is not the one a persisted REVIEW_DISPATCHED names",
        );
    }
    Ok(())
}

/// Check 4: PASS verdict with blocking findings closed, via the existing
/// exact-SHA foundation (repair history, independence, finding floors),
/// bound to the admitted origin, and explicit dispositions for nonblocking
/// findings.
fn check4(i: &AcceptanceInput<'_>) -> Verdict {
    use ExactShaAcceptanceGateError as E;
    bind_origin(i)?;
    let implementation = i.capsule.implementation_sha().as_str();
    let baseline = i.capsule.baseline_sha().as_str();
    match ExactShaAcceptanceGate::evaluate(ExactShaAcceptanceGateInput {
        handoffs: i.handoffs,
        reviews: i.reviews,
        completed_repair_attempts: i.completed_repair_attempts,
        capsule: Some(i.capsule),
        accepted_candidate_sha: Some(implementation),
        current_baseline_sha: Some(baseline),
        dependency_freshness: &[],
    }) {
        Ok(_) => {
            let review = current(i.reviews, "A4 review")?;
            if review.verdict() == crate::A4ReviewVerdict::PassWithNonblockingFindings {
                validate_dispositions(review, i.finding_dispositions).map_err(|e| {
                    (
                        Blocked,
                        format!("nonblocking findings are not dispositioned: {e:?}"),
                    )
                })?;
            }
            Ok("verdict passes, origin bound, no blocking finding is open".into())
        }
        Err(E::RepairCycleNotClear(RepairCycleDisposition::RepairBoundExhausted)) => fail(
            HumanRequired,
            "repair bound exhausted with a rejecting review",
        ),
        Err(
            e @ (E::RepairCycleNotClear(_)
            | E::BlockingFindingsPresent
            | E::VerdictFindingsMismatch
            | E::UnauthorizedFileChangesPresent),
        ) => fail(Repair, format!("{e:?}")),
        Err(e @ E::RepairHistory(_)) => fail(Blocked, format!("{e:?}")),
        Err(e) => fail(Blocked, format!("{e:?}")),
    }
}

/// Check 5: the checks the effective (raised-only) profile requires are
/// present at the exact SHA, from the right channel, passing, not timed out.
fn check5(i: &AcceptanceInput<'_>, profile: AssuranceProfile) -> Verdict {
    let req = profile.requirements();
    let sha = i.capsule.implementation_sha();
    let handoff = current(i.handoffs, "A3 handoff")?;
    let review = current(i.reviews, "A4 review")?;
    if req.worker_checks_required {
        if handoff.checks().is_empty() {
            return fail(Blocked, "worker checks are required but absent");
        }
        for (n, c) in handoff.checks().iter().enumerate() {
            if c.code_sha() != sha
                || c.source() != WorkspaceCheckpointCheckSource::WorkerExecution
                || c.timed_out() == Some(true)
                || c.result() != Some(A3HandoffCheckResult::Pass)
            {
                return fail(
                    Blocked,
                    format!("worker check {n} is not a passing, untimed check at the SHA"),
                );
            }
            if c.exit_code() != 0 {
                return fail(
                    Blocked,
                    format!(
                        "worker check {n} contradicts itself: PASS with exit code {}",
                        c.exit_code()
                    ),
                );
            }
            // The check must be one the dispatched capsule carries.
            let carried = i.capsule.checks().unwrap_or_default().iter().any(|k| {
                k.command() == c.command()
                    && k.exit_code() == c.exit_code()
                    && k.code_sha() == c.code_sha()
                    && k.result() == Some(crate::ReviewCapsuleCheckResult::Pass)
            });
            if !carried {
                return fail(
                    Blocked,
                    format!("worker check {n} is not in the dispatched review capsule"),
                );
            }
        }
    }
    if req.independent_a4_required && !review.independence_attested() {
        return fail(Blocked, "independent review required");
    }
    if req.a4_reproduction_required {
        let checks = review
            .reproduction()
            .filter(|r| r.performed() == Some(true))
            .and_then(|r| r.checks())
            .unwrap_or_default();
        if checks.is_empty() {
            return fail(Blocked, "A4 reproduction required but not performed");
        }
        for (n, c) in checks.iter().enumerate() {
            if c.code_sha() != sha.as_str()
                || c.source() != WorkspaceCheckpointCheckSource::ReviewExecution
                || c.timed_out() == Some(true)
                || c.result() != Some(A4ReviewReproductionCheckResult::Pass)
            {
                return fail(
                    Blocked,
                    format!("reproduction check {n} is not a passing, untimed check at the SHA"),
                );
            }
            if c.exit_code() != 0 {
                return fail(
                    Blocked,
                    format!(
                        "reproduction check {n} contradicts itself: PASS with exit code {}",
                        c.exit_code()
                    ),
                );
            }
        }
    }
    if req.broker_deterministic_rerun_required {
        return fail(
            HumanRequired,
            "HIGH_ASSURANCE broker re-execution (RI-W7-1) is not available",
        );
    }
    let security = match req.security_pipeline {
        SecurityPipelinePolicy::Always => true,
        SecurityPipelinePolicy::OnSecurityTasks => {
            i.capsule.review_scope() == ReviewCapsuleReviewScope::Security
        }
        SecurityPipelinePolicy::NoAutomaticRequirement => false,
    };
    if security {
        return fail(
            HumanRequired,
            "required security pipeline (RI-W5-3) is not available",
        );
    }
    Ok(format!(
        "{} requirements present at the SHA",
        profile.as_str()
    ))
}

/// Check 6: the committed diff is inside `allowed_write_paths`.
fn check6(i: &AcceptanceInput<'_>) -> Verdict {
    let v = verify_write_scope(
        i.repository_root,
        i.capsule.baseline_sha().as_str(),
        i.capsule.implementation_sha().as_str(),
        i.capsule.allowed_write_paths(),
        i.forbidden_write_paths,
    )
    .map_err(|e| (Blocked, format!("write scope unverifiable: {e:?}")))?;
    if v.verification() != WriteScopeVerificationStatus::Pass {
        return fail(
            Repair,
            format!(
                "out-of-scope writes: {:?}; forbidden: {}",
                v.unauthorized_paths(),
                v.forbidden_matches().len()
            ),
        );
    }
    Ok(format!(
        "{} changed paths in scope",
        v.changed_paths().len()
    ))
}

/// What State records about one accepted task: its range and acceptance.
struct AcceptedRange {
    start: CommitSha,
    implementation: CommitSha,
}

fn accepted_range(
    i: &AcceptanceInput<'_>,
    state: &StateReader,
    task: &str,
) -> Result<AcceptedRange, (FailureSeverity, String)> {
    let chain = state
        .provenance_chain(i.project_id, task)
        .map_err(|e| (Blocked, format!("{task}: provenance unreadable: {e}")))?;
    if !chain.contradictions.is_empty() {
        return fail(Blocked, format!("{task}: provenance contradictions"));
    }
    if chain.link(ProvenanceLinkKind::AcceptanceRef).is_none() {
        return fail(Blocked, format!("{task}: not accepted"));
    }
    let sha = |kind, what: &str| {
        let value = chain
            .link(kind)
            .ok_or((Blocked, format!("{task}: no {what} recorded")))?;
        CommitSha::parse(value).map_err(|_| (Blocked, format!("{task}: malformed {what}")))
    };
    Ok(AcceptedRange {
        start: sha(ProvenanceLinkKind::StartSha, "start SHA")?,
        implementation: sha(ProvenanceLinkKind::ImplementationSha, "implementation SHA")?,
    })
}

fn changed_between(
    i: &AcceptanceInput<'_>,
    from: &CommitSha,
    to: &CommitSha,
    what: &str,
) -> Result<Vec<String>, (FailureSeverity, String)> {
    review_diff(i.repository_root, from, to)
        .map(|d| d.changed_paths)
        .map_err(|e| (Blocked, format!("{what}: diff unavailable: {e}")))
}

/// Verifies one supersession claim: the superseding task is accepted, its
/// own range changed the path, it is in the candidate, and the candidate
/// carries that task's content for the path (no later change).
fn verify_supersession(
    i: &AcceptanceInput<'_>,
    state: &StateReader,
    s: &DependencySupersession,
) -> Result<(), (FailureSeverity, String)> {
    let what = format!("{} superseded by {}", s.path, s.superseding_task_id);
    if s.superseding_task_id == s.dependency_task_id || s.superseding_task_id == i.capsule.task_id()
    {
        return fail(
            Blocked,
            format!("{what}: a task cannot supersede itself or be its own evidence"),
        );
    }
    let range = accepted_range(i, state, &s.superseding_task_id)?;
    if !changed_between(i, &range.start, &range.implementation, &what)?.contains(&s.path) {
        return fail(
            Blocked,
            format!("{what}: that task did not change the path"),
        );
    }
    review_commit_range(
        i.repository_root,
        &range.implementation,
        i.capsule.implementation_sha(),
    )
    .map_err(|_| {
        (
            Blocked,
            format!("{what}: superseding task is not in the candidate"),
        )
    })?;
    if changed_between(
        i,
        &range.implementation,
        i.capsule.implementation_sha(),
        &what,
    )?
    .contains(&s.path)
    {
        return fail(
            Blocked,
            format!("{what}: the path changed again after that task"),
        );
    }
    Ok(())
}

/// Check 7: every dependency has a recorded acceptance and no contradiction
/// in State, its implementation is in the candidate, AND its effect is still
/// present. Ancestry alone proves neither (`git revert` keeps ancestry), so
/// each path the dependency changed must be byte-identical in the candidate
/// to the dependency's accepted implementation, or carry verified adopted
/// supersession evidence. Anything else is uncertainty and cannot ACCEPT.
/// Path-level: it does not reason about intent, and no commit message is
/// read.
fn check7(i: &AcceptanceInput<'_>) -> Verdict {
    if i.dependencies.is_empty() {
        return Ok("no dependencies".into());
    }
    let state = i
        .state
        .ok_or((Blocked, "dependencies need a State reader".to_string()))?;
    let candidate = i.capsule.implementation_sha();
    for dep in i.dependencies {
        let range = accepted_range(i, state, dep)?;
        review_commit_range(i.repository_root, &range.implementation, candidate).map_err(|_| {
            (
                Blocked,
                format!(
                    "{dep}: accepted implementation is not in this candidate (reverted or missing)"
                ),
            )
        })?;
        let effect = changed_between(i, &range.start, &range.implementation, dep)?;
        let drifted = changed_between(i, &range.implementation, candidate, dep)?;
        for path in effect.iter().filter(|p| drifted.contains(p)) {
            let claim = i
                .supersessions
                .iter()
                .find(|s| s.dependency_task_id == *dep && s.path == *path);
            match claim {
                Some(s) => verify_supersession(i, state, s)?,
                None => {
                    return fail(
                        Blocked,
                        format!(
                            "{dep}: {path} differs from its accepted implementation and no adopted supersession evidence covers it (reverted, overwritten or unproven)"
                        ),
                    );
                }
            }
        }
    }
    Ok(format!(
        "{} dependencies accepted and their effect is present",
        i.dependencies.len()
    ))
}

/// Check 8: the candidate already contains the current merge target.
fn check8(i: &AcceptanceInput<'_>, o: &Observed) -> Verdict {
    let (target, _) = o.target_head.clone()?;
    review_commit_range(i.repository_root, &target, i.capsule.implementation_sha()).map_err(
        |_| {
            (
                Repair,
                "merge target moved past the candidate's base".to_string(),
            )
        },
    )?;
    Ok(format!("candidate contains target {}", target.as_str()))
}

/// Check 9: the component suite passes on the post-merge tree.
fn check9(i: &AcceptanceInput<'_>, o: &Observed) -> Verdict {
    if i.component_suite.is_empty() {
        return fail(Blocked, "no component suite configured");
    }
    let (target, _) = o.target_head.clone()?;
    let merge = provision_temporary_merge(
        &ReviewWorkspaceRequest {
            repository_root: i.repository_root.to_path_buf(),
            workspace_id: derived_id("merge-ws", &[i.capsule.review_id()]),
            task_id: i.capsule.task_id().to_owned(),
            reviewed_sha: i.capsule.implementation_sha().clone(),
            worktree_path: scratch_path(i, "merge"),
        },
        &target,
    )
    .map_err(|e| (Repair, format!("post-merge tree unavailable: {e}")))?;
    let owned = Owned {
        root: i.repository_root,
        workspace: Some(merge.workspace),
    };
    let wt = owned
        .workspace
        .as_ref()
        .map(|w| w.handle().worktree_path().to_path_buf())
        .unwrap_or_default();
    let run = run_check(i.checks, &wt, i.component_suite);
    let integrity = owned.workspace.as_ref().map(verify_review_workspace);
    owned.release()?;
    if let Some(Err(e)) = integrity {
        return fail(Blocked, format!("suite changed the post-merge tree: {e}"));
    }
    match run.status {
        CheckRunStatus::Passed => Ok(format!(
            "component suite passed on merge {}",
            merge.merge_sha.as_str()
        )),
        CheckRunStatus::Failed => fail(
            Repair,
            format!("component suite failed: exit {:?}", run.exit_code),
        ),
        CheckRunStatus::TimedOut => fail(Repair, "component suite timed out"),
        CheckRunStatus::NotRunnable => fail(
            Blocked,
            format!("component suite not runnable: {:?}", run.detail),
        ),
    }
}

fn decision(
    i: &AcceptanceInput<'_>,
    outcome: IntegrationDecisionOutcome,
    checks: &[AcceptanceCheckResult],
) -> IntegrationDecision {
    let evaluated = checks
        .iter()
        .map(|c| {
            IntegrationDecisionCheck::new(
                c.id.as_str().to_owned(),
                if c.failure.is_none() {
                    IntegrationDecisionCheckResult::Pass
                } else {
                    IntegrationDecisionCheckResult::Fail
                },
                Some(c.detail.clone()),
            )
        })
        .collect();
    let unmet = checks
        .iter()
        .filter(|c| c.failure.is_some())
        .map(|c| c.id.as_str().to_owned())
        .collect();
    let implementation = i.capsule.implementation_sha().as_str().to_owned();
    let review_sha = i
        .reviews
        .last()
        .map(|r| r.reviewed_sha().to_owned())
        .filter(|s| CommitSha::parse(s).is_ok())
        .unwrap_or_else(|| implementation.clone());
    let provenance = IntegrationDecisionProvenance::new(
        i.capsule.task_id().to_owned(),
        i.capsule.baseline_sha().as_str().to_owned(),
        implementation.clone(),
        review_sha,
        if outcome == IntegrationDecisionOutcome::Accept {
            IntegrationDecisionNullableString::String(implementation)
        } else {
            IntegrationDecisionNullableString::Null
        },
        IntegrationDecisionNullableString::Omitted,
    )
    .ok();
    let core = IntegrationDecisionNonTemporalCore::new(
        derived_id(
            "acceptance",
            &[
                i.integration_request.request_id(),
                i.capsule.review_id(),
                outcome.as_str(),
            ],
        ),
        i.integration_request.request_id().to_owned(),
        outcome,
        evaluated,
        provenance.map(|p| vec![p]),
        Some(unmet),
        IntegrationDecisionNullableString::Omitted,
    )
    .expect("nine named checks satisfy the IntegrationDecision shape");
    IntegrationDecision::new(core, (i.clock)())
}

fn proposal(
    i: &AcceptanceInput<'_>,
    d: &IntegrationDecision,
) -> Result<ReviewRecordProposal, String> {
    let core = i.review_request.core();
    let mut b = ProposalBuilder::new(
        i.project_id,
        core.task_id(),
        core.context_epoch().decimal_digits(),
        &derived_id("acceptance-claim", &[i.project_id, d.core().decision_id()]),
    )?;
    let json = encode_integration_decision(d);
    let id = d.core().decision_id();
    b.decision(DecisionRecordKind::IntegrationDecision, id, &json)?;
    let payload = b.content(&format!("review:acceptance:{id}"), json.as_bytes());
    b.event(
        EventType::AcceptanceEvaluated,
        (SubjectKind::Task, core.task_id()),
        payload,
    )?;
    Ok(b.finish())
}
