//! Review's durable sink over the trusted core and real SQLite State.
//!
//! [`CoreReviewSink`] is the library-integration binding of
//! `ReviewRecordSink` to `CoreStateWriter`. It keeps no durable authority of
//! its own: the claim, the exact sealed recovery bytes and the published
//! package all live in State, written only through the core.
//!
//! * `claim` turns the typed `ReviewClaimBinding` into a `CoreReviewUnitV1`
//!   (project, task, subject SHA, request digest, generation) and asks the
//!   core. Only a fresh claim returns `Acquired`; in-flight, terminal and
//!   abandoned claims never authorize execution, and a sealed claim returns
//!   the sealed result from State's stored bytes, re-validated.
//! * `seal` hands the core the exact bytes of `SealedReview::to_bytes()`; the
//!   core decodes them with [`ReviewSealedCodec`], resolves and hashes every
//!   event payload, and State stores the bytes before anything is published.
//! * `publish` never takes a package from the caller: it reads the seal State
//!   holds, refuses a `proposal` that is not that package, and lets the core
//!   rebuild and publish it with the claim settlement in one State
//!   transaction. It is idempotent and works identically live and after a
//!   restart; the reviewer is never involved.
//!
//! A claim that was taken and never sealed (a crash between the claim and the
//! seal) stays in flight: nothing re-executes it. It is ended only by
//! [`CoreReviewSink::abandon`] under an admitted authority, and the work then
//! needs a new generation from the trusted composition.

use receipts_orchestration::orchestration::{
    CoreAbandonmentGrantV1, CoreContentResolverV1, CoreEventV1, CorePublicationInputV1,
    CoreRecoveredReviewV1, CoreRecoveryCodecV1, CoreReviewClaimV1, CoreReviewUnitV1,
    CoreStateWriter,
};
use receipts_review_integration::state::{ReviewClaimRecordV1, TrustedClockV1};
use receipts_review_integration::{
    ReviewClaim, ReviewClaimBinding, ReviewRecordProposal, ReviewRecordSink, SEALED_REVIEW_KIND,
    SealedReview,
};

/// The core's unit of work for `binding`: exactly the typed scope.
fn unit_of(binding: &ReviewClaimBinding) -> Result<CoreReviewUnitV1, String> {
    let scope = binding.scope();
    CoreReviewUnitV1::new(
        scope.project_id(),
        scope.task_id(),
        scope.subject_sha(),
        scope.request_sha256(),
        scope.generation(),
    )
    .map_err(|refusal| format!("the core refused the claim scope: {refusal:?}"))
}

/// The typed inputs of the core publication for a rebuilt package: records,
/// events with their stable keys, no availability facts.
pub fn publication_input(
    proposal: &ReviewRecordProposal,
) -> Result<CorePublicationInputV1, String> {
    if proposal.events.len() != proposal.event_keys.len() {
        return Err("package events and event keys disagree".into());
    }
    Ok(CorePublicationInputV1 {
        project_id: proposal.project_id.clone(),
        records: proposal.decision_records.clone(),
        events: proposal
            .events
            .iter()
            .zip(&proposal.event_keys)
            .map(|(event, key)| CoreEventV1::new(event.clone(), key.clone()))
            .collect(),
        availability: Vec::new(),
    })
}

/// Event payload bytes of one package, scoped to its project. The core hashes
/// every resolved payload against the digest the event names.
#[derive(Debug, Clone)]
pub struct SealedContent {
    project_id: String,
    content: Vec<(String, Vec<u8>)>,
}

impl SealedContent {
    pub fn of(proposal: &ReviewRecordProposal) -> Self {
        Self {
            project_id: proposal.project_id.clone(),
            content: proposal.content.clone(),
        }
    }
}

impl CoreContentResolverV1 for SealedContent {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        (project_id == self.project_id)
            .then(|| {
                self.content
                    .iter()
                    .find(|(r, _)| r == reference)
                    .map(|(_, bytes)| bytes.clone())
            })
            .flatten()
    }
}

/// Decodes sealed review bytes (`SEALED_REVIEW_KIND`) into the typed package
/// and claim disposition. It validates integrity, State shape, the scope/key
/// binding and the outcome, never returns a validity flag, and refuses bytes
/// sealed for any scope or operation other than the one it was built for.
#[derive(Debug, Clone)]
pub struct ReviewSealedCodec {
    expected: ReviewClaimBinding,
}

impl ReviewSealedCodec {
    /// Accepts only bytes sealed for exactly `binding`.
    pub fn for_binding(binding: &ReviewClaimBinding) -> Self {
        Self {
            expected: binding.clone(),
        }
    }
}

impl CoreRecoveryCodecV1 for ReviewSealedCodec {
    fn kind(&self) -> &str {
        SEALED_REVIEW_KIND
    }

    fn decode(&self, project_id: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String> {
        let sealed = SealedReview::from_bytes(bytes)?;
        let proposal = sealed.proposal()?;
        if proposal.binding().scope().project_id() != project_id {
            return Err("sealed package belongs to another project".into());
        }
        if *proposal.binding() != self.expected {
            return Err("sealed package belongs to another claim scope or operation".into());
        }
        Ok(CoreRecoveredReviewV1 {
            input: publication_input(&proposal)?,
            settlement: sealed.settlement()?,
        })
    }
}

/// `ReviewRecordSink` over `CoreStateWriter`. See the module docs.
pub struct CoreReviewSink {
    writer: CoreStateWriter,
    clock: Box<dyn TrustedClockV1 + Send>,
}

impl CoreReviewSink {
    /// `clock` is the composition's trusted clock; State stamps claims, seals
    /// and events with it, never with anything Review supplies.
    pub fn new(writer: CoreStateWriter, clock: impl TrustedClockV1 + Send + 'static) -> Self {
        Self {
            writer,
            clock: Box::new(clock),
        }
    }

    /// Read access for the composition and its controls.
    pub fn writer(&self) -> &CoreStateWriter {
        &self.writer
    }

    /// The core's claim id for `binding`: what an admitted identity needs to
    /// mint an abandonment grant for it.
    pub fn claim_id(binding: &ReviewClaimBinding) -> Result<String, String> {
        Ok(unit_of(binding)?.claim_id())
    }

    /// Ends an unfinished claim without running anything, under the authority
    /// the writer's admitted identity names (the grant must be minted by that
    /// identity for exactly this claim). The work then needs a new generation.
    pub fn abandon(
        &mut self,
        binding: &ReviewClaimBinding,
        grant: CoreAbandonmentGrantV1,
    ) -> Result<ReviewClaimRecordV1, String> {
        self.writer
            .abandon_review_claim(&*self.clock, &unit_of(binding)?, grant)
            .map_err(|e| e.to_string())
    }
}

impl ReviewRecordSink for CoreReviewSink {
    fn claim(&mut self, binding: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
        let unit = unit_of(binding)?;
        match self
            .writer
            .claim_review(&*self.clock, &unit)
            .map_err(|e| e.to_string())?
        {
            CoreReviewClaimV1::Acquired => Ok(ReviewClaim::Acquired),
            CoreReviewClaimV1::InFlight(_) => Ok(ReviewClaim::InFlight),
            CoreReviewClaimV1::Terminal(record) => Ok(ReviewClaim::Terminal(
                record.outcome_kind.unwrap_or_else(|| "UNKNOWN".into()),
            )),
            CoreReviewClaimV1::Abandoned(_) => Ok(ReviewClaim::Abandoned),
            CoreReviewClaimV1::Sealed(sealed) => {
                if sealed.seal.recovery_kind != SEALED_REVIEW_KIND {
                    return Err("the stored seal is not a sealed review".into());
                }
                let review = SealedReview::from_bytes(&sealed.seal.recovery_data)?;
                if review.binding()? != *binding {
                    return Err("the stored seal belongs to another claim scope".into());
                }
                Ok(ReviewClaim::Sealed(review))
            }
        }
    }

    fn seal(&mut self, binding: &ReviewClaimBinding, sealed: &SealedReview) -> Result<(), String> {
        let proposal = sealed.proposal()?;
        if proposal.binding() != binding {
            return Err("the sealed package belongs to another claim scope".into());
        }
        sealed.settlement()?;
        self.writer
            .seal_review(
                &*self.clock,
                &SealedContent::of(&proposal),
                &ReviewSealedCodec::for_binding(binding),
                &unit_of(binding)?,
                sealed.to_bytes(),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn publish(
        &mut self,
        binding: &ReviewClaimBinding,
        proposal: &ReviewRecordProposal,
    ) -> Result<(), String> {
        if proposal.binding() != binding {
            return Err("the package belongs to another claim scope".into());
        }
        let unit = unit_of(binding)?;
        let stored = self
            .writer
            .sealed_review(&unit)
            .map_err(|e| e.to_string())?
            .ok_or("nothing is sealed for this claim")?;
        // What gets published is what State sealed, never the caller's copy.
        let sealed = SealedReview::from_bytes(&stored.seal.recovery_data)?;
        if sealed.proposal()? != *proposal {
            return Err("the package is not the one sealed for this claim".into());
        }
        self.writer
            .publish_sealed_review(
                &*self.clock,
                &SealedContent::of(&sealed.proposal()?),
                &ReviewSealedCodec::for_binding(binding),
                &unit,
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
