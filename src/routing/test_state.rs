//! Test-only trusted composition: implements [`RoutingStatePort`] over a real
//! exclusive `TrustedCoreStore`, the way the composition root will over the
//! core's `CoreStateWriter`. Event content is resolved from State by the
//! proposal's reference and compared with both the supplied bytes and the
//! proposal digest before anything is persisted. Synthetic same-user store.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_state::{
    AvailabilityFactReceiptV1, AvailabilityFactV1, DecisionRecordKind, DecisionRecordV1,
    EventPersistReceiptV1, EventProposalV1, SqliteStateRepository, StateEpochValueV1, StateError,
    StateWriteOperationV1 as Op, StateWriteSubjectV1, TrustedClockV1, TrustedCoreStore,
    TrustedTimeSampleV1,
};

use crate::persistence::{EventScope, RoutingStatePort};

pub(crate) const PROJECT: &str = "project-routing";

struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-11-08T10:00:00.000000000Z".into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

#[derive(Debug)]
pub(crate) enum HarnessError {
    #[allow(dead_code)] // Read through Debug in assertion messages.
    State(StateError),
    ContentMismatch,
}

pub(crate) struct CoreHarness {
    dir: PathBuf,
    store: Option<TrustedCoreStore>,
    next_event: u64,
}

impl CoreHarness {
    pub(crate) fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "receipts-routing-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let store = TrustedCoreStore::open_exclusive(dir.join("state.db")).unwrap();
        Self {
            dir,
            store: Some(store),
            next_event: 1,
        }
    }

    /// Closes the exclusive store and opens it again (a restart).
    pub(crate) fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(TrustedCoreStore::open_exclusive(self.dir.join("state.db")).unwrap());
    }

    pub(crate) fn scope() -> EventScope {
        EventScope {
            project_id: PROJECT.into(),
            goal_id: None,
            epoch: StateEpochValueV1::try_from(1_i64).unwrap(),
        }
    }

    fn store(&mut self) -> &mut TrustedCoreStore {
        self.store.as_mut().unwrap()
    }

    fn admit(&self, operation: Op, project: &str) -> receipts_state::StateWriteAdmission {
        self.store.as_ref().unwrap().admit(StateWriteSubjectV1 {
            operation,
            project_id: project.into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        })
    }

    /// Resolves `state-query:decision_record/<KIND>/<id>` from State.
    fn resolve(&self, project: &str, reference: &str) -> Option<DecisionRecordV1> {
        let rest = reference.strip_prefix("state-query:decision_record/")?;
        let (kind, id) = rest.split_once('/')?;
        let kind = DecisionRecordKind::ALL
            .into_iter()
            .find(|k| k.as_str() == kind)?;
        self.reader()
            .find_decision_record(project, kind, id)
            .ok()
            .flatten()
    }
}

impl Drop for CoreHarness {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl RoutingStatePort for CoreHarness {
    type Error = HarnessError;

    fn reader(&self) -> &SqliteStateRepository {
        self.store.as_ref().unwrap().reader()
    }

    fn append_decision_record(&mut self, record: &DecisionRecordV1) -> Result<(), HarnessError> {
        let admission = self.admit(Op::DecisionRecord, record.project_id());
        self.store()
            .append_decision_record(admission, record)
            .map_err(HarnessError::State)
    }

    fn persist_event_proposal(
        &mut self,
        proposal: &EventProposalV1,
        content: &[u8],
    ) -> Result<EventPersistReceiptV1, HarnessError> {
        let stored = self
            .resolve(proposal.project_id(), &proposal.payload().reference)
            .ok_or(HarnessError::ContentMismatch)?;
        if stored.record_json().as_bytes() != content
            || proposal.payload().digest != format!("sha256:{}", stored.record_sha256())
        {
            return Err(HarnessError::ContentMismatch);
        }
        let event_id = format!("01ARZ3NDEKTSV4RRFFQ69G{:04}", self.next_event);
        self.next_event += 1;
        let admission = self.admit(Op::EventProposal, proposal.project_id());
        self.store()
            .persist_event_proposal(admission, &Clock, proposal, &event_id)
            .map_err(HarnessError::State)
    }

    fn append_availability_fact(
        &mut self,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, HarnessError> {
        let admission = self.admit(Op::AvailabilityFact, fact.project_id());
        self.store()
            .append_availability_fact(admission, &Clock, fact)
            .map_err(HarnessError::State)
    }
}

// ---- shared routing fixtures ---------------------------------------------------

use crate::intelligence::{
    LifecycleEvidence, LifecycleState, LifecycleTransition, RoutablePromotionEvidence,
    VerificationPath,
};
use crate::policy_eligibility::{
    ModelRoutingDateTimeV1, PolicyEvidenceLabel, PolicyStatus, ProviderPolicyEligibility,
    TechnicalStatus,
};
use crate::registry::{
    CapabilityEvidence, CapabilityId, CapabilitySubject, CapabilityValue, ModelId,
    ModelIntelligenceService, Observation, ProviderId, RuntimeId,
};
use crate::router::quality_tier_capability;
use crate::{
    AvailabilitySignalSource, AvailabilityState, AvailabilityStateKind,
    AvailabilityStateNonTemporalCore, EvidenceConfidence, EvidenceSourceRef, EvidenceSourceRefType,
    RoutingQualityFloor,
};

pub(crate) const CONTEXT: &str = "LOCAL_EXTERNAL_WORKER";
pub(crate) const NOW: &str = "2026-11-08T10:00:00Z";
pub(crate) const CUTOFF: &str = "2026-10-01T00:00:00Z";
pub(crate) const FRESH: &str = "2026-11-01T00:00:00Z";
pub(crate) const STALE: &str = "2026-09-01T00:00:00Z";

pub(crate) fn ts(text: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(text.into()).unwrap()
}

pub(crate) fn source() -> EvidenceSourceRef {
    EvidenceSourceRef::try_new(
        EvidenceSourceRefType::ArtifactId,
        "synthetic-routing-evidence".into(),
        None,
        None,
    )
    .unwrap()
}

pub(crate) fn obs(confidence: EvidenceConfidence, at: &str) -> Observation {
    Observation {
        confidence,
        source_ref: Some(source()),
        observed_at: ts(at),
    }
}

pub(crate) fn ids(p: &str, m: &str, r: &str) -> (ProviderId, ModelId, RuntimeId) {
    (
        ProviderId::try_new(p.into()).unwrap(),
        ModelId::try_new(m.into()).unwrap(),
        RuntimeId::try_new(r.into()).unwrap(),
    )
}

/// One synthetic candidate.
#[derive(Clone)]
pub(crate) struct Spec {
    pub p: &'static str,
    pub m: &'static str,
    pub r: &'static str,
    /// Tier claims: (tier, value, confidence, observed_at).
    pub tiers: Vec<(
        RoutingQualityFloor,
        Option<CapabilityValue>,
        EvidenceConfidence,
        &'static str,
    )>,
    pub routable: bool,
    /// Pair capabilities confirmed OFFICIAL_VERIFIED (required are a subset).
    pub caps: Vec<&'static str>,
}

impl Spec {
    pub(crate) fn frontier(p: &'static str, m: &'static str, r: &'static str) -> Self {
        Self {
            p,
            m,
            r,
            tiers: vec![(
                RoutingQualityFloor::Frontier,
                Some(CapabilityValue::Boolean(true)),
                EvidenceConfidence::OfficialVerified,
                FRESH,
            )],
            routable: true,
            caps: vec!["coding"],
        }
    }
    pub(crate) fn tier(mut self, tier: RoutingQualityFloor) -> Self {
        self.tiers[0].0 = tier;
        self
    }
}

/// The ordered registry operations for `specs` (shared by in-memory and
/// persistent builders).
pub(crate) enum RegistryOp {
    Provider(ProviderId),
    Model(ProviderId, ModelId),
    Runtime(ProviderId, RuntimeId),
    Associate(ProviderId, ModelId, RuntimeId),
    Evidence(CapabilityEvidence),
    Transition(ProviderId, ModelId, LifecycleTransition),
}

pub(crate) fn registry_ops(specs: &[Spec]) -> Vec<RegistryOp> {
    use RegistryOp::*;
    let mut ops = Vec::new();
    let (mut providers, mut models, mut runtimes) = (Vec::new(), Vec::new(), Vec::new());
    for s in specs {
        let (p, m, r) = ids(s.p, s.m, s.r);
        if !providers.contains(&s.p) {
            providers.push(s.p);
            ops.push(Provider(p.clone()));
        }
        if !models.contains(&(s.p, s.m)) {
            models.push((s.p, s.m));
            ops.push(Model(p.clone(), m.clone()));
        }
        if !runtimes.contains(&(s.p, s.r)) {
            runtimes.push((s.p, s.r));
            ops.push(Runtime(p.clone(), r.clone()));
        }
        ops.push(Associate(p.clone(), m.clone(), r.clone()));
        let model_subject = CapabilitySubject::Model {
            provider_id: p.clone(),
            model_id: m.clone(),
        };
        let pair = CapabilitySubject::ModelRuntimePair {
            provider_id: p.clone(),
            model_id: m.clone(),
            runtime_id: r.clone(),
        };
        let fact = |subject: &CapabilitySubject, cap: &str, confidence| {
            Evidence(CapabilityEvidence {
                subject: subject.clone(),
                capability: CapabilityId::try_new(cap.into()).unwrap(),
                value: Some(CapabilityValue::Boolean(true)),
                observation: obs(confidence, FRESH),
                sample_size: None,
            })
        };
        for (tier, value, confidence, at) in &s.tiers {
            ops.push(Evidence(CapabilityEvidence {
                subject: model_subject.clone(),
                capability: quality_tier_capability(*tier),
                value: *value,
                observation: obs(*confidence, at),
                sample_size: None,
            }));
        }
        for cap in &s.caps {
            ops.push(fact(
                &model_subject,
                cap,
                EvidenceConfidence::OfficialVerified,
            ));
            ops.push(fact(&pair, cap, EvidenceConfidence::OfficialVerified));
            ops.push(fact(&pair, cap, EvidenceConfidence::LocalEmpirical));
        }
        if s.routable && !s.caps.is_empty() {
            let step = |target, evidence, confidence| {
                Transition(
                    p.clone(),
                    m.clone(),
                    LifecycleTransition {
                        target,
                        evidence,
                        observation: obs(confidence, FRESH),
                    },
                )
            };
            use EvidenceConfidence::*;
            ops.push(step(
                LifecycleState::Unassessed,
                LifecycleEvidence::ReadyForAssessment,
                OfficialVerified,
            ));
            ops.push(step(
                LifecycleState::CapabilityVerified,
                LifecycleEvidence::CapabilityVerification(VerificationPath {
                    runtime_id: r.clone(),
                    required_capabilities: s
                        .caps
                        .iter()
                        .map(|c| CapabilityId::try_new((*c).into()).unwrap())
                        .collect(),
                }),
                OfficialVerified,
            ));
            ops.push(step(
                LifecycleState::Calibrating,
                LifecycleEvidence::BoundedCalibrationAdmission { authorized: true },
                OfficialVerified,
            ));
            ops.push(step(
                LifecycleState::Routable,
                LifecycleEvidence::RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                    sufficient_acceptable_observations: true,
                }),
                LocalEmpirical,
            ));
        }
    }
    ops
}

pub(crate) fn build(specs: &[Spec]) -> ModelIntelligenceService {
    let mut s = ModelIntelligenceService::new();
    let o = || obs(EvidenceConfidence::Unverified, FRESH);
    for op in registry_ops(specs) {
        match op {
            RegistryOp::Provider(p) => s.insert_provider(p, o()),
            RegistryOp::Model(p, m) => s.discover_model(p, m, o()),
            RegistryOp::Runtime(p, r) => s.insert_runtime(p, r, o()),
            RegistryOp::Associate(p, m, r) => s.associate_runtime(p, m, r, o()),
            RegistryOp::Evidence(e) => s.record_capability(e),
            RegistryOp::Transition(p, m, t) => s.try_transition(p.as_str(), m.as_str(), t),
        }
        .unwrap();
    }
    s
}

pub(crate) fn availability(
    p: &str,
    m: &str,
    r: &str,
    state: AvailabilityStateKind,
    at: &str,
) -> AvailabilityState {
    AvailabilityState::new(
        AvailabilityStateNonTemporalCore::try_new(
            p.into(),
            Some(m.into()),
            Some(r.into()),
            state,
            None,
            Some(AvailabilitySignalSource::Probe),
            None,
        )
        .unwrap(),
        ts(at),
    )
}

pub(crate) fn policy(p: &str, r: &str, status: PolicyStatus) -> ProviderPolicyEligibility {
    ProviderPolicyEligibility::try_new(
        p.into(),
        r.into(),
        "USER_API".into(),
        TechnicalStatus::Connected,
        status,
        Some(vec![CONTEXT.into()]),
        ts(FRESH),
        None,
        PolicyEvidenceLabel::VerifiedCurrentSelfFetched,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}
