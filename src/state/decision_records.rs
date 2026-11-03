//! Immutable, project-isolated decision/registry/observation/review/
//! integration records and link-by-link provenance (migration 17).
//!
//! State stores what other owners define; it never decides meaning. Each
//! record keeps the exact supplied schema-shaped JSON bytes and their
//! SHA-256, plus typed identity metadata extracted from them. Admission
//! validates the full owner-frozen schema shape (`decision_schema`): unique
//! keys at every depth, and at every nested level the required properties,
//! no unknown property, types, enums, identifier lengths, SHA patterns,
//! item shapes and exact-integer rules. Truly open fields stay open. Numbers
//! are never converted through `f64`. Stored records are re-admitted on every
//! read. External payloads stay reference+digest; storing a record is not
//! acceptance.

use std::collections::HashSet;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use crate::decision_schema::{self as schema, Obj};
use crate::error::{RecordFailureCode as Code, StateError};
use crate::graph_repository::{hex, id};
use crate::repository::SqliteStateRepository;

/// The closed set of persisted decision/provenance record families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecisionRecordKind {
    /// `RoutingDecision` (MR-owned shape); id `decision_id`.
    RoutingDecision,
    /// `ModelCapability` registry row incl. lifecycle; caller storage id.
    RegistryCapability,
    /// `ModelObservation`; id `observation_id`.
    ModelObservation,
    /// `AvailabilityState` observation; caller storage id.
    AvailabilityObservation,
    /// `ReviewRequest`; id `request_id`.
    ReviewRequest,
    /// `A4Review` incl. its findings; id `review_id`.
    A4Review,
    /// `IntegrationRequest`; id `request_id`.
    IntegrationRequest,
    /// `IntegrationDecision`; id `decision_id`.
    IntegrationDecision,
}

impl DecisionRecordKind {
    pub const ALL: [Self; 8] = [
        Self::RoutingDecision,
        Self::RegistryCapability,
        Self::ModelObservation,
        Self::AvailabilityObservation,
        Self::ReviewRequest,
        Self::A4Review,
        Self::IntegrationRequest,
        Self::IntegrationDecision,
    ];
    pub fn as_str(self) -> &'static str {
        spec(self).storage
    }
    fn from_storage(value: &str) -> Result<Self, StateError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| failure(Code::CorruptStore, RECORD))
    }
}

struct Spec {
    storage: &'static str,
    root: &'static Obj,
    id_key: Option<&'static str>,
    task_key: Option<&'static str>,
    link_key: Option<&'static str>,
    sha_key: Option<&'static str>,
}

fn spec(kind: DecisionRecordKind) -> &'static Spec {
    match kind {
        DecisionRecordKind::RoutingDecision => &Spec {
            storage: "ROUTING_DECISION",
            root: &schema::ROUTING_DECISION,
            id_key: Some("decision_id"),
            task_key: Some("task_id"),
            link_key: Some("request_id"),
            sha_key: None,
        },
        DecisionRecordKind::RegistryCapability => &Spec {
            storage: "REGISTRY_CAPABILITY",
            root: &schema::MODEL_CAPABILITY,
            id_key: None,
            task_key: None,
            link_key: None,
            sha_key: None,
        },
        DecisionRecordKind::ModelObservation => &Spec {
            storage: "MODEL_OBSERVATION",
            root: &schema::MODEL_OBSERVATION,
            id_key: Some("observation_id"),
            task_key: Some("task_id"),
            link_key: None,
            sha_key: None,
        },
        DecisionRecordKind::AvailabilityObservation => &Spec {
            storage: "AVAILABILITY_OBSERVATION",
            root: &schema::AVAILABILITY_STATE,
            id_key: None,
            task_key: None,
            link_key: None,
            sha_key: None,
        },
        DecisionRecordKind::ReviewRequest => &Spec {
            storage: "REVIEW_REQUEST",
            root: &schema::REVIEW_REQUEST,
            id_key: Some("request_id"),
            task_key: Some("task_id"),
            link_key: None,
            sha_key: Some("implementation_sha"),
        },
        DecisionRecordKind::A4Review => &Spec {
            storage: "A4_REVIEW",
            root: &schema::A4_REVIEW,
            id_key: Some("review_id"),
            task_key: Some("task_id"),
            link_key: None,
            sha_key: Some("reviewed_sha"),
        },
        DecisionRecordKind::IntegrationRequest => &Spec {
            storage: "INTEGRATION_REQUEST",
            root: &schema::INTEGRATION_REQUEST,
            id_key: Some("request_id"),
            task_key: None,
            link_key: None,
            sha_key: Some("source_sha"),
        },
        DecisionRecordKind::IntegrationDecision => &Spec {
            storage: "INTEGRATION_DECISION",
            root: &schema::INTEGRATION_DECISION,
            id_key: Some("decision_id"),
            task_key: None,
            link_key: Some("request_id"),
            sha_key: None,
        },
    }
}

const RECORD: &str = "decision_record";
const LINK: &str = "provenance_link";

fn failure(code: Code, kind: &'static str) -> StateError {
    StateError::DecisionRecord { code, kind }
}
fn invalid() -> StateError {
    failure(Code::InvalidShape, RECORD)
}

/// One validated, immutable decision record plus its typed metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct DecisionRecordV1 {
    project_id: String,
    kind: DecisionRecordKind,
    record_id: String,
    task_id: Option<String>,
    link_id: Option<String>,
    subject_sha: Option<String>,
    record_json: String,
    record_sha256: String,
}

impl fmt::Debug for DecisionRecordV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecisionRecordV1")
            .field("kind", &self.kind)
            .field("record_sha256", &self.record_sha256)
            .finish_non_exhaustive()
    }
}

impl DecisionRecordV1 {
    /// Validates `record_json` against `kind`'s complete frozen schema shape.
    ///
    /// `storage_id` is the record identity: for kinds with an intrinsic id
    /// it must equal that id; for registry/availability rows (whose schemas
    /// have no id) it is the caller's opaque storage key.
    pub fn parse(
        project_id: &str,
        kind: DecisionRecordKind,
        storage_id: &str,
        record_json: &str,
    ) -> Result<Self, StateError> {
        if !id(project_id) || !id(storage_id) {
            return Err(invalid());
        }
        let spec = spec(kind);
        let entries = top_level(record_json)?;
        let root = serde_json::from_str::<Box<RawValue>>(record_json).map_err(|_| invalid())?;
        if !schema::admits_object(spec.root, &root) {
            return Err(invalid());
        }
        let text = |key: Option<&str>| -> Option<String> {
            let key = key?;
            let (_, value) = entries.iter().find(|(name, _)| name == key)?;
            serde_json::from_str::<String>(value.get()).ok()
        };
        if let Some(intrinsic) = text(spec.id_key)
            && intrinsic != storage_id
        {
            return Err(failure(Code::IdentityMismatch, RECORD));
        }
        Ok(Self {
            project_id: project_id.to_string(),
            kind,
            record_id: storage_id.to_string(),
            task_id: text(spec.task_key),
            link_id: text(spec.link_key),
            subject_sha: text(spec.sha_key),
            record_sha256: sha256_hex(record_json.as_bytes()),
            record_json: record_json.to_string(),
        })
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn kind(&self) -> DecisionRecordKind {
        self.kind
    }
    pub fn record_id(&self) -> &str {
        &self.record_id
    }
    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    /// The linked request id (routing/integration decisions).
    pub fn link_id(&self) -> Option<&str> {
        self.link_id.as_deref()
    }
    /// The bound SHA (implementation/reviewed/source SHA).
    pub fn subject_sha(&self) -> Option<&str> {
        self.subject_sha.as_deref()
    }
    /// The exact supplied JSON bytes.
    pub fn record_json(&self) -> &str {
        &self.record_json
    }
    /// Lowercase SHA-256 hex of [`Self::record_json`].
    pub fn record_sha256(&self) -> &str {
        &self.record_sha256
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Parses one JSON object into ordered top-level entries, refusing duplicate
/// decoded keys at any depth and trailing data. Numbers stay raw tokens.
fn top_level(text: &str) -> Result<Vec<(String, Box<RawValue>)>, StateError> {
    struct Entries;
    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<(String, Box<RawValue>)>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries: Vec<(String, Box<RawValue>)> = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                if entries.iter().any(|(seen, _)| *seen == key) {
                    return Err(de::Error::custom("duplicate key"));
                }
                let value: Box<RawValue> = map.next_value()?;
                unique_keys(&value).map_err(|_| de::Error::custom("duplicate key"))?;
                entries.push((key, value));
            }
            Ok(entries)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let entries = deserializer
        .deserialize_map(Entries)
        .map_err(|_| invalid())?;
    deserializer.end().map_err(|_| invalid())?;
    Ok(entries)
}

/// Refuses duplicate decoded object keys anywhere inside `raw`.
pub(crate) fn unique_keys(raw: &RawValue) -> Result<(), serde_json::Error> {
    struct Children;
    impl<'de> Visitor<'de> for Children {
        type Value = Vec<&'de RawValue>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("JSON container")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut seen = HashSet::new();
            let mut children = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                if !seen.insert(key) {
                    return Err(de::Error::custom("duplicate key"));
                }
                children.push(map.next_value::<&RawValue>()?);
            }
            Ok(children)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut children = Vec::new();
            while let Some(child) = seq.next_element::<&RawValue>()? {
                children.push(child);
            }
            Ok(children)
        }
    }
    let mut pending = vec![raw];
    while let Some(value) = pending.pop() {
        let children = match value.get().as_bytes().first() {
            Some(b'{') => value.deserialize_map(Children)?,
            Some(b'[') => value.deserialize_seq(Children)?,
            _ => continue,
        };
        pending.extend(children);
    }
    Ok(())
}

/// The closed provenance link kinds, in chain order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceLinkKind {
    StartSha,
    ImplementationSha,
    ReviewSha,
    /// Id of the persisted `A4_REVIEW` record for this task.
    VerdictRef,
    /// Id of the persisted `INTEGRATION_DECISION` record accepting this task.
    AcceptanceRef,
    WorkstreamSha,
    MainSha,
}

impl ProvenanceLinkKind {
    pub const ALL: [Self; 7] = [
        Self::StartSha,
        Self::ImplementationSha,
        Self::ReviewSha,
        Self::VerdictRef,
        Self::AcceptanceRef,
        Self::WorkstreamSha,
        Self::MainSha,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StartSha => "START_SHA",
            Self::ImplementationSha => "IMPLEMENTATION_SHA",
            Self::ReviewSha => "REVIEW_SHA",
            Self::VerdictRef => "VERDICT_REF",
            Self::AcceptanceRef => "ACCEPTANCE_REF",
            Self::WorkstreamSha => "WORKSTREAM_SHA",
            Self::MainSha => "MAIN_SHA",
        }
    }
    fn is_sha(self) -> bool {
        !matches!(self, Self::VerdictRef | Self::AcceptanceRef)
    }
    fn from_storage(value: &str) -> Result<Self, StateError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| failure(Code::CorruptStore, LINK))
    }
}

/// A link-level inconsistency found while reconstructing a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceContradictionV1 {
    /// REVIEW_SHA differs from IMPLEMENTATION_SHA.
    ReviewShaNotImplementationSha,
    /// VERDICT_REF names no persisted A4_REVIEW in this project.
    DanglingVerdictRef,
    /// The referenced review is for another task.
    VerdictTaskMismatch,
    /// The referenced review's `reviewed_sha` differs from REVIEW_SHA.
    VerdictShaMismatch,
    /// ACCEPTANCE_REF names no persisted INTEGRATION_DECISION in this project.
    DanglingAcceptanceRef,
    /// The referenced decision's outcome is not `ACCEPT`.
    AcceptanceNotAccepted,
    /// An `ACCEPT` decision is the acceptance, but the same task's review at
    /// the same SHA (the VERDICT_REF) is a `REJECT`.
    AcceptanceWithRejectedVerdict,
    /// The referenced decision has no provenance entry for this task.
    AcceptanceTaskEntryAbsent,
    /// The referenced decision has more than one provenance entry for this task.
    AcceptanceTaskEntryAmbiguous,
    /// The decision's one provenance entry for this task disagrees with a link.
    AcceptanceChainMismatch,
    /// The decision's non-null `integration_sha` differs from MAIN_SHA.
    MainShaMismatch,
}

/// The reconstructed chain for one task. Complete storage is evidence for a
/// gate to evaluate; it is not itself acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceChainV1 {
    pub task_id: String,
    /// Recorded links in chain order.
    pub links: Vec<(ProvenanceLinkKind, String)>,
    /// Link kinds with no recorded row, in chain order.
    pub missing: Vec<ProvenanceLinkKind>,
    pub contradictions: Vec<ProvenanceContradictionV1>,
}

impl ProvenanceChainV1 {
    pub fn link(&self, kind: ProvenanceLinkKind) -> Option<&str> {
        self.links
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.as_str())
    }
    /// Every link recorded and no contradiction found.
    pub fn is_complete_and_consistent(&self) -> bool {
        self.missing.is_empty() && self.contradictions.is_empty()
    }
}

impl SqliteStateRepository {
    /// Appends one immutable decision record; a duplicate identity is refused.
    pub fn append_decision_record(&mut self, record: &DecisionRecordV1) -> Result<(), StateError> {
        self.run_transaction(|uow| {
            let conn = uow.tx();
            let exists = conn
                .query_row(
                    "SELECT 1 FROM decision_records WHERE project_id=?1 AND record_kind=?2 AND record_id=?3",
                    params![record.project_id, record.kind.as_str(), record.record_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| sql_failure(e, RECORD))?;
            if exists.is_some() {
                return Err(failure(Code::AlreadyExists, RECORD));
            }
            conn.execute(
                "INSERT INTO decision_records(project_id,record_kind,record_id,task_id,link_id,subject_sha,record_json,record_sha256) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    record.project_id,
                    record.kind.as_str(),
                    record.record_id,
                    record.task_id,
                    record.link_id,
                    record.subject_sha,
                    record.record_json,
                    record.record_sha256
                ],
            )
            .map_err(|e| sql_failure(e, RECORD))?;
            Ok(())
        })
    }

    /// Reads one record by exact project, kind and id.
    pub fn find_decision_record(
        &self,
        project_id: &str,
        kind: DecisionRecordKind,
        record_id: &str,
    ) -> Result<Option<DecisionRecordV1>, StateError> {
        if !id(project_id) || !id(record_id) {
            return Err(invalid());
        }
        read_record(self.connection(), project_id, kind, record_id)
    }

    /// Lists one project's records of `kind`, optionally only those bound to
    /// `task_id`, in binary record-id order from one snapshot.
    pub fn list_decision_records(
        &self,
        project_id: &str,
        kind: DecisionRecordKind,
        task_id: Option<&str>,
    ) -> Result<Vec<DecisionRecordV1>, StateError> {
        if !id(project_id) || task_id.is_some_and(|t| !id(t)) {
            return Err(invalid());
        }
        let tx = self
            .connection()
            .unchecked_transaction()
            .map_err(|e| sql_failure(e, RECORD))?;
        let records = {
            let mut statement = tx
                .prepare(&format!(
                    "{SELECT_RECORD} WHERE project_id=?1 AND record_kind=?2 ORDER BY record_id COLLATE BINARY"
                ))
                .map_err(|e| sql_failure(e, RECORD))?;
            statement
                .query_map(params![project_id, kind.as_str()], raw_record)
                .map_err(|e| sql_failure(e, RECORD))?
                .map(|row| {
                    row.map_err(|e| sql_failure(e, RECORD))
                        .and_then(decode_record)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        tx.commit().map_err(|e| sql_failure(e, RECORD))?;
        Ok(records
            .into_iter()
            .filter(|record| task_id.is_none_or(|task| record.task_id() == Some(task)))
            .collect())
    }

    /// Appends one immutable provenance link. SHA kinds require strict
    /// lowercase 40-hex; ref kinds an identifier. Any second value for the
    /// same task and kind — identical or contradictory — is refused.
    pub fn append_provenance_link(
        &mut self,
        project_id: &str,
        task_id: &str,
        kind: ProvenanceLinkKind,
        value: &str,
    ) -> Result<(), StateError> {
        let well_formed = if kind.is_sha() {
            hex(value, 40)
        } else {
            id(value)
        };
        if !id(project_id) || !id(task_id) || !well_formed {
            return Err(failure(Code::InvalidShape, LINK));
        }
        self.run_transaction(|uow| {
            let conn = uow.tx();
            let exists = conn
                .query_row(
                    "SELECT 1 FROM provenance_links WHERE project_id=?1 AND task_id=?2 AND link_kind=?3",
                    params![project_id, task_id, kind.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| sql_failure(e, LINK))?;
            if exists.is_some() {
                return Err(failure(Code::AlreadyExists, LINK));
            }
            conn.execute(
                "INSERT INTO provenance_links(project_id,task_id,link_kind,link_value) VALUES (?1,?2,?3,?4)",
                params![project_id, task_id, kind.as_str(), value],
            )
            .map_err(|e| sql_failure(e, LINK))?;
            Ok(())
        })
    }

    /// Reconstructs one task's chain from a single read snapshot, naming
    /// every missing link and every contradiction with stored records.
    pub fn provenance_chain(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<ProvenanceChainV1, StateError> {
        if !id(project_id) || !id(task_id) {
            return Err(failure(Code::InvalidShape, LINK));
        }
        let tx = self
            .connection()
            .unchecked_transaction()
            .map_err(|e| sql_failure(e, LINK))?;
        let chain = build_chain(&tx, project_id, task_id)?;
        tx.commit().map_err(|e| sql_failure(e, LINK))?;
        Ok(chain)
    }
}

fn build_chain(
    conn: &Connection,
    project_id: &str,
    task_id: &str,
) -> Result<ProvenanceChainV1, StateError> {
    let mut stored = Vec::new();
    {
        let mut statement = conn
            .prepare("SELECT link_kind, link_value FROM provenance_links WHERE project_id=?1 AND task_id=?2")
            .map_err(|e| sql_failure(e, LINK))?;
        let rows = statement
            .query_map(params![project_id, task_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| sql_failure(e, LINK))?;
        for row in rows {
            let (kind, value) = row.map_err(|e| sql_failure(e, LINK))?;
            stored.push((ProvenanceLinkKind::from_storage(&kind)?, value));
        }
    }
    let mut chain = ProvenanceChainV1 {
        task_id: task_id.to_string(),
        links: Vec::new(),
        missing: Vec::new(),
        contradictions: Vec::new(),
    };
    for kind in ProvenanceLinkKind::ALL {
        match stored.iter().find(|(k, _)| *k == kind) {
            Some((_, value)) => chain.links.push((kind, value.clone())),
            None => chain.missing.push(kind),
        }
    }
    use ProvenanceContradictionV1 as C;
    use ProvenanceLinkKind as K;
    let implementation = chain.link(K::ImplementationSha).map(str::to_string);
    let review = chain.link(K::ReviewSha).map(str::to_string);
    if let (Some(i), Some(r)) = (&implementation, &review)
        && i != r
    {
        chain.contradictions.push(C::ReviewShaNotImplementationSha);
    }
    let mut rejected_verdict = false;
    if let Some(verdict) = chain.link(K::VerdictRef).map(str::to_string) {
        match read_record(conn, project_id, DecisionRecordKind::A4Review, &verdict)? {
            None => chain.contradictions.push(C::DanglingVerdictRef),
            Some(record) => {
                let same_task = record.task_id() == Some(task_id);
                if !same_task {
                    chain.contradictions.push(C::VerdictTaskMismatch);
                }
                let same_sha = review.is_none() || record.subject_sha() == review.as_deref();
                if !same_sha {
                    chain.contradictions.push(C::VerdictShaMismatch);
                }
                let view: VerdictView = serde_json::from_str(record.record_json())
                    .map_err(|_| failure(Code::CorruptStore, RECORD))?;
                rejected_verdict = same_task && same_sha && view.verdict == "REJECT";
            }
        }
    }
    if let Some(acceptance) = chain.link(K::AcceptanceRef).map(str::to_string) {
        match read_record(
            conn,
            project_id,
            DecisionRecordKind::IntegrationDecision,
            &acceptance,
        )? {
            None => chain.contradictions.push(C::DanglingAcceptanceRef),
            Some(record) => {
                let decision: AcceptanceView = serde_json::from_str(record.record_json())
                    .map_err(|_| failure(Code::CorruptStore, RECORD))?;
                if decision.outcome != "ACCEPT" {
                    chain.contradictions.push(C::AcceptanceNotAccepted);
                } else if rejected_verdict {
                    chain.contradictions.push(C::AcceptanceWithRejectedVerdict);
                }
                let entries: Vec<&AcceptanceEntry> = decision
                    .provenance_chain
                    .iter()
                    .filter(|entry| entry.task_id == task_id)
                    .collect();
                match entries.as_slice() {
                    [] => chain.contradictions.push(C::AcceptanceTaskEntryAbsent),
                    [entry] => {
                        let disagrees = [
                            (K::StartSha, &entry.start_sha),
                            (K::ImplementationSha, &entry.implementation_sha),
                            (K::ReviewSha, &entry.review_sha),
                        ]
                        .iter()
                        .any(|(kind, value)| {
                            chain.link(*kind).is_some_and(|link| link != value.as_str())
                        });
                        if disagrees {
                            chain.contradictions.push(C::AcceptanceChainMismatch);
                        }
                    }
                    _ => chain.contradictions.push(C::AcceptanceTaskEntryAmbiguous),
                }
                if let (Some(main), Some(integrated)) =
                    (chain.link(K::MainSha), &decision.integration_sha)
                    && main != integrated
                {
                    chain.contradictions.push(C::MainShaMismatch);
                }
            }
        }
    }
    Ok(chain)
}

#[derive(serde::Deserialize)]
struct VerdictView {
    verdict: String,
}
#[derive(serde::Deserialize)]
struct AcceptanceView {
    outcome: String,
    #[serde(default)]
    provenance_chain: Vec<AcceptanceEntry>,
    #[serde(default)]
    integration_sha: Option<String>,
}
#[derive(serde::Deserialize)]
struct AcceptanceEntry {
    task_id: String,
    start_sha: String,
    implementation_sha: String,
    review_sha: String,
}

const SELECT_RECORD: &str = "SELECT project_id,record_kind,record_id,record_json,record_sha256,task_id,link_id,subject_sha FROM decision_records";

fn read_record(
    conn: &Connection,
    project_id: &str,
    kind: DecisionRecordKind,
    record_id: &str,
) -> Result<Option<DecisionRecordV1>, StateError> {
    conn.query_row(
        &format!("{SELECT_RECORD} WHERE project_id=?1 AND record_kind=?2 AND record_id=?3"),
        params![project_id, kind.as_str(), record_id],
        raw_record,
    )
    .optional()
    .map_err(|e| sql_failure(e, RECORD))?
    .map(decode_record)
    .transpose()
}

type RawRecord = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
fn raw_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRecord> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}

/// Re-admits stored bytes and re-derives metadata; any drift is corruption.
fn decode_record(
    (project_id, kind, record_id, json, digest, task, link, sha): RawRecord,
) -> Result<DecisionRecordV1, StateError> {
    let kind = DecisionRecordKind::from_storage(&kind)?;
    let record = DecisionRecordV1::parse(&project_id, kind, &record_id, &json)
        .map_err(|_| failure(Code::CorruptStore, RECORD))?;
    if record.record_sha256 != digest
        || record.task_id != task
        || record.link_id != link
        || record.subject_sha != sha
    {
        return Err(failure(Code::CorruptStore, RECORD));
    }
    Ok(record)
}

fn sql_failure(error: rusqlite::Error, kind: &'static str) -> StateError {
    let code = match &error {
        rusqlite::Error::SqliteFailure(cause, _) => match cause.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                Code::StoreBusy
            }
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                Code::CorruptStore
            }
            _ => Code::StorageFailure,
        },
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::Utf8Error(..) => Code::CorruptStore,
        _ => Code::StorageFailure,
    };
    failure(code, kind)
}
