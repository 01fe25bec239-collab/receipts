//! Routing's atomic library adapter over Core admissions and real SQLite.
//!
//! The trusted composition supplies the project, writer, clock and admitted
//! request resolver. Event-attached bytes never admit a request. Record
//! references resolve from typed records rebuilt by the caller after restart,
//! and Core independently hashes every resolved payload before publication.

use receipts_model_routing::persistence::{
    Publication, PublicationReceipt, RoutingStatePort, record_reference, sha256_hex,
};
use receipts_orchestration::orchestration::{
    CoreContentResolverV1, CoreEventV1, CorePolicyRefusal, CorePublicationInputV1,
    CoreStateWriteError, CoreStateWriter,
};
use receipts_review_integration::state::{SqliteStateRepository, TrustedClockV1};

/// A project-bound Routing port. It holds no journal: State owns replay.
pub struct CoreRoutingState<'a> {
    writer: &'a mut CoreStateWriter,
    clock: &'a dyn TrustedClockV1,
    requests: &'a dyn CoreContentResolverV1,
    project_id: &'a str,
}

impl<'a> CoreRoutingState<'a> {
    pub fn new(
        writer: &'a mut CoreStateWriter,
        clock: &'a dyn TrustedClockV1,
        requests: &'a dyn CoreContentResolverV1,
        project_id: &'a str,
    ) -> Self {
        Self {
            writer,
            clock,
            requests,
            project_id,
        }
    }
}

struct Content<'a> {
    project_id: &'a str,
    publication: &'a Publication,
    requests: &'a dyn CoreContentResolverV1,
}

impl CoreContentResolverV1 for Content<'_> {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        if project_id != self.project_id {
            return None;
        }
        if reference.starts_with("routing-request:") {
            return self.requests.resolve(project_id, reference);
        }
        if let Some(record) = self
            .publication
            .records
            .iter()
            .find(|r| r.project_id() == project_id && record_reference(r) == reference)
        {
            return Some(record.record_json().as_bytes().to_vec());
        }
        None
    }
}

impl RoutingStatePort for CoreRoutingState<'_> {
    type Error = CoreStateWriteError;

    fn reader(&self) -> &SqliteStateRepository {
        self.writer.reader()
    }

    fn publish(&mut self, publication: &Publication) -> Result<PublicationReceipt, Self::Error> {
        if publication.events.iter().any(|event| {
            event.proposal.payload().digest != format!("sha256:{}", sha256_hex(&event.content))
        }) {
            return Err(CorePolicyRefusal::ContentDigestMismatch.into());
        }
        // The first append slot or request reference identifies the operation,
        // independent of payload bytes, sibling count, digests and outcomes.
        let logical = if let Some(record) = publication.records.first() {
            record_reference(record)
        } else if let [event] = publication.events.as_slice()
            && event
                .proposal
                .payload()
                .reference
                .starts_with("routing-request:")
        {
            event.proposal.payload().reference.clone()
        } else {
            return Err(CorePolicyRefusal::InvalidShape.into());
        };
        let key = format!("routing-v1-{}", sha256_hex(logical.as_bytes()));
        let input = CorePublicationInputV1 {
            project_id: self.project_id.to_owned(),
            records: publication.records.clone(),
            events: publication
                .events
                .iter()
                .enumerate()
                .map(|(i, event)| {
                    CoreEventV1::new(event.proposal.clone(), format!("routing-event-{i}"))
                })
                .collect(),
            availability: publication.fact.iter().cloned().collect(),
        };
        let content = Content {
            project_id: self.project_id,
            publication,
            requests: self.requests,
        };
        let receipt = self
            .writer
            .publish_decision_package(self.clock, &content, &key, &input)?;
        Ok(PublicationReceipt {
            events: receipt.events,
            fact: receipt.availability.into_iter().next(),
            replayed: receipt.replayed,
        })
    }
}
