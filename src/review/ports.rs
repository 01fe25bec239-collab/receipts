//! Review-owned ports. Orchestration delivers `ReviewRequest`s through its
//! own ReviewPort; Routing, the reviewer runtime and durable persistence are
//! reached only through these traits, which the trusted composition (HI-W4-1)
//! binds. Nothing here imports Orchestration or Routing.

use crate::records::{PackageSpec, ProposalBuilder};
use crate::{DistinctProviderPolicy, ReviewCapsuleNonTemporalCore, ReviewRequestReviewerFloor};
use receipts_runtime_adapters::{
    ClaudeAttemptResult, ClaudeProviderResult, CodexAttemptResult, FailureClass,
};
use receipts_state::{DecisionRecordV1, EventProposalV1};
use receipts_workspace_execution::WorkspaceHandle;
use serde::{Deserialize, Serialize};

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
    /// The effective, raise-only quality floor: the assurance profile's
    /// BALANCED/FRONTIER minimum, raised (never lowered) by the request.
    pub reviewer_floor: ReviewRequestReviewerFloor,
    /// Providers that may not review (the implementer's when REQUIRED).
    pub excluded_providers: Vec<String>,
    /// Sessions that may not review (request exclusions plus the implementer's).
    pub excluded_session_refs: Vec<String>,
}

/// Routing's evidence for the quality tier of the model it selected: the
/// tier and a reference to the registry/observation row that supports it.
/// The controller can check the tier against the floor and that a basis is
/// named; it cannot authenticate the basis itself (Routing's evidence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerQualityEvidence {
    pub tier: ReviewRequestReviewerFloor,
    pub basis_ref: String,
}

/// A Routing selection. `resume_session` must be `None`: reviews always run
/// in a fresh session and the controller refuses any resume. A missing
/// `quality` is never treated as meeting any floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerCandidate {
    pub provider_id: String,
    pub runtime_id: String,
    pub model_id: Option<String>,
    pub resume_session: Option<String>,
    pub quality: Option<ReviewerQualityEvidence>,
}

/// Ordering of the reviewer quality ladder: ECONOMY < BALANCED < FRONTIER.
pub const fn reviewer_tier_rank(tier: ReviewRequestReviewerFloor) -> u8 {
    match tier {
        ReviewRequestReviewerFloor::Economy => 0,
        ReviewRequestReviewerFloor::Balanced => 1,
        ReviewRequestReviewerFloor::Frontier => 2,
    }
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
/// and the exact bytes each proposal's payload digest names. `event_keys[i]`
/// is the stable idempotency identity of `events[i]`: publishing the same
/// package again must name the same events.
#[derive(Debug, Clone)]
pub struct ReviewRecordProposal {
    pub project_id: String,
    pub decision_records: Vec<DecisionRecordV1>,
    pub events: Vec<EventProposalV1>,
    pub event_keys: Vec<String>,
    /// `(payload reference, exact bytes)` for every event payload.
    pub content: Vec<(String, Vec<u8>)>,
    pub(crate) spec: PackageSpec,
}

impl ReviewRecordProposal {
    /// The durable identity of this package (the claim it completes).
    pub fn claim_key(&self) -> &str {
        &self.spec.claim_key
    }

    /// Plain-data journal form: a sink stores these bytes and can rebuild
    /// the identical package after a restart with [`Self::from_journal`].
    pub fn to_journal(&self) -> Vec<u8> {
        serde_json::to_vec(&self.spec).unwrap_or_default()
    }

    /// Rebuilds the package through State's own validators; refuses a
    /// journal that does not rebuild to exactly what was sealed.
    pub fn from_journal(bytes: &[u8]) -> Result<Self, String> {
        let spec: PackageSpec = serde_json::from_slice(bytes)
            .map_err(|e| format!("journal is not a review package: {e}"))?;
        ProposalBuilder::replay(&spec)
    }
}

/// The result of claiming one attempt (or one disposition set) durably.
#[derive(Debug, Clone)]
pub enum ReviewClaim {
    /// This caller now holds the claim and may execute exactly once.
    Acquired,
    /// An earlier or concurrent caller holds the claim and has sealed no
    /// result. Nothing may be executed again: the earlier run may have
    /// reached the reviewer. Needs operator recovery.
    InFlight,
    /// An earlier run sealed its result; complete publication from it.
    Sealed(SealedReview),
}

/// A sealed result: the exact package plus the facts a recovering caller
/// needs. Sealed packages are never replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedReview {
    /// Terminal disposition label (`REVIEWED`, `POLICY_BLOCKED`,
    /// `INVALID_OUTPUT`, `REFUSED`, `DISPOSITIONS`, ...).
    pub outcome: String,
    /// The owned review worktree was removed.
    pub cleanup_ok: bool,
    /// Reviewer dispatches the original run performed.
    pub dispatches: usize,
    journal: String,
    /// SHA-256 of `journal`: a sealed package that no longer matches it is
    /// refused rather than published.
    journal_sha256: String,
}

impl SealedReview {
    pub fn new(
        proposal: &ReviewRecordProposal,
        outcome: impl Into<String>,
        cleanup_ok: bool,
        dispatches: usize,
    ) -> Self {
        Self {
            outcome: outcome.into(),
            cleanup_ok,
            dispatches,
            journal: String::from_utf8_lossy(&proposal.to_journal()).into_owned(),
            journal_sha256: crate::records::sha256_hex(&proposal.to_journal()),
        }
    }

    pub fn proposal(&self) -> Result<ReviewRecordProposal, String> {
        if crate::records::sha256_hex(self.journal.as_bytes()) != self.journal_sha256 {
            return Err("sealed journal does not match its digest".into());
        }
        ReviewRecordProposal::from_journal(self.journal.as_bytes())
    }

    /// Durable byte form for the sink's journal.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let sealed: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("not a sealed review: {e}"))?;
        sealed.proposal()?;
        Ok(sealed)
    }
}

/// Durable persistence through the trusted core (`CoreStateWriter` in the
/// composition). Review never holds a State writer.
///
/// Contract (the conformance every composition's sink must keep):
/// 1. `claim` is an atomic, durable compare-and-set keyed by `claim_key`.
///    Exactly one caller of a new key receives [`ReviewClaim::Acquired`];
///    the claim survives restart. Any later or concurrent caller gets
///    `InFlight` (nothing sealed yet) or `Sealed`. `Err` means no claim was
///    taken and the caller executes nothing.
/// 2. `seal` durably stores the exact package under the claim BEFORE any
///    record is published. Re-sealing identical bytes is a no-op; sealing
///    different bytes is refused. A sealed package is never replaced.
/// 3. `publish` applies a sealed package completely and idempotently:
///    each decision record and each event (by `event_keys`) is inserted
///    once, an identical existing one counts as done, and a conflicting
///    existing one is an error. It may be called again after any failure
///    or restart, and returns `Ok` only when the whole package is
///    published. It never replaces a stored record.
pub trait ReviewRecordSink {
    fn claim(&mut self, claim_key: &str) -> Result<ReviewClaim, String>;
    fn seal(&mut self, claim_key: &str, sealed: &SealedReview) -> Result<(), String>;
    fn publish(&mut self, claim_key: &str, proposal: &ReviewRecordProposal) -> Result<(), String>;
}
