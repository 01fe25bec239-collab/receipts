//! Review claim → seal → publish, composed in Core.
//!
//! A review is one unit of work, [`CoreReviewUnitV1`], with a deterministic
//! claim identity. The only path to execute it is
//!
//! 1. [`CoreStateWriter::claim_review`] returns [`CoreReviewClaimV1::Acquired`]
//!    (exactly once per unit, ever: the claim is durable);
//! 2. the claimant executes and calls [`CoreStateWriter::seal_review`] with the
//!    exact recovery bytes of its result BEFORE anything is published;
//! 3. [`CoreStateWriter::publish_sealed_review`] rebuilds the package from the
//!    sealed bytes and publishes it with the claim settlement in one State
//!    transaction. It never takes a package from the caller, so what is
//!    published is always what was sealed, live or after a restart.
//!
//! Any other state is never executed again: an unsealed claim (the earlier run
//! may have reached the reviewer) is [`CoreReviewClaimV1::InFlight`] until an
//! explicitly authorized abandonment ends it; re-running then needs a new
//! `generation`. Plain settlement is only available for a terminal refusal
//! with nothing to publish, so it can never discard review records or events.
//!
//! State never parses the recovery bytes. Their format belongs to the
//! reviewer, which Core must not depend on, so the trusted composition
//! supplies a [`CoreRecoveryCodecV1`]: Core calls it to decode the bytes into
//! typed package inputs and checks the result itself, before sealing and again
//! before every rebuild.

use receipts_state::{
    ReviewClaimAbandonV1, ReviewClaimOutcomeV1, ReviewClaimRecordV1, ReviewClaimRequestV1,
    ReviewClaimSettlementV1, ReviewClaimStateV1, ReviewRecoveryDataV1, ReviewSealRecordV1,
    ReviewSealRequestV1, StateError, StateWriteOperationV1 as Op, TrustedClockV1,
};
use sha2::{Digest, Sha256};

use super::decision_publication::{CorePublicationInputV1, build_package, publication_id_v1};
use super::{
    CoreAbandonmentGrantV1, CoreContentResolverV1, CorePolicyRefusal, CoreStateWriteError,
    CoreStateWriter,
};
use receipts_state::DecisionPublicationReceiptV1;

/// One unit of review work. Its claim identity is a pure function of the
/// fields, so every callback for the same work names the same claim.
/// `generation` starts at 1; a new generation is the only way to run the work
/// again after its claim was abandoned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreReviewUnitV1 {
    project_id: String,
    task_id: String,
    subject_sha: Option<String>,
    request_sha256: String,
    generation: u32,
}

impl CoreReviewUnitV1 {
    /// `subject_sha` is the 40-hex SHA under review when there is one and
    /// `request_sha256` the 64-hex SHA-256 of the reviewer's request bytes.
    pub fn new(
        project_id: &str,
        task_id: &str,
        subject_sha: Option<&str>,
        request_sha256: &str,
        generation: u32,
    ) -> Result<Self, CorePolicyRefusal> {
        let unit = Self {
            project_id: project_id.to_owned(),
            task_id: task_id.to_owned(),
            subject_sha: subject_sha.map(str::to_owned),
            request_sha256: request_sha256.to_owned(),
            generation,
        };
        if generation == 0 || unit.claim_request().is_err() {
            return Err(CorePolicyRefusal::InvalidShape);
        }
        Ok(unit)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// The State claim id: `review-<sha256 of the unit's fields>`.
    pub fn claim_id(&self) -> String {
        let digest = Sha256::digest(
            serde_json::json!([
                "receipts.core.review-claim.v1",
                self.project_id,
                self.task_id,
                self.subject_sha,
                self.request_sha256,
                self.generation,
            ])
            .to_string()
            .as_bytes(),
        );
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("review-{hex}")
    }

    /// The State publication id the unit's sealed package is published under.
    pub fn publication_id(&self) -> String {
        publication_id_v1(&self.project_id, &self.claim_id())
    }

    fn claim_request(&self) -> Result<ReviewClaimRequestV1, StateError> {
        ReviewClaimRequestV1::new(
            &self.project_id,
            &self.claim_id(),
            &self.task_id,
            self.subject_sha.as_deref(),
            &self.request_sha256,
        )
    }

    fn key(&self) -> (String, String) {
        (self.project_id.clone(), self.claim_id())
    }
}

/// What a decoded recovery blob denotes: the package to publish and the
/// disposition the claim takes when it is published.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreRecoveredReviewV1 {
    pub input: CorePublicationInputV1,
    pub settlement: ReviewClaimSettlementV1,
}

/// The trusted composition's decoder for sealed recovery bytes. It owns the
/// byte format (the reviewer's sealed result) and must fully validate the
/// bytes: integrity, shape and that they rebuild the package they claim to.
/// It returns typed values, never a validity flag. Core still checks the
/// project, rebuilds the State package itself and compares it with the seal.
pub trait CoreRecoveryCodecV1 {
    /// The State recovery-kind token of the bytes this codec decodes (for
    /// example `REVIEW_SEALED_V1`).
    fn kind(&self) -> &str;
    fn decode(&self, project_id: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String>;
}

/// The durable seal of a claim and the claim it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSealedReviewV1 {
    /// `claim.state` is `Completed` or `Refused` once the sealed package was
    /// published, `Claimed` before.
    pub claim: ReviewClaimRecordV1,
    /// `seal.recovery_data` are the exact sealed bytes.
    pub seal: ReviewSealRecordV1,
}

/// The result of [`CoreStateWriter::claim_review`]. Only `Acquired` permits
/// execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreReviewClaimV1 {
    /// This writer holds the live claim: execute exactly once, then seal.
    Acquired,
    /// Claimed and not sealed (by an earlier process or a concurrent caller).
    /// Never execute: the earlier run may have reached the reviewer. Needs an
    /// explicitly authorized abandonment and then a new generation.
    InFlight(ReviewClaimRecordV1),
    /// Sealed: publish from the seal ([`CoreStateWriter::publish_sealed_review`])
    /// without executing. Also returned for a finished claim, where the
    /// publication replays exactly.
    Sealed(Box<CoreSealedReviewV1>),
    /// Finished without a seal (a terminal refusal): nothing to execute or
    /// publish.
    Terminal(ReviewClaimRecordV1),
    /// Explicitly abandoned: not executed again under this generation.
    Abandoned(ReviewClaimRecordV1),
}

impl CoreStateWriter {
    /// Claims `unit` BEFORE it is routed or executed. A live ticket for a new
    /// claim stays inside the writer; it neither leaves it nor survives a
    /// restart.
    pub fn claim_review(
        &mut self,
        clock: &dyn TrustedClockV1,
        unit: &CoreReviewUnitV1,
    ) -> Result<CoreReviewClaimV1, CoreStateWriteError> {
        self.admitted_source(unit.project_id())?;
        let request = unit.claim_request()?;
        let admission = self.admit(Op::ReviewClaim, unit.project_id(), None)?;
        let record = match self.store.claim_review_v1(admission, clock, &request)? {
            ReviewClaimOutcomeV1::Claimed(ticket) => {
                self.review_tickets.insert(unit.key(), ticket);
                return Ok(CoreReviewClaimV1::Acquired);
            }
            ReviewClaimOutcomeV1::Existing(record) => record,
        };
        let seal = self
            .reader()
            .find_review_seal(&record.project_id, &record.claim_id)?;
        Ok(match (record.state, seal) {
            (ReviewClaimStateV1::Abandoned, _) => CoreReviewClaimV1::Abandoned(record),
            (_, Some(seal)) => CoreReviewClaimV1::Sealed(Box::new(CoreSealedReviewV1 {
                claim: record,
                seal,
            })),
            (ReviewClaimStateV1::Claimed, None) => CoreReviewClaimV1::InFlight(record),
            (_, None) => CoreReviewClaimV1::Terminal(record),
        })
    }

    /// The sealed outcome of `unit`, read-only (for example after a restart).
    pub fn sealed_review(
        &self,
        unit: &CoreReviewUnitV1,
    ) -> Result<Option<CoreSealedReviewV1>, CoreStateWriteError> {
        self.admitted_source(unit.project_id())?;
        let claim_id = unit.claim_id();
        let reader = self.reader();
        Ok(
            match (
                reader.find_review_claim(unit.project_id(), &claim_id)?,
                reader.find_review_seal(unit.project_id(), &claim_id)?,
            ) {
                (Some(claim), Some(seal)) => Some(CoreSealedReviewV1 { claim, seal }),
                _ => None,
            },
        )
    }

    /// Durably seals the claimant's exact result before anything is published.
    /// Only the writer that acquired the claim can seal. `recovery_bytes` are
    /// decoded by `codec`; the package, its stable identities and the claim
    /// disposition all come from that decoding, every event payload is
    /// resolved and hashed, and the bytes are sealed only if all of it holds.
    /// An identical reseal returns the stored seal; different bytes or a
    /// different package are refused by State.
    pub fn seal_review(
        &mut self,
        clock: &dyn TrustedClockV1,
        content: &dyn CoreContentResolverV1,
        codec: &dyn CoreRecoveryCodecV1,
        unit: &CoreReviewUnitV1,
        recovery_bytes: Vec<u8>,
    ) -> Result<ReviewSealRecordV1, CoreStateWriteError> {
        self.admitted_source(unit.project_id())?;
        let recovered = decode(codec, unit.project_id(), &recovery_bytes)?;
        let package = build_package(content, &unit.publication_id(), &recovered.input)?;
        let ticket = self
            .review_tickets
            .get(&unit.key())
            .ok_or(CorePolicyRefusal::NoLiveClaim)?;
        let package = package.with_claim_settlement(ticket, recovered.settlement)?;
        let digest = Sha256::digest(&recovery_bytes);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let request = ReviewSealRequestV1::new(
            &unit.claim_request()?,
            &package,
            ReviewRecoveryDataV1::new(codec.kind(), recovery_bytes, &hex)?,
        )?;
        let admission = self.admit(Op::ReviewClaim, unit.project_id(), None)?;
        Ok(self
            .store
            .seal_review_claim_v1(admission, clock, ticket, &request)?)
    }

    /// Rebuilds the sealed package from the seal State stored and publishes it
    /// with the claim settlement atomically. The caller supplies no package, so
    /// the same call completes a live run and a run recovered after a restart,
    /// and runs no reviewer. An exact retry replays the recorded receipt.
    pub fn publish_sealed_review(
        &mut self,
        clock: &dyn TrustedClockV1,
        content: &dyn CoreContentResolverV1,
        codec: &dyn CoreRecoveryCodecV1,
        unit: &CoreReviewUnitV1,
    ) -> Result<DecisionPublicationReceiptV1, CoreStateWriteError> {
        self.admitted_source(unit.project_id())?;
        let seal = self
            .reader()
            .find_review_seal(unit.project_id(), &unit.claim_id())?
            .ok_or(CorePolicyRefusal::NotSealed)?;
        let invalid = CorePolicyRefusal::RecoveryDataInvalid;
        if seal.task_id != unit.task_id
            || seal.subject_sha != unit.subject_sha
            || seal.request_sha256 != unit.request_sha256
            || seal.publication_id != unit.publication_id()
            || seal.recovery_kind != codec.kind()
        {
            return Err(invalid.into());
        }
        let recovered = decode(codec, unit.project_id(), &seal.recovery_data)?;
        if recovered.settlement != seal.settlement {
            return Err(invalid.into());
        }
        let package = build_package(content, &seal.publication_id, &recovered.input)?
            .with_sealed_claim(&seal)?;
        if package.sha256() != seal.package_sha256 {
            return Err(invalid.into());
        }
        let admission = self.admit(Op::DecisionPublication, unit.project_id(), None)?;
        let receipt = self
            .store
            .publish_decision_package_v1(admission, clock, &package)?;
        if receipt
            .claim
            .as_ref()
            .is_some_and(|claim| claim.state != ReviewClaimStateV1::Claimed)
        {
            self.review_tickets.remove(&unit.key());
        }
        Ok(receipt)
    }

    /// Settles the live claim as a terminal refusal with nothing to publish
    /// (for example a policy block found before any execution). Completion is
    /// refused: a completed review is only ever the publication of its sealed
    /// package, and State refuses plain settlement once a seal exists.
    pub fn settle_review_refusal(
        &mut self,
        clock: &dyn TrustedClockV1,
        unit: &CoreReviewUnitV1,
        settlement: &ReviewClaimSettlementV1,
    ) -> Result<ReviewClaimRecordV1, CoreStateWriteError> {
        self.admitted_source(unit.project_id())?;
        if settlement.is_completed() {
            return Err(CorePolicyRefusal::CompletionRequiresPublication.into());
        }
        let ticket = self
            .review_tickets
            .get(&unit.key())
            .ok_or(CorePolicyRefusal::NoLiveClaim)?;
        let admission = self.admit(Op::ReviewClaim, unit.project_id(), None)?;
        let record = self
            .store
            .settle_review_claim_v1(admission, clock, ticket, settlement)?;
        self.review_tickets.remove(&unit.key());
        Ok(record)
    }

    /// Ends an unfinished claim without re-running anything, under the
    /// authority the admitted identity names. The grant must have been minted
    /// by this writer's identity for exactly this claim; a name alone, a
    /// project without admitted authority or a foreign identity is refused
    /// before State. The authority recorded on the claim is the admitted one.
    pub fn abandon_review_claim(
        &mut self,
        clock: &dyn TrustedClockV1,
        unit: &CoreReviewUnitV1,
        grant: CoreAbandonmentGrantV1,
    ) -> Result<ReviewClaimRecordV1, CoreStateWriteError> {
        let claim_id = unit.claim_id();
        let authorized = self.identity.owns(&grant.mark)
            && grant.project_id == unit.project_id()
            && grant.claim_id == claim_id
            && self.identity.abandonment_authority(unit.project_id())
                == Some(grant.authority_ref.as_str());
        if !authorized {
            return Err(CorePolicyRefusal::AbandonmentNotAuthorized.into());
        }
        self.admitted_source(unit.project_id())?;
        let request =
            ReviewClaimAbandonV1::new(unit.project_id(), &claim_id, &grant.authority_ref)?;
        let admission = self.admit(Op::ReviewClaim, unit.project_id(), None)?;
        let record = self
            .store
            .abandon_review_claim_v1(admission, clock, &request)?;
        self.review_tickets.remove(&unit.key());
        Ok(record)
    }
}

fn decode(
    codec: &dyn CoreRecoveryCodecV1,
    project_id: &str,
    bytes: &[u8],
) -> Result<CoreRecoveredReviewV1, CoreStateWriteError> {
    codec
        .decode(project_id, bytes)
        .ok()
        .filter(|recovered| recovered.input.project_id == project_id)
        .ok_or_else(|| CorePolicyRefusal::RecoveryDataInvalid.into())
}
