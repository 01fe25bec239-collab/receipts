//! Shared atomic publication boundaries (two typed operations).
//!
//! # Graph mutation with its results
//!
//! [`GraphMutationPublicationV1`] carries the node-result rows a mutation
//! attaches, the mutation evidence and the successor snapshot.
//! [`TrustedCoreStore::publish_graph_mutation_v1`](crate::TrustedCoreStore::publish_graph_mutation_v1)
//! writes results, snapshot, mutation history, seal, trusted-time watermark
//! and the head CAS in ONE serialized transaction: any failure leaves none of
//! them. An exact retry of an applied mutation (same mutation bytes, same
//! snapshot digest, same result rows) returns
//! [`GraphPublicationOutcomeV1::Replayed`] without writing; the same mutation
//! id with different content refuses. A result row already stored with the
//! identical version, node and bytes (for example left by the older
//! one-row-per-transaction writer) is adopted, never replaced.
//!
//! # Decision publication
//!
//! [`DecisionPublicationV1`] is one admitted package: decision records, the
//! related event proposals (each with its trusted-core-minted event id), and
//! the availability facts that belong with them, optionally together with the
//! settlement of the caller's review claim.
//! [`TrustedCoreStore::publish_decision_package_v1`](crate::TrustedCoreStore::publish_decision_package_v1)
//! samples trusted time once and writes the publication identity, records,
//! events with receipts, availability facts (State-assigned sequences) and
//! the claim settlement in ONE transaction. The publication identity
//! (`project`, `publication_id`) is bound to the package's content digest:
//! an exact retry returns the recorded outcome (`replayed = true`, same event
//! receipts and availability sequences) and writes nothing; the same identity
//! with different content refuses; a record or event whose own identity
//! already exists outside a recorded publication refuses and the transaction
//! rolls back. Immutability of records, receipts and facts is unchanged, so a
//! stored `REJECT` can never be replaced by a later package.
//!
//! Referenced-content validation stays with the trusted core: State, as for
//! every event today, checks shape and identity, not that the bytes behind a
//! payload reference match its digest. The Core adapter must resolve and hash
//! each event payload before it builds the package.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::availability_facts::{
    AvailabilityFactReceiptV1, AvailabilityFactV1, check_follows, insert_fact, lifecycle,
};
use crate::decision_records::{DecisionRecordV1, find_record, insert_record};
use crate::error::StateError;
use crate::error::{
    GraphFailureCode as GCode, GraphPhase as Phase, PublicationFailureCode as Code,
};
use crate::event_proposal::{
    EventPersistReceiptV1, EventProposalV1, insert_event_and_receipt, receipt_exists,
};
use crate::graph_history::{
    StateGraphMutationV1, append_mutation_in_tx, check_mutation_matches_snapshot,
};
use crate::graph_node_result::{StateGraphNodeResultV1, insert_result_in_tx};
use crate::graph_repository::{self as graph, StateGraphSnapshotV1, positive};
use crate::repository::SqliteStateRepository;
use crate::review_claim::{
    ReviewClaimRecordV1, ReviewClaimSettlementV1, ReviewClaimTicketV1, ReviewSealRecordV1,
    bounded_id, read_claim, read_seal, settle_in_tx,
};
use crate::trusted_time::{self, TrustedClockV1};

fn fail(code: Code, kind: &'static str) -> StateError {
    StateError::Publication { code, kind }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- graph mutation with its results ---------------------------------------

const GRAPH: &str = "graph_publication";

/// One node-result row to attach, bound to the graph version in which its
/// node exists.
pub struct GraphResultAttachmentV1 {
    version: String,
    reference: String,
    raw: String,
    result: StateGraphNodeResultV1,
}

impl GraphResultAttachmentV1 {
    pub fn new(
        version: &str,
        reference: &str,
        result: StateGraphNodeResultV1,
    ) -> Result<Self, StateError> {
        if !positive(version) || reference.is_empty() {
            return Err(fail(Code::InvalidShape, GRAPH));
        }
        Ok(Self {
            version: version.to_owned(),
            reference: reference.to_owned(),
            raw: result.to_json()?,
            result,
        })
    }
}

impl std::fmt::Debug for GraphResultAttachmentV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GraphResultAttachmentV1")
    }
}

/// Results + mutation evidence + successor snapshot, published together.
pub struct GraphMutationPublicationV1 {
    results: Vec<GraphResultAttachmentV1>,
    mutation: StateGraphMutationV1,
    raw_mutation: String,
    snapshot: StateGraphSnapshotV1,
}

impl std::fmt::Debug for GraphMutationPublicationV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GraphMutationPublicationV1")
    }
}

impl GraphMutationPublicationV1 {
    /// Every result must belong to the mutation's graph and no result
    /// reference may repeat; the mutation, snapshot and head must describe one
    /// successor.
    pub fn new(
        results: Vec<GraphResultAttachmentV1>,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<Self, StateError> {
        check_mutation_matches_snapshot(&mutation, &snapshot)?;
        let mut seen = std::collections::BTreeSet::new();
        for result in &results {
            if result.result.graph_id() != mutation.graph_id()
                || !seen.insert(result.reference.as_str())
            {
                return Err(graph::failure(Phase::Input, GCode::IdentityMismatch, None));
            }
        }
        let raw_mutation = mutation.to_json()?;
        Ok(Self {
            results,
            mutation,
            raw_mutation,
            snapshot,
        })
    }

    pub fn project_id(&self) -> &str {
        self.snapshot.content.project_id()
    }
    pub fn graph_id(&self) -> &str {
        self.mutation.graph_id()
    }
    pub(crate) fn snapshot(&self) -> &StateGraphSnapshotV1 {
        &self.snapshot
    }
}

fn attachment_digest(p: &GraphMutationPublicationV1) -> Result<String, StateError> {
    let mut rows: Vec<_> = p
        .results
        .iter()
        .map(|r| (&r.reference, &r.version, &r.raw))
        .collect();
    rows.sort();
    let bytes =
        serde_json_canonicalizer::to_vec(&rows).map_err(|_| fail(Code::InvalidShape, GRAPH))?;
    Ok(hex(&Sha256::digest(bytes)))
}

/// What a graph publication did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphPublicationOutcomeV1 {
    /// Results, snapshot, history and head were all written.
    Applied,
    /// An identical earlier publication is already recorded; nothing changed.
    Replayed,
}

/// `true` when the mutation is already stored with exactly this content;
/// refuses when the mutation id exists with anything different.
fn graph_replay(conn: &Connection, p: &GraphMutationPublicationV1) -> Result<bool, StateError> {
    let content = &p.snapshot.content;
    let stored: Option<(String, String)> = conn
        .query_row(
            "SELECT mutation_json,resulting_version FROM graph_mutations WHERE project_id=?1 AND graph_id=?2 AND mutation_id=?3",
            params![content.project_id(), content.graph_id(), p.mutation.mutation_id()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
    let Some((json, version)) = stored else {
        return Ok(false);
    };
    let digest: Option<String> = conn
        .query_row(
            "SELECT resulting_digest FROM graph_versions WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3",
            params![content.project_id(), content.graph_id(), version],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
    // Legacy mutations have no membership evidence; never infer it from result rows.
    let attachments: Option<String> = conn.query_row(
        "SELECT attachments_sha256 FROM graph_publication_identity WHERE project_id=?1 AND graph_id=?2 AND mutation_id=?3",
        params![p.project_id(),p.graph_id(),p.mutation.mutation_id()], |r| r.get(0)
    ).optional().map_err(|e| graph::sqlite_error(Phase::Write,e))?;
    let mut exact = attachments.as_deref() == Some(attachment_digest(p)?.as_str())
        && json == p.raw_mutation
        && version == p.snapshot.graph_version()
        && digest.as_deref() == Some(content.resulting_digest());
    for attached in &p.results {
        if !exact {
            break;
        }
        exact = crate::graph_node_result::read_result(
            conn,
            content.project_id(),
            content.graph_id(),
            &attached.reference,
        )?
        .is_some_and(|stored| {
            stored.graph_version() == attached.version
                && stored.result().node_id() == attached.result.node_id()
                && stored
                    .result()
                    .to_json()
                    .is_ok_and(|stored| stored == attached.raw)
        });
    }
    if exact {
        Ok(true)
    } else {
        Err(fail(Code::ConflictingRetry, GRAPH))
    }
}

impl SqliteStateRepository {
    pub(crate) fn publish_graph_mutation_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        p: &GraphMutationPublicationV1,
    ) -> Result<GraphPublicationOutcomeV1, StateError> {
        let sample = clock
            .sample()
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        let (sample, time) = trusted_time::validate_sample(sample)
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        graph::write_transaction(self.connection(), |conn| {
            if graph_replay(conn, p)? {
                return Ok(GraphPublicationOutcomeV1::Replayed);
            }
            for attached in &p.results {
                insert_result_in_tx(
                    conn,
                    p.project_id(),
                    &attached.version,
                    &attached.reference,
                    &attached.result,
                    &attached.raw,
                    true,
                )?;
            }
            append_mutation_in_tx(
                conn,
                &p.mutation,
                &p.snapshot,
                &p.raw_mutation,
                &sample,
                &time,
            )?;
            conn.execute(
                "INSERT INTO graph_publication_identity VALUES (?1,?2,?3,?4)",
                params![
                    p.project_id(),
                    p.graph_id(),
                    p.mutation.mutation_id(),
                    attachment_digest(p)?
                ],
            )
            .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            crate::write_fault::boundary()?;
            Ok(GraphPublicationOutcomeV1::Applied)
        })
    }
}

// ---- decision publication ----------------------------------------------------

const PUBLICATION: &str = "decision_publication";

/// One event of a package: the validated proposal and the event id the
/// trusted core minted for it.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationEventV1 {
    proposal: EventProposalV1,
    event_id: String,
}

impl PublicationEventV1 {
    /// Validates the event id (canonical ULID) against the frozen envelope.
    pub fn new(proposal: EventProposalV1, event_id: &str) -> Result<Self, StateError> {
        proposal.prepare(event_id, "1970-01-01T00:00:00.000000000Z", "")?;
        Ok(Self {
            proposal,
            event_id: event_id.to_owned(),
        })
    }

    /// The event id derived from the publication identity and a stable,
    /// caller-owned `event_key` (for example the Review package's event key),
    /// so rebuilding the package after a retry or restart can never mint a
    /// different id and therefore never a different package digest.
    pub fn stable(
        proposal: EventProposalV1,
        publication_id: &str,
        event_key: &str,
    ) -> Result<Self, StateError> {
        if !bounded_id(publication_id) || !bounded_id(event_key) {
            return Err(fail(Code::InvalidShape, PUBLICATION));
        }
        let event_id = crate::recovery::derived_event_id(&[
            "PUBLICATION_EVENT",
            proposal.project_id(),
            publication_id,
            event_key,
        ]);
        Self::new(proposal, &event_id)
    }

    pub fn event_id(&self) -> &str {
        &self.event_id
    }
}

/// The claim settlement a package carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaimBinding {
    /// `Some` when the live claimant's ticket attached the settlement; `None`
    /// when a sealed claim is being finished (after a restart).
    ticket: Option<(String, u64)>,
    claim_id: String,
    settlement: ReviewClaimSettlementV1,
}

/// One admitted decision publication package.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionPublicationV1 {
    project_id: String,
    publication_id: String,
    records: Vec<DecisionRecordV1>,
    events: Vec<PublicationEventV1>,
    availability: Vec<AvailabilityFactV1>,
    claim: Option<ClaimBinding>,
}

impl DecisionPublicationV1 {
    /// A package needs at least one item, every item must belong to
    /// `project_id`, and no record, event id or proposal may repeat.
    pub fn new(
        project_id: &str,
        publication_id: &str,
        records: Vec<DecisionRecordV1>,
        events: Vec<PublicationEventV1>,
        availability: Vec<AvailabilityFactV1>,
    ) -> Result<Self, StateError> {
        let shape = fail(Code::InvalidShape, PUBLICATION);
        if !bounded_id(project_id)
            || !bounded_id(publication_id)
            || (records.is_empty() && events.is_empty() && availability.is_empty())
            || records.iter().any(|r| r.project_id() != project_id)
            || events.iter().any(|e| e.proposal.project_id() != project_id)
            || availability.iter().any(|f| f.project_id() != project_id)
        {
            return Err(shape);
        }
        let mut record_keys = std::collections::BTreeSet::new();
        let mut event_ids = std::collections::BTreeSet::new();
        let mut proposals = std::collections::BTreeSet::new();
        if records
            .iter()
            .any(|r| !record_keys.insert((r.kind().as_str(), r.record_id())))
            || events.iter().any(|e| {
                !event_ids.insert(e.event_id.as_str()) || !proposals.insert(e.proposal.sha256())
            })
        {
            return Err(fail(Code::InvalidShape, PUBLICATION));
        }
        Ok(Self {
            project_id: project_id.to_owned(),
            publication_id: publication_id.to_owned(),
            records,
            events,
            availability,
            claim: None,
        })
    }

    /// Also settles the caller's review claim in the same transaction, so a
    /// published verdict and its completed claim can never be observed apart.
    pub fn with_claim_settlement(
        mut self,
        ticket: &ReviewClaimTicketV1,
        settlement: ReviewClaimSettlementV1,
    ) -> Result<Self, StateError> {
        if ticket.project_id != self.project_id {
            return Err(fail(Code::InvalidShape, PUBLICATION));
        }
        self.claim = Some(ClaimBinding {
            ticket: Some((ticket.store_id.clone(), ticket.instance)),
            claim_id: ticket.claim_id.clone(),
            settlement,
        });
        Ok(self)
    }

    /// Finishes a SEALED claim without a ticket (the restart path): the claim
    /// and its settlement come from the seal read back from State, so the
    /// caller cannot choose a different disposition. Publication refuses
    /// unless this package's digest equals the digest sealed for the claim.
    pub fn with_sealed_claim(mut self, seal: &ReviewSealRecordV1) -> Result<Self, StateError> {
        if seal.project_id != self.project_id {
            return Err(fail(Code::InvalidShape, PUBLICATION));
        }
        self.claim = Some(ClaimBinding {
            ticket: None,
            claim_id: seal.claim_id.clone(),
            settlement: seal.settlement.clone(),
        });
        Ok(self)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn publication_id(&self) -> &str {
        &self.publication_id
    }

    /// The claim's `(store_id, instance)` ticket binding, if any.
    pub(crate) fn claim_binding(&self) -> Option<(&str, u64)> {
        self.claim
            .as_ref()
            .and_then(|c| c.ticket.as_ref())
            .map(|(store_id, instance)| (store_id.as_str(), *instance))
    }

    /// The claim this package settles and the disposition it applies.
    pub(crate) fn claim_identity(&self) -> Option<(&str, &ReviewClaimSettlementV1)> {
        self.claim
            .as_ref()
            .map(|c| (c.claim_id.as_str(), &c.settlement))
    }

    /// Lowercase SHA-256 over the whole package: the publication identity's
    /// content binding.
    pub fn sha256(&self) -> String {
        let fields = serde_json::json!([
            "receipts.decision-publication.v1",
            self.project_id,
            self.publication_id,
            self.records
                .iter()
                .map(|r| [r.kind().as_str(), r.record_id(), r.record_sha256()])
                .collect::<Vec<_>>(),
            self.events
                .iter()
                .map(|e| [e.event_id.clone(), e.proposal.sha256()])
                .collect::<Vec<_>>(),
            self.availability
                .iter()
                .map(AvailabilityFactV1::canonical)
                .collect::<Vec<_>>(),
            self.claim
                .as_ref()
                .map(|c| serde_json::json!([c.claim_id, c.settlement.canonical()])),
        ]);
        hex(&Sha256::digest(fields.to_string().as_bytes()))
    }
}

/// The recorded outcome of a publication (the same for the first call and any
/// exact retry, apart from `replayed`).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionPublicationReceiptV1 {
    pub publication_id: String,
    pub replayed: bool,
    pub recorded_at: String,
    /// In package order.
    pub events: Vec<EventPersistReceiptV1>,
    /// In package order, with the State-assigned sequences.
    pub availability: Vec<AvailabilityFactReceiptV1>,
    /// The settled claim, when the package carried a settlement.
    pub claim: Option<ReviewClaimRecordV1>,
}

fn corrupt() -> StateError {
    fail(Code::CorruptStore, PUBLICATION)
}

/// Rebuilds the recorded outcome of an already-recorded publication from its
/// authoritative rows; any missing or drifting row is corruption.
fn recorded_receipt(
    conn: &Connection,
    p: &DecisionPublicationV1,
    recorded_at: String,
) -> Result<DecisionPublicationReceiptV1, StateError> {
    for record in &p.records {
        let stored = find_record(conn, &p.project_id, record.kind(), record.record_id())?;
        if stored.as_ref().map(DecisionRecordV1::record_sha256) != Some(record.record_sha256()) {
            return Err(corrupt());
        }
    }
    let mut events = Vec::new();
    for event in &p.events {
        let row: Option<(String, String, String, String, String)> = conn
            .query_row(
                "SELECT proposal_sha256,origin,store_id,occurred_at,project_id FROM event_proposal_receipt WHERE event_id=?1",
                [&event.event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
        let Some((sha, origin, store_id, occurred_at, project)) = row else {
            return Err(corrupt());
        };
        if sha != event.proposal.sha256()
            || project != p.project_id
            || origin != event.proposal.origin().as_str()
        {
            return Err(corrupt());
        }
        events.push(EventPersistReceiptV1 {
            event_id: event.event_id.clone(),
            proposal_sha256: sha,
            origin: event.proposal.origin(),
            store_id,
            occurred_at,
        });
    }
    let mut availability = Vec::new();
    for (ordinal, fact) in p.availability.iter().enumerate() {
        let row: Option<(i64, String)> = conn
            .query_row(
                "SELECT a.sequence,f.recorded_at FROM atomic_publication_availability a JOIN availability_fact f ON f.project_id=a.project_id AND f.provider_id=a.provider_id AND f.runtime_id=a.runtime_id AND f.model_id=a.model_id AND f.sequence=a.sequence WHERE a.project_id=?1 AND a.publication_id=?2 AND a.ordinal=?3 AND a.provider_id=?4 AND a.runtime_id=?5 AND a.model_id=?6",
                params![
                    p.project_id,
                    p.publication_id,
                    ordinal as i64,
                    fact.provider_id(),
                    fact.runtime_id(),
                    fact.model_id()
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
        let Some((sequence, fact_recorded_at)) = row else {
            return Err(corrupt());
        };
        let status = lifecycle(
            conn,
            &p.project_id,
            fact.provider_id(),
            fact.runtime_id(),
            fact.model_id(),
        )?
        .status;
        availability.push(AvailabilityFactReceiptV1 {
            sequence: u64::try_from(sequence).map_err(|_| corrupt())?,
            recorded_at: fact_recorded_at,
            status,
        });
    }
    let claim = match &p.claim {
        None => None,
        Some(binding) => {
            let record = read_claim(conn, &p.project_id, &binding.claim_id)?.ok_or_else(corrupt)?;
            // A recorded publication settled its claim in the same transaction.
            if binding.settlement.canonical() != settled_form(&record) {
                return Err(corrupt());
            }
            Some(record)
        }
    };
    Ok(DecisionPublicationReceiptV1 {
        publication_id: p.publication_id.clone(),
        replayed: true,
        recorded_at,
        events,
        availability,
        claim,
    })
}

/// The claim's recorded disposition in [`ReviewClaimSettlementV1::canonical`] form.
fn settled_form(record: &ReviewClaimRecordV1) -> serde_json::Value {
    serde_json::json!([
        record.state.as_str(),
        record.outcome_kind,
        record.outcome_ref,
        record.outcome_sha256,
    ])
}

impl SqliteStateRepository {
    pub(crate) fn publish_decision_package_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        p: &DecisionPublicationV1,
        store_id: &str,
    ) -> Result<DecisionPublicationReceiptV1, StateError> {
        let sample = clock
            .sample()
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        let (sample, time) = trusted_time::validate_sample(sample)
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        let at = sample.canonical_utc_timestamp.clone();
        let digest = p.sha256();
        // Events are prepared (validated) before any write.
        let mut prepared = Vec::new();
        for event in &p.events {
            prepared.push(event.proposal.prepare(&event.event_id, &at, store_id)?);
        }
        graph::write_transaction(self.connection(), |conn| {
            let recorded: Option<(String, String)> = conn
                .query_row(
                    "SELECT package_sha256,recorded_at FROM atomic_publication WHERE project_id=?1 AND publication_id=?2",
                    params![p.project_id, p.publication_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            if let Some((stored, recorded_at)) = recorded {
                if stored != digest {
                    return Err(fail(Code::ConflictingRetry, PUBLICATION));
                }
                return recorded_receipt(conn, p, recorded_at);
            }
            // A claim that was sealed is finished only by its sealed package;
            // a restart publication needs the seal it names.
            if let Some(binding) = &p.claim {
                match read_seal(conn, &p.project_id, &binding.claim_id)? {
                    Some(seal) if seal.package_sha256 != digest => {
                        return Err(fail(Code::SealMismatch, PUBLICATION));
                    }
                    None if binding.ticket.is_none() => {
                        return Err(fail(Code::SealNotFound, PUBLICATION));
                    }
                    _ => {}
                }
            }
            for event in &p.events {
                if receipt_exists(conn, &p.project_id, &event.proposal.sha256())? {
                    return Err(graph::failure(Phase::Write, GCode::AlreadyExists, None));
                }
            }
            graph::accept_graph_sample(conn, &p.project_id, &sample, &time)?;
            crate::write_fault::boundary()?;
            conn.execute(
                "INSERT INTO atomic_publication(project_id,publication_id,package_sha256,recorded_at) VALUES (?1,?2,?3,?4)",
                params![p.project_id, p.publication_id, digest, at],
            )
            .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            crate::write_fault::boundary()?;
            for record in &p.records {
                insert_record(conn, record)?;
                crate::write_fault::boundary()?;
            }
            let mut events = Vec::new();
            for (event, (envelope, receipt)) in p.events.iter().zip(&prepared) {
                insert_event_and_receipt(conn, &event.proposal, envelope, receipt)?;
                crate::write_fault::boundary()?;
                events.push(receipt.clone());
            }
            let mut availability = Vec::new();
            for (ordinal, fact) in p.availability.iter().enumerate() {
                let current = check_follows(conn, fact)?;
                let receipt = insert_fact(conn, fact, current, &at)?;
                conn.execute(
                    "INSERT INTO atomic_publication_availability(project_id,publication_id,ordinal,provider_id,runtime_id,model_id,sequence) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        p.project_id,
                        p.publication_id,
                        ordinal as i64,
                        fact.provider_id(),
                        fact.runtime_id(),
                        fact.model_id(),
                        receipt.sequence as i64
                    ],
                )
                .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
                crate::write_fault::boundary()?;
                availability.push(receipt);
            }
            let claim = match &p.claim {
                None => None,
                Some(binding) => Some(settle_in_tx(
                    conn,
                    &p.project_id,
                    &binding.claim_id,
                    &binding.settlement,
                    &at,
                )?),
            };
            Ok(DecisionPublicationReceiptV1 {
                publication_id: p.publication_id.clone(),
                replayed: false,
                recorded_at: at.clone(),
                events,
                availability,
                claim,
            })
        })
    }
}
