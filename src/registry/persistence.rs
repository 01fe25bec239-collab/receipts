//! Append-only registry persistence in State (MR-W2-3) and runtime-probe
//! consumption.
//!
//! Every successful registry change is one immutable State `ModelCapability`
//! record (`REGISTRY_CAPABILITY`, storage id `registry-<20-digit sequence>`,
//! so State's binary id order is append order) plus one Routing event
//! proposal; a probe report publishes all its entries together. Capability evidence is stored as itself. Structure and lifecycle
//! changes use the reserved `receipts.registry.*` capability names:
//!
//! | change | subject_kind / ids | capability | value / lifecycle_state |
//! |---|---|---|---|
//! | provider | MODEL, provider only | `receipts.registry.provider` | `true` |
//! | model discovered | MODEL | `receipts.registry.model` | `true`, DISCOVERED |
//! | runtime | RUNTIME | `receipts.registry.runtime` | `true` |
//! | association | MODEL_RUNTIME_PAIR | `receipts.registry.association` | `true` |
//! | lifecycle transition | MODEL | `receipts.registry.lifecycle` | evidence object, target state |
//!
//! Reload replays the records through [`ModelIntelligenceService`], so every
//! invariant is re-checked and the reloaded registry equals the one written.
//! Each change is validated on a copy first and published (record + event)
//! as one atomic [`Publication`]; memory advances only after the port reports
//! success. A transaction failure leaves no partial state; a lost
//! acknowledgement can leave memory behind the committed history. Repeating
//! the identical call after a lost acknowledgement
//! returns the original receipt.

use receipts_runtime_adapters::{
    CapabilityStatus, ClaudeCapabilityEvidence, CodexCapabilityEvidence, RuntimeCapabilities,
};
use receipts_state::{
    DecisionRecordKind, DecisionRecordV1, EventPersistReceiptV1, EventSubject, EventType,
    SqliteStateRepository, SubjectKind as StateSubjectKind,
};
use serde_json::{Value, json};

use super::*;
use crate::intelligence::DemotionReason;
use crate::persistence::{
    EventScope, Publication, RoutingPersistError, RoutingStatePort, publish, record_event,
};
use crate::wire::{decode_model_capability, encode_model_capability};

pub const RESERVED_CAPABILITY_PREFIX: &str = "receipts.registry.";
const PROVIDER: &str = "receipts.registry.provider";
const MODEL: &str = "receipts.registry.model";
const RUNTIME: &str = "receipts.registry.runtime";
const ASSOCIATION: &str = "receipts.registry.association";
const LIFECYCLE: &str = "receipts.registry.lifecycle";

#[derive(Debug)]
pub enum RegistryPersistError<E> {
    Registry(RegistryError),
    /// Capability names under `receipts.registry.` are reserved.
    ReservedCapability,
    /// A probe report names a different runtime.
    ProbeRuntimeMismatch,
    /// A stored record could not be replayed (index in append order).
    CorruptHistory(usize),
    /// A validated change did not replay from its own record (encoding bug).
    ReplayMismatch,
    Persist(RoutingPersistError<E>),
}

impl<E> From<RegistryError> for RegistryPersistError<E> {
    fn from(e: RegistryError) -> Self {
        Self::Registry(e)
    }
}
impl<E> From<RoutingPersistError<E>> for RegistryPersistError<E> {
    fn from(e: RoutingPersistError<E>) -> Self {
        Self::Persist(e)
    }
}

/// The Model Intelligence owner's registry, durable in State.
#[derive(Debug)]
pub struct PersistentRegistry {
    service: ModelIntelligenceService,
    project_id: String,
    next_sequence: u64,
}

#[allow(clippy::too_many_arguments)] // One schema row's fields.
fn structural(
    kind: SubjectKind,
    provider: &ProviderId,
    model: Option<&ModelId>,
    runtime: Option<&RuntimeId>,
    capability: &str,
    value: Value,
    observation: &Observation,
    lifecycle: Option<LifecycleState>,
) -> ModelCapabilityRecord {
    ModelCapabilityRecord::try_new(
        kind,
        Some(provider.as_str().to_owned()),
        model.map(|m| m.as_str().to_owned()),
        runtime.map(|r| r.as_str().to_owned()),
        capability.to_owned(),
        Some(value),
        observation.confidence,
        observation.source_ref.clone(),
        observation.observed_at.clone(),
        None,
        lifecycle,
    )
    .expect("registry identifiers are non-empty")
}

fn evidence_value(evidence: &LifecycleEvidence) -> Value {
    use LifecycleEvidence::*;
    match evidence {
        ReadyForAssessment => json!({"evidence": "READY_FOR_ASSESSMENT"}),
        CapabilityVerification(path) => json!({
            "evidence": "CAPABILITY_VERIFICATION",
            "runtime_id": path.runtime_id.as_str(),
            "required_capabilities": path
                .required_capabilities
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>(),
        }),
        BoundedCalibrationAdmission { authorized } => {
            json!({"evidence": "BOUNDED_CALIBRATION_ADMISSION", "authorized": authorized})
        }
        RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
            sufficient_acceptable_observations,
        }) => json!({
            "evidence": "ROUTABLE_PROMOTION_LOCAL_CALIBRATION",
            "sufficient_acceptable_observations": sufficient_acceptable_observations,
        }),
        RoutablePromotion(RoutablePromotionEvidence::OfficialAndIndependent {
            ask_on_uncertainty_user_consent,
        }) => json!({
            "evidence": "ROUTABLE_PROMOTION_OFFICIAL_AND_INDEPENDENT",
            "ask_on_uncertainty_user_consent": ask_on_uncertainty_user_consent,
        }),
        Demotion(reason) => json!({"evidence": "DEMOTION", "reason": reason.as_str()}),
    }
}

fn parse_evidence(value: &Value) -> Option<LifecycleEvidence> {
    use LifecycleEvidence::*;
    let object = value.as_object()?;
    let flag = |key: &str| object.get(key)?.as_bool();
    let keys = |n: usize| (object.len() == n).then_some(());
    Some(match object.get("evidence")?.as_str()? {
        "READY_FOR_ASSESSMENT" => keys(1).map(|_| ReadyForAssessment)?,
        "CAPABILITY_VERIFICATION" => {
            keys(3)?;
            let runtime_id =
                RuntimeId::try_new(object.get("runtime_id")?.as_str()?.to_owned()).ok()?;
            let required_capabilities = object
                .get("required_capabilities")?
                .as_array()?
                .iter()
                .map(|c| CapabilityId::try_new(c.as_str()?.to_owned()).ok())
                .collect::<Option<Vec<_>>>()?;
            CapabilityVerification(crate::intelligence::VerificationPath {
                runtime_id,
                required_capabilities,
            })
        }
        "BOUNDED_CALIBRATION_ADMISSION" => {
            keys(2)?;
            BoundedCalibrationAdmission {
                authorized: flag("authorized")?,
            }
        }
        "ROUTABLE_PROMOTION_LOCAL_CALIBRATION" => {
            keys(2)?;
            RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                sufficient_acceptable_observations: flag("sufficient_acceptable_observations")?,
            })
        }
        "ROUTABLE_PROMOTION_OFFICIAL_AND_INDEPENDENT" => {
            keys(2)?;
            RoutablePromotion(RoutablePromotionEvidence::OfficialAndIndependent {
                ask_on_uncertainty_user_consent: flag("ask_on_uncertainty_user_consent")?,
            })
        }
        "DEMOTION" => {
            keys(2)?;
            let reason = object.get("reason")?.as_str()?;
            Demotion(
                DemotionReason::ALL
                    .into_iter()
                    .find(|r| r.as_str() == reason)?,
            )
        }
        _ => return None,
    })
}

fn observation_of(record: &ModelCapabilityRecord) -> Observation {
    Observation {
        confidence: record.confidence(),
        source_ref: record.source_ref().cloned(),
        observed_at: record.observed_at().clone(),
    }
}

/// Replays one stored record onto `service`.
fn replay(service: &mut ModelIntelligenceService, record: &ModelCapabilityRecord) -> Option<()> {
    let id = |v: Option<&str>| v.map(str::to_owned);
    let provider = || ProviderId::try_new(id(record.provider_id())?).ok();
    let model = || ModelId::try_new(id(record.model_id())?).ok();
    let runtime = || RuntimeId::try_new(id(record.runtime_id())?).ok();
    let truthy = record.value() == Some(&Value::Bool(true));
    let observation = observation_of(record);
    let ok = |r: Result<(), RegistryError>| r.ok();
    match (record.capability(), record.subject_kind()) {
        (PROVIDER, SubjectKind::Model) if truthy && record.model_id().is_none() => {
            ok(service.insert_provider(provider()?, observation))
        }
        (MODEL, SubjectKind::Model)
            if truthy && record.lifecycle_state() == Some(LifecycleState::Discovered) =>
        {
            ok(service.discover_model(provider()?, model()?, observation))
        }
        (RUNTIME, SubjectKind::Runtime) if truthy => {
            ok(service.insert_runtime(provider()?, runtime()?, observation))
        }
        (ASSOCIATION, SubjectKind::ModelRuntimePair) if truthy => {
            ok(service.associate_runtime(provider()?, model()?, runtime()?, observation))
        }
        (LIFECYCLE, SubjectKind::Model) => {
            let transition = LifecycleTransition {
                target: record.lifecycle_state()?,
                evidence: parse_evidence(record.value()?)?,
                observation,
            };
            ok(service.try_transition(provider()?.as_str(), model()?.as_str(), transition))
        }
        (name, _) if name.starts_with(RESERVED_CAPABILITY_PREFIX) => None,
        _ => ok(service.record_capability(record.to_evidence().ok()?)),
    }
}

/// Registry probe capability names for the typed Runtime reports. These are
/// runtime flag-surface facts, not model identities.
pub(crate) fn probe_entries(report: &RuntimeCapabilities) -> Vec<(String, CapabilityValue)> {
    let codex = |e: CodexCapabilityEvidence| match e {
        CodexCapabilityEvidence::Supported => CapabilityValue::Boolean(true),
        CodexCapabilityEvidence::Unknown => CapabilityValue::Unknown,
    };
    let claude = |e: ClaudeCapabilityEvidence| match e {
        ClaudeCapabilityEvidence::Supported => CapabilityValue::Boolean(true),
        ClaudeCapabilityEvidence::Unknown => CapabilityValue::Unknown,
    };
    let named = |pairs: Vec<(&str, CapabilityValue)>| {
        pairs
            .into_iter()
            .map(|(n, v)| (format!("runtime_probe.{n}"), v))
            .collect()
    };
    match report {
        RuntimeCapabilities::Codex(r) => named(vec![
            ("json", codex(r.json)),
            ("output_schema", codex(r.output_schema)),
            ("sandbox", codex(r.sandbox)),
        ]),
        RuntimeCapabilities::Claude(r) => named(vec![
            ("print", claude(r.print)),
            ("input_format", claude(r.input_format)),
            ("output_format", claude(r.output_format)),
            ("no_session_persistence", claude(r.no_session_persistence)),
            ("permission_mode", claude(r.permission_mode)),
            ("verbose", claude(r.verbose)),
        ]),
        RuntimeCapabilities::Extension(r) => r
            .entries()
            .map(|(n, s)| {
                (
                    format!("runtime_probe.{n}"),
                    match s {
                        CapabilityStatus::Supported => CapabilityValue::Boolean(true),
                        CapabilityStatus::Unsupported => CapabilityValue::Boolean(false),
                        CapabilityStatus::Unknown => CapabilityValue::Unknown,
                    },
                )
            })
            .collect(),
        RuntimeCapabilities::Unknown => Vec::new(),
    }
}

impl PersistentRegistry {
    /// An empty registry for `project_id` (nothing stored yet).
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            service: ModelIntelligenceService::new(),
            project_id: project_id.into(),
            next_sequence: 1,
        }
    }

    /// Rebuilds the registry by replaying every stored record in order.
    pub fn reload<E>(
        reader: &SqliteStateRepository,
        project_id: &str,
    ) -> Result<Self, RegistryPersistError<E>> {
        let stored = reader
            .list_decision_records(project_id, DecisionRecordKind::RegistryCapability, None)
            .map_err(|e| RegistryPersistError::Persist(RoutingPersistError::State(e)))?;
        let mut service = ModelIntelligenceService::new();
        for (index, r) in stored.iter().enumerate() {
            let expected = format!("registry-{:020}", index + 1);
            let record = decode_model_capability(r.record_json())
                .map_err(|_| RegistryPersistError::CorruptHistory(index))?;
            if r.record_id() != expected || replay(&mut service, &record).is_none() {
                return Err(RegistryPersistError::CorruptHistory(index));
            }
        }
        Ok(Self {
            service,
            project_id: project_id.to_owned(),
            next_sequence: stored.len() as u64 + 1,
        })
    }

    pub fn registry(&self) -> &Registry {
        self.service.registry()
    }

    /// Persists `changes` (validated in order on a copy) as ONE publication:
    /// all records and events become durable together or not at all. Memory
    /// advances only after the port reports success, so a failed publish
    /// leaves no partial publication. Lost acknowledgement may leave memory
    /// behind State; the identical call retried returns the
    /// original receipts.
    fn append_all<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        changes: Vec<(ModelCapabilityRecord, EventType)>,
    ) -> Result<Vec<EventPersistReceiptV1>, RegistryPersistError<P::Error>> {
        if changes.is_empty() {
            return Ok(Vec::new());
        }
        let mut next = ModelIntelligenceService {
            registry: self.service.registry.clone(),
        };
        let mut publication = Publication::default();
        for (offset, (record, event)) in changes.iter().enumerate() {
            if replay(&mut next, record).is_none() {
                return Err(RegistryPersistError::ReplayMismatch);
            }
            let stored = DecisionRecordV1::parse(
                &self.project_id,
                DecisionRecordKind::RegistryCapability,
                &format!("registry-{:020}", self.next_sequence + offset as u64),
                &encode_model_capability(record),
            )
            .map_err(|e| RegistryPersistError::Persist(RoutingPersistError::State(e)))?;
            let event = record_event(
                scope,
                *event,
                EventSubject {
                    kind: StateSubjectKind::Provider,
                    id: record.provider_id().unwrap_or_default().to_owned(),
                },
                &stored,
                stored.record_id(),
            )
            .map_err(|e| RegistryPersistError::Persist(RoutingPersistError::State(e)))?;
            publication.events.push(event);
            publication.records.push(stored);
        }
        let receipt = publish(port, &publication)?;
        self.service = next;
        self.next_sequence += changes.len() as u64;
        Ok(receipt.events)
    }

    fn append<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        record: ModelCapabilityRecord,
        event: EventType,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        Ok(self
            .append_all(port, scope, vec![(record, event)])?
            .pop()
            .expect("one change, one receipt"))
    }

    pub fn insert_provider<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider_id: ProviderId,
        discovery: Observation,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        self.service
            .clone_check(|s| s.insert_provider(provider_id.clone(), discovery.clone()))?;
        let r = structural(
            SubjectKind::Model,
            &provider_id,
            None,
            None,
            PROVIDER,
            Value::Bool(true),
            &discovery,
            None,
        );
        self.append(port, scope, r, EventType::RegistryRefreshed)
    }

    pub fn discover_model<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider_id: ProviderId,
        model_id: ModelId,
        discovery: Observation,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        self.service.clone_check(|s| {
            s.discover_model(provider_id.clone(), model_id.clone(), discovery.clone())
        })?;
        let r = structural(
            SubjectKind::Model,
            &provider_id,
            Some(&model_id),
            None,
            MODEL,
            Value::Bool(true),
            &discovery,
            Some(LifecycleState::Discovered),
        );
        self.append(port, scope, r, EventType::ModelDiscovered)
    }

    pub fn insert_runtime<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider_id: ProviderId,
        runtime_id: RuntimeId,
        discovery: Observation,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        self.service.clone_check(|s| {
            s.insert_runtime(provider_id.clone(), runtime_id.clone(), discovery.clone())
        })?;
        let r = structural(
            SubjectKind::Runtime,
            &provider_id,
            None,
            Some(&runtime_id),
            RUNTIME,
            Value::Bool(true),
            &discovery,
            None,
        );
        self.append(port, scope, r, EventType::RegistryRefreshed)
    }

    pub fn associate_runtime<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider_id: ProviderId,
        model_id: ModelId,
        runtime_id: RuntimeId,
        observation: Observation,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        self.service.clone_check(|s| {
            s.associate_runtime(
                provider_id.clone(),
                model_id.clone(),
                runtime_id.clone(),
                observation.clone(),
            )
        })?;
        let r = structural(
            SubjectKind::ModelRuntimePair,
            &provider_id,
            Some(&model_id),
            Some(&runtime_id),
            ASSOCIATION,
            Value::Bool(true),
            &observation,
            None,
        );
        self.append(port, scope, r, EventType::RegistryRefreshed)
    }

    pub fn record_capability<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        evidence: CapabilityEvidence,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        if evidence
            .capability
            .as_str()
            .starts_with(RESERVED_CAPABILITY_PREFIX)
        {
            return Err(RegistryPersistError::ReservedCapability);
        }
        self.service
            .clone_check(|s| s.record_capability(evidence.clone()))?;
        let r = ModelCapabilityRecord::from_evidence(&evidence);
        self.append(port, scope, r, EventType::RegistryRefreshed)
    }

    pub fn try_transition<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider: &ProviderId,
        model: &ModelId,
        transition: LifecycleTransition,
    ) -> Result<EventPersistReceiptV1, RegistryPersistError<P::Error>> {
        self.service.clone_check(|s| {
            s.try_transition(provider.as_str(), model.as_str(), transition.clone())
        })?;
        let r = structural(
            SubjectKind::Model,
            provider,
            Some(model),
            None,
            LIFECYCLE,
            evidence_value(&transition.evidence),
            &transition.observation,
            Some(transition.target),
        );
        self.append(port, scope, r, EventType::ModelLifecycleChanged)
    }

    /// Records one Runtime probe report as RUNTIME-subject LOCAL_EMPIRICAL
    /// evidence (`runtime_probe.<flag>`): Supported → true, Unsupported →
    /// false, Unknown → UNKNOWN. A whole-report `Unknown` records nothing.
    /// Probe evidence describes the runtime; it never confirms a
    /// model/runtime pair capability or promotes a model.
    #[allow(clippy::too_many_arguments)] // Probe identity, report and provenance.
    pub fn record_runtime_probe<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        provider_id: &ProviderId,
        runtime_id: &RuntimeId,
        report: &RuntimeCapabilities,
        observed_at: crate::policy_eligibility::ModelRoutingDateTimeV1,
        source_ref: crate::EvidenceSourceRef,
    ) -> Result<Vec<EventPersistReceiptV1>, RegistryPersistError<P::Error>> {
        if let RuntimeCapabilities::Extension(r) = report
            && r.runtime_id() != runtime_id.as_str()
        {
            return Err(RegistryPersistError::ProbeRuntimeMismatch);
        }
        let evidence: Vec<_> = probe_entries(report)
            .into_iter()
            .map(|(name, value)| CapabilityEvidence {
                subject: CapabilitySubject::Runtime {
                    provider_id: provider_id.clone(),
                    runtime_id: runtime_id.clone(),
                },
                capability: CapabilityId::try_new(name).expect("non-empty"),
                value: Some(value),
                observation: Observation {
                    confidence: EvidenceConfidence::LocalEmpirical,
                    source_ref: Some(source_ref.clone()),
                    observed_at: observed_at.clone(),
                },
                sample_size: None,
            })
            .collect();
        // The whole report is one publication: it is accepted entirely or
        // not at all, so a retry can never duplicate part of it.
        self.service.clone_check(|s| {
            evidence
                .iter()
                .try_for_each(|e| s.record_capability(e.clone()))
        })?;
        let changes = evidence
            .iter()
            .map(|e| {
                (
                    ModelCapabilityRecord::from_evidence(e),
                    EventType::RegistryRefreshed,
                )
            })
            .collect();
        self.append_all(port, scope, changes)
    }
}

impl ModelIntelligenceService {
    /// Runs `op` on a copy to surface its exact refusal without mutating.
    fn clone_check(
        &self,
        op: impl FnOnce(&mut Self) -> Result<(), RegistryError>,
    ) -> Result<(), RegistryError> {
        op(&mut Self {
            registry: self.registry.clone(),
        })
    }
}
