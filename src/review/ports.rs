//! Review-owned ports. Orchestration delivers `ReviewRequest`s through its
//! own ReviewPort; Routing, the reviewer runtime and durable persistence are
//! reached only through these traits, which the trusted composition (HI-W4-1)
//! binds. Nothing here imports Orchestration or Routing.

use crate::claims::{ReviewClaimBinding, ReviewOperation};
use crate::records::{PackageSpec, ProposalBuilder};
use crate::wire::{decode_a4_review, decode_integration_decision};
use crate::{DistinctProviderPolicy, ReviewCapsuleNonTemporalCore, ReviewRequestReviewerFloor};
use receipts_runtime_adapters::{
    ClaudeAttemptResult, ClaudeProviderResult, CodexAttemptResult, FailureClass,
};
use receipts_state::{
    DecisionRecordKind, DecisionRecordV1, EventProposalV1, ReviewClaimSettlementV1,
};
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
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewRecordProposal {
    pub project_id: String,
    pub decision_records: Vec<DecisionRecordV1>,
    pub events: Vec<EventProposalV1>,
    pub event_keys: Vec<String>,
    /// `(payload reference, exact bytes)` for every event payload.
    pub content: Vec<(String, Vec<u8>)>,
    pub(crate) binding: ReviewClaimBinding,
    pub(crate) spec: PackageSpec,
}

impl ReviewRecordProposal {
    /// The typed identity of this package: operation, scope and claim key.
    pub fn binding(&self) -> &ReviewClaimBinding {
        &self.binding
    }

    /// The opaque claim key of [`Self::binding`].
    pub fn claim_key(&self) -> &str {
        self.binding.key()
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

/// The result of claiming one unit of work (one [`ReviewClaimBinding`])
/// durably. Only `Acquired` authorizes execution.
#[derive(Debug, Clone)]
pub enum ReviewClaim {
    /// This caller now holds the claim and may execute exactly once.
    Acquired,
    /// An earlier or concurrent caller holds the claim and has sealed no
    /// result. Nothing may be executed again: the earlier run may have
    /// reached the reviewer. Needs operator recovery (authorized abandonment,
    /// then a new generation).
    InFlight,
    /// An earlier run sealed its result; complete publication from it.
    Sealed(SealedReview),
    /// The claim finished without a sealed result (a retained terminal
    /// refusal): the recorded outcome token. Nothing to execute or publish.
    Terminal(String),
    /// The claim was explicitly abandoned under an authority. It is not run
    /// again under this generation.
    Abandoned,
}

/// State token naming the format of [`SealedReview::to_bytes`].
pub const SEALED_REVIEW_KIND: &str = "REVIEW_SEALED_V1";

/// A sealed result: the exact package plus the facts a recovering caller
/// needs. Sealed packages are never replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedReview {
    /// Terminal disposition label (`REVIEWED`, `POLICY_BLOCKED`,
    /// `INVALID_OUTPUT`, `REFUSED`, `DISPOSITIONS`, ...). Which labels a
    /// package may carry depends on its [`ReviewOperation`].
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

    /// The scope and operation the sealed package was built for.
    pub fn binding(&self) -> Result<ReviewClaimBinding, String> {
        Ok(self.proposal()?.binding().clone())
    }

    /// The disposition the claim takes when this package is published,
    /// derived only from the package and its outcome label. Refuses a label
    /// the package's operation does not allow, an empty package and a
    /// package whose records do not belong to its scope.
    pub fn settlement(&self) -> Result<ReviewClaimSettlementV1, String> {
        let proposal = self.proposal()?;
        let (operation, scope) = (proposal.binding.operation(), proposal.binding.scope());
        if !operation.outcomes().contains(&self.outcome.as_str()) {
            return Err(format!(
                "outcome {} is not a {} outcome",
                self.outcome,
                operation.as_str()
            ));
        }
        if proposal.decision_records.is_empty() && proposal.events.is_empty() {
            return Err("sealed package holds nothing to publish".into());
        }
        let wanted = match operation {
            ReviewOperation::Review if self.outcome == "REVIEWED" => {
                Some(DecisionRecordKind::A4Review)
            }
            ReviewOperation::Acceptance => Some(DecisionRecordKind::IntegrationDecision),
            _ => None,
        };
        let records = &proposal.decision_records;
        if records.len() != usize::from(wanted.is_some())
            || records.first().map(DecisionRecordV1::kind) != wanted
        {
            return Err("sealed package holds the wrong decision records for its outcome".into());
        }
        let (mut kind, mut reference, mut digest) = (self.outcome.clone(), None, None);
        if let Some(record) = records.first() {
            // The event payload that names the record must carry exactly the
            // record's bytes: the package cannot say two different things.
            let payload = match record.kind() {
                DecisionRecordKind::A4Review => format!("review:a4:{}", record.record_id()),
                _ => format!("review:acceptance:{}", record.record_id()),
            };
            if !proposal
                .content
                .iter()
                .any(|(r, bytes)| *r == payload && bytes == record.record_json().as_bytes())
            {
                return Err("sealed record and its event payload disagree".into());
            }
            reference = Some(record.record_id());
            digest = Some(record.record_sha256());
            if wanted == Some(DecisionRecordKind::A4Review) {
                let review = decode_a4_review(record.record_json())
                    .map_err(|e| format!("sealed review record does not decode: {e:?}"))?;
                let core = review.core();
                if core.reviewed_sha() != scope.subject_sha().unwrap_or_default()
                    || core.task_id() != scope.task_id()
                    || core.review_id() != record.record_id()
                {
                    return Err("sealed review record is not for the claim's scope".into());
                }
                kind = core.verdict().as_str().to_owned();
            } else {
                let decision = decode_integration_decision(record.record_json())
                    .map_err(|e| format!("sealed acceptance record does not decode: {e:?}"))?;
                let chain = decision.core().provenance_chain().unwrap_or_default();
                if chain.len() != 1
                    || chain[0].task_id() != scope.task_id()
                    || Some(chain[0].implementation_sha().as_str()) != scope.subject_sha()
                {
                    return Err("sealed acceptance record is not for the claim's scope".into());
                }
            }
        }
        let refused = matches!(self.outcome.as_str(), "POLICY_BLOCKED" | "REFUSED");
        if refused {
            ReviewClaimSettlementV1::refused(&kind, reference, digest)
        } else {
            ReviewClaimSettlementV1::completed(&kind, reference, digest)
        }
        .map_err(|e| e.to_string())
    }

    /// Durable byte form for the sink's journal: the exact recovery bytes the
    /// trusted core seals ([`SEALED_REVIEW_KIND`]).
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Decodes and fully validates recovery bytes: digest, journal rebuilt
    /// through State's validators, scope/key binding and settlement.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let sealed: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("not a sealed review: {e}"))?;
        sealed.settlement()?;
        Ok(sealed)
    }
}

/// Durable persistence through the trusted core (`CoreStateWriter` in the
/// composition). Review never holds a State writer.
///
/// Contract (the conformance every composition's sink must keep). Every
/// method names its work by the typed [`ReviewClaimBinding`]; a sink derives
/// everything it stores from the binding's scope and operation and refuses a
/// sealed package whose own binding differs.
/// 1. `claim` is an atomic, durable compare-and-set. Exactly one caller of a
///    new binding receives [`ReviewClaim::Acquired`]; the claim survives
///    restart. Any later or concurrent caller gets `InFlight` (nothing sealed
///    yet), `Sealed`, `Terminal` or `Abandoned`. `Err` means no claim was
///    taken and the caller executes nothing.
/// 2. `seal` durably stores the exact package under the claim BEFORE any
///    record is published. Re-sealing identical bytes is a no-op; sealing
///    different bytes, or bytes for another scope, is refused. A sealed
///    package is never replaced.
/// 3. `publish` applies the sealed package completely and idempotently:
///    each decision record and each event (by its stable key) is inserted
///    once with the claim settlement, an identical existing one counts as
///    done, and a conflicting one (including a `proposal` that is not the
///    sealed package) is an error with nothing written. It may be called
///    again after any failure or restart, and returns `Ok` only when the
///    whole package is published. It never replaces a stored record.
pub trait ReviewRecordSink {
    fn claim(&mut self, binding: &ReviewClaimBinding) -> Result<ReviewClaim, String>;
    fn seal(&mut self, binding: &ReviewClaimBinding, sealed: &SealedReview) -> Result<(), String>;
    fn publish(
        &mut self,
        binding: &ReviewClaimBinding,
        proposal: &ReviewRecordProposal,
    ) -> Result<(), String>;
}

/// Trusted composition's payload storage. Bytes are evidence only after the gate
/// hashes them against a persisted event; missing or unreadable content refuses.
pub trait ReviewContentResolver {
    fn resolve(&self, project_id: &str, reference: &str) -> Result<Option<Vec<u8>>, String>;
}
