//! Review-owned ports. Orchestration delivers `ReviewRequest`s through its
//! own ReviewPort; Routing, the reviewer runtime and durable persistence are
//! reached only through these traits, which the trusted composition (HI-W4-1)
//! binds. Nothing here imports Orchestration or Routing.

use crate::{DistinctProviderPolicy, ReviewCapsuleNonTemporalCore, ReviewRequestReviewerFloor};
use receipts_runtime_adapters::{
    ClaudeAttemptResult, ClaudeProviderResult, CodexAttemptResult, FailureClass,
};
use receipts_state::{DecisionRecordV1, EventProposalV1};
use receipts_workspace_execution::WorkspaceHandle;

/// Who produced the implementation under review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementerIdentity {
    pub provider_id: String,
    pub runtime_id: String,
    pub model_id: Option<String>,
    /// The implementing session, when the runtime reported one.
    pub session_ref: Option<String>,
}

/// What the controller asks Routing for. `distinct_provider` is already the
/// raised-only composition of the assurance profile and the request policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerRequirements {
    pub distinct_provider: DistinctProviderPolicy,
    pub reviewer_floor: Option<ReviewRequestReviewerFloor>,
    /// Providers that may not review (the implementer's when REQUIRED).
    pub excluded_providers: Vec<String>,
    /// Sessions that may not review (request exclusions plus the implementer's).
    pub excluded_session_refs: Vec<String>,
}

/// A Routing selection. `resume_session` must be `None`: reviews always run
/// in a fresh session and the controller refuses any resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerCandidate {
    pub provider_id: String,
    pub runtime_id: String,
    pub model_id: Option<String>,
    pub resume_session: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewerRoutingRefusal {
    /// No registered reviewer satisfies the requirements.
    NoEligibleReviewer,
    /// Provider policy refuses this capsule; never shopped elsewhere.
    PolicyBlocked,
}

/// RI-defined routing port (HI-W4-1 adapts Model Routing to it).
pub trait ReviewerRoutingPort {
    fn select_reviewer(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        requirements: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal>;
}

/// What a finished reviewer run produced, interpreted from actual Runtime
/// results only (see [`reviewer_output_from_claude`] and
/// [`reviewer_output_from_codex`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerRunOutput {
    /// Final reviewer text; `None` when the protocol produced none.
    pub text: Option<String>,
    /// Captured output was cut at its bound; never parsed as a verdict.
    pub truncated: bool,
    /// Provider session the reviewer actually ran in, when reported.
    pub session_ref: Option<String>,
    /// Collected from a Workspace protected launch.
    pub protected: bool,
    /// The runtime reports reduced isolation (plan mode / read-only sandbox
    /// are not filesystem confinement).
    pub reduced_isolation: bool,
}

/// One reviewer runtime. Implementations call the frozen
/// `RuntimeAdapter::start` with `RuntimeCapsuleFamily::Review` under a
/// read-only, protected-launch policy, collect the result and interpret it
/// with the functions below. Refusals keep their `FailureClass`; in
/// particular `PolicyBlocked` is never retried elsewhere.
pub trait ReviewerRuntime {
    fn runtime_id(&self) -> &str;
    fn run_review(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        workspace: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass>;
}

/// Interprets a collected Claude result. A non-protected collection, a
/// classified failure or a non-success provider result yields `Err`.
pub fn reviewer_output_from_claude(
    result: &ClaudeAttemptResult,
) -> Result<ReviewerRunOutput, FailureClass> {
    if let Some(failure) = result.failure() {
        return Err(failure);
    }
    if !result.protected() || result.provider_result() != ClaudeProviderResult::Success {
        return Err(FailureClass::Unknown);
    }
    let protocol = result
        .outcome
        .protocol()
        .map_err(|_| FailureClass::Unknown)?;
    let parsed = protocol.result().map_err(|_| FailureClass::Unknown)?;
    let process = result.outcome.process();
    Ok(ReviewerRunOutput {
        text: parsed.text().map(str::to_owned),
        truncated: process.stdout().truncated() || process.stderr().truncated(),
        session_ref: parsed.session_id().map(str::to_owned),
        protected: true,
        reduced_isolation: result.reduced_isolation,
    })
}

/// Interprets a collected Codex result (final agent message only).
pub fn reviewer_output_from_codex(
    result: &CodexAttemptResult,
) -> Result<ReviewerRunOutput, FailureClass> {
    if let Some(failure) = result.failure() {
        return Err(failure);
    }
    if !result.protected() {
        return Err(FailureClass::Unknown);
    }
    let protocol = result
        .outcome
        .protocol()
        .map_err(|_| FailureClass::Unknown)?;
    let process = result.outcome.process();
    Ok(ReviewerRunOutput {
        text: protocol.final_agent_message().map(str::to_owned),
        truncated: process.stdout().truncated() || process.stderr().truncated(),
        session_ref: None,
        protected: true,
        // Codex read-only sandbox is not filesystem confinement either.
        reduced_isolation: true,
    })
}

/// Immutable review records for the trusted core to persist: decision
/// records already admitted by State's shape validator, event proposals,
/// and the exact bytes each proposal's payload digest names.
#[derive(Debug, Clone)]
pub struct ReviewRecordProposal {
    pub project_id: String,
    pub decision_records: Vec<DecisionRecordV1>,
    pub events: Vec<EventProposalV1>,
    /// `(payload reference, exact bytes)` for every event payload.
    pub content: Vec<(String, Vec<u8>)>,
}

/// Durable persistence through the trusted core (`CoreStateWriter` in the
/// composition). Review never holds a State writer.
pub trait ReviewRecordSink {
    fn persist(&mut self, proposal: &ReviewRecordProposal) -> Result<(), String>;
}
