//! Typed decision/event/availability publication built inside Core.
//!
//! Core never forwards an opaque State package. A caller hands over the typed
//! inputs; Core admits the project, resolves and hashes EVERY event payload
//! through the composition's [`CoreContentResolverV1`], derives stable
//! publication and event identities from caller-owned keys, builds the State
//! package itself and publishes it in one State transaction. Any refusal
//! writes nothing.
//!
//! Identities are deterministic so a retry or a rebuild after a restart is
//! the same package: the publication id comes from `(project, publication
//! key)` and each event id from the publication id and the event's key.

use receipts_state::{
    AvailabilityFactV1, DecisionPublicationReceiptV1, DecisionPublicationV1, DecisionRecordV1,
    EventProposalV1, PublicationEventV1, StateWriteOperationV1 as Op, TrustedClockV1,
};
use sha2::{Digest, Sha256};

use super::state_write_authority::verify_event_content;
use super::{CoreContentResolverV1, CorePolicyRefusal, CoreStateWriteError, CoreStateWriter};

/// One event of a package: the lower-trust proposal and the stable key (unique
/// within the package, owned by the caller) its event id derives from.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreEventV1 {
    proposal: EventProposalV1,
    event_key: String,
}

impl CoreEventV1 {
    pub fn new(proposal: EventProposalV1, event_key: impl Into<String>) -> Self {
        Self {
            proposal,
            event_key: event_key.into(),
        }
    }

    pub fn proposal(&self) -> &EventProposalV1 {
        &self.proposal
    }

    pub fn event_key(&self) -> &str {
        &self.event_key
    }
}

/// The typed inputs of one publication. Every item must belong to
/// `project_id`; the package must hold at least one item.
#[derive(Debug, Clone, PartialEq)]
pub struct CorePublicationInputV1 {
    pub project_id: String,
    pub records: Vec<DecisionRecordV1>,
    pub events: Vec<CoreEventV1>,
    pub availability: Vec<AvailabilityFactV1>,
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_key(key: &str) -> bool {
    (1..=200).contains(&key.chars().count()) && !key.chars().any(char::is_control)
}

/// The deterministic State publication id of `(project_id, publication_key)`.
pub fn publication_id_v1(project_id: &str, publication_key: &str) -> String {
    let digest = Sha256::digest(
        serde_json::json!(["receipts.core.publication.v1", project_id, publication_key])
            .to_string()
            .as_bytes(),
    );
    format!("pub-{}", lower_hex(&digest))
}

/// Builds the State package for `input` under `publication_id`: every event's
/// content is resolved and hashed, event ids are stable, and State's own
/// constructor checks project binding and uniqueness.
pub(super) fn build_package(
    content: &dyn CoreContentResolverV1,
    publication_id: &str,
    input: &CorePublicationInputV1,
) -> Result<DecisionPublicationV1, CoreStateWriteError> {
    let mut events = Vec::with_capacity(input.events.len());
    for event in &input.events {
        if event.proposal.project_id() != input.project_id {
            return Err(CorePolicyRefusal::InvalidShape.into());
        }
        verify_event_content(content, &event.proposal)?;
        events.push(PublicationEventV1::stable(
            event.proposal.clone(),
            publication_id,
            &event.event_key,
        )?);
    }
    Ok(DecisionPublicationV1::new(
        &input.project_id,
        publication_id,
        input.records.clone(),
        events,
        input.availability.clone(),
    )?)
}

impl CoreStateWriter {
    /// Publishes records, events (with receipts) and availability facts as one
    /// atomic State package identified by `publication_key`. An exact retry
    /// returns the recorded receipt with `replayed = true`; the same key with
    /// different content, or a record or event identity that already exists
    /// outside a recorded publication, is refused with nothing written. A stored
    /// `REJECT` is never replaced.
    ///
    /// This is not the review path: a review outcome follows
    /// claim → seal → [`Self::publish_sealed_review`].
    pub fn publish_decision_package(
        &mut self,
        clock: &dyn TrustedClockV1,
        content: &dyn CoreContentResolverV1,
        publication_key: &str,
        input: &CorePublicationInputV1,
    ) -> Result<DecisionPublicationReceiptV1, CoreStateWriteError> {
        self.admitted_source(&input.project_id)?;
        if !valid_key(publication_key) {
            return Err(CorePolicyRefusal::InvalidShape.into());
        }
        let id = publication_id_v1(&input.project_id, publication_key);
        let package = build_package(content, &id, input)?;
        let admission = self.admit(Op::DecisionPublication, &input.project_id, None)?;
        Ok(self
            .store
            .publish_decision_package_v1(admission, clock, &package)?)
    }
}
