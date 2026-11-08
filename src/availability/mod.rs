//! Availability/quota manager, bounded backoff and failover (MR-W2-2).
//!
//! Typed Runtime [`FailureClass`] observations drive availability:
//! `RateLimited` → RATE_LIMITED, `SessionExhausted` → SESSION_EXHAUSTED and
//! `ProviderDown` → PROVIDER_DOWN, each for a window of `retry_after` when the
//! observation carries one, else capped exponential backoff with jitter per
//! (provider, model) — the attempt history is shared by every runtime of the
//! model, so changing runtime never restarts it. `SafetyCheckPending`/
//! `PolicyBlocked` never change availability: they are holds for the safety
//! protocol and stop failover (I-12, no provider-shopping), and an active
//! limit never overwrites a safety, policy or authentication base state
//! ([`AvailabilityManager::effective`]). Every other class (timeouts,
//! crashes, invalid output, cancellation, auth, unknown) is not an
//! availability signal here.
//!
//! Quota scope stays UNKNOWN unless a provider-reported QuotaState verifies
//! it ([`QuotaState::verified_scope`]) AND, for MODEL scope, names the
//! observed model; evidence for another provider or model is refused. With
//! UNKNOWN scope a limit marks only the exact candidate and the provider
//! DEGRADED for the same window — never sibling models RATE_LIMITED. A
//! verified PROVIDER (MODEL) scope limits every runtime of that provider
//! (model).
//!
//! The manager keeps the effect records it derived (AvailabilityState
//! values, append order) so it can be persisted and replayed after restart;
//! each record's note carries the attempt and, for a backoff window, the
//! ceiling and jitter drawn. An elapsed window is not recovery evidence: the
//! candidate becomes retryable as DEGRADED until caller evidence newer than
//! the limit reports otherwise. Nothing here reports a provider recovered
//! (no authenticated producer exists; D5).

pub mod failover;
#[cfg(test)]
mod tests;

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;

use receipts_runtime_adapters::FailureClass;
use receipts_state::{
    AvailabilityFactKindV1, AvailabilityFactReceiptV1, AvailabilityFactSourceV1,
    AvailabilityFactV1, DecisionRecordKind, DecisionRecordV1, EventPersistReceiptV1, EventSubject,
    EventType, SqliteStateRepository, SubjectKind,
};

use crate::persistence::{
    EventScope, Publication, RoutingPersistError, RoutingStatePort, publish, record_event,
    record_reference,
};
use crate::policy_eligibility::{
    ModelRoutingDateTimeComparisonError, ModelRoutingDateTimeV1, compare_instants,
    elapsed_whole_seconds,
};
use crate::registry::{ModelId, ProviderId, RuntimeId};
use crate::wire::{decode_availability_state, encode_availability_state};
use crate::{
    AvailabilitySignalSource, AvailabilityState, AvailabilityStateKind,
    AvailabilityStateNonTemporalCore, QuotaScope, QuotaState,
};

/// Capped exponential backoff with jitter: attempt n has the ceiling
/// `min(cap, base·2^(n-1))` and waits a jittered `ceil(ceiling/2) ..= ceiling`
/// ("equal jitter"), so a window is never zero, never exceeds the cap and
/// concurrent workers spread out. Values are deployment configuration
/// (calibration knobs), not vendor data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    base_seconds: u64,
    cap_seconds: u64,
    max_attempts: u32,
}

impl BackoffPolicy {
    /// `1 <= base <= cap`, `max_attempts >= 1`.
    pub fn try_new(base_seconds: u64, cap_seconds: u64, max_attempts: u32) -> Option<Self> {
        (base_seconds >= 1 && base_seconds <= cap_seconds && max_attempts >= 1).then_some(Self {
            base_seconds,
            cap_seconds,
            max_attempts,
        })
    }
    /// The un-jittered ceiling for `attempt` (saturating, never above the cap).
    pub fn delay_seconds(&self, attempt: u32) -> u64 {
        let factor = 1u64
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u64::MAX);
        self.base_seconds
            .saturating_mul(factor)
            .min(self.cap_seconds)
    }
    /// The inclusive window range for `attempt`: `(ceil(ceiling/2), ceiling)`.
    pub fn window_range(&self, attempt: u32) -> (u64, u64) {
        let ceiling = self.delay_seconds(attempt);
        (ceiling - ceiling / 2, ceiling)
    }
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

/// Where backoff jitter comes from. Production uses [`RandomJitter`]; tests
/// inject a controlled source. The manager clamps every draw to its bound.
pub trait JitterSource: Send + Sync + std::fmt::Debug {
    /// A value in `0..=max` for the backoff of `(provider, model)` at `attempt`.
    fn pick(&self, provider: &str, model: &str, attempt: u32, max: u64) -> u64;
}

/// Process-random jitter from the standard library's randomly keyed hasher.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomJitter;

impl JitterSource for RandomJitter {
    fn pick(&self, provider: &str, model: &str, attempt: u32, max: u64) -> u64 {
        let mut hasher = RandomState::new().build_hasher();
        (provider, model, attempt).hash(&mut hasher);
        let draw = hasher.finish();
        match max.checked_add(1) {
            Some(span) => draw % span,
            None => draw,
        }
    }
}

/// A controlled source: always draws `self.0` (clamped to the bound).
#[derive(Debug, Clone, Copy)]
pub struct FixedJitter(pub u64);

impl JitterSource for FixedJitter {
    fn pick(&self, _: &str, _: &str, _: u32, max: u64) -> u64 {
        self.0.min(max)
    }
}

/// One typed Runtime observation for an exact candidate.
#[derive(Debug, Clone)]
pub struct AvailabilityObservation {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub runtime_id: RuntimeId,
    pub failure: FailureClass,
    /// Provider-supplied retry-after, when observable.
    pub retry_after_seconds: Option<u64>,
    pub signal_source: AvailabilitySignalSource,
    pub observed_at: ModelRoutingDateTimeV1,
    /// Provider quota report accompanying the failure, if any.
    pub quota: Option<QuotaState>,
}

/// What one observation means for availability.
#[derive(Debug, Clone, PartialEq)]
pub enum ObservationEffect {
    Limited {
        state: AvailabilityStateKind,
        window_seconds: u64,
        attempt: u32,
        /// `attempt` exceeded the policy's bound: further failover ASKs.
        exhausted: bool,
        quota_scope: QuotaScope,
        /// Effect records to append (exact candidate first).
        records: Vec<AvailabilityState>,
    },
    /// Safety/policy refusal: availability unchanged, failover forbidden.
    SafetyOrPolicyHold(FailureClass),
    NotAnAvailabilitySignal(FailureClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityError {
    Time(ModelRoutingDateTimeComparisonError),
    /// The quota report names another provider.
    QuotaProviderMismatch,
    /// The quota report names another model than the observed one.
    QuotaModelMismatch,
    /// Observation older than the latest record for the same candidate.
    OutOfOrder,
}

impl From<ModelRoutingDateTimeComparisonError> for AvailabilityError {
    fn from(e: ModelRoutingDateTimeComparisonError) -> Self {
        Self::Time(e)
    }
}

const fn is_limit(state: AvailabilityStateKind) -> bool {
    matches!(
        state,
        AvailabilityStateKind::RateLimited
            | AvailabilityStateKind::SessionExhausted
            | AvailabilityStateKind::ProviderDown
    )
}

fn window_of(record: &AvailabilityState) -> u64 {
    record.core().retry_after_seconds().flatten().unwrap_or(0)
}

/// Does `record` apply to the exact candidate?
fn covers(record: &AvailabilityState, provider: &str, model: &str, runtime: &str) -> bool {
    let c = record.core();
    c.provider_id() == provider
        && c.model_id().is_none_or(|m| m == model)
        && c.runtime_id().is_none_or(|r| r == runtime)
}

#[derive(Debug, Clone)]
pub struct AvailabilityManager {
    policy: BackoffPolicy,
    jitter: Arc<dyn JitterSource>,
    records: Vec<AvailabilityState>,
}

impl AvailabilityManager {
    /// A manager with process-random jitter.
    pub fn new(policy: BackoffPolicy) -> Self {
        Self {
            policy,
            jitter: Arc::new(RandomJitter),
            records: Vec::new(),
        }
    }

    /// The same manager drawing its backoff jitter from `jitter`.
    pub fn with_jitter(mut self, jitter: Arc<dyn JitterSource>) -> Self {
        self.jitter = jitter;
        self
    }

    /// Effect records in append order.
    pub fn records(&self) -> &[AvailabilityState] {
        &self.records
    }

    /// Derives the effect of one observation without changing the manager.
    pub fn assess(
        &self,
        obs: &AvailabilityObservation,
    ) -> Result<ObservationEffect, AvailabilityError> {
        let state = match obs.failure {
            FailureClass::RateLimited => AvailabilityStateKind::RateLimited,
            FailureClass::SessionExhausted => AvailabilityStateKind::SessionExhausted,
            FailureClass::ProviderDown => AvailabilityStateKind::ProviderDown,
            FailureClass::SafetyCheckPending | FailureClass::PolicyBlocked => {
                return Ok(ObservationEffect::SafetyOrPolicyHold(obs.failure));
            }
            other => return Ok(ObservationEffect::NotAnAvailabilitySignal(other)),
        };
        let (p, m, r) = (
            obs.provider_id.as_str(),
            obs.model_id.as_str(),
            obs.runtime_id.as_str(),
        );
        let quota_scope = match &obs.quota {
            Some(q) if q.core().provider_id() != &obs.provider_id => {
                return Err(AvailabilityError::QuotaProviderMismatch);
            }
            Some(q) if q.model_id().is_some_and(|named| named != m) => {
                return Err(AvailabilityError::QuotaModelMismatch);
            }
            // MODEL scope is proven only by a report that names this model.
            Some(q) => match q.verified_scope() {
                QuotaScope::Model if q.model_id().is_none() => QuotaScope::Unknown,
                scope => scope,
            },
            None => QuotaScope::Unknown,
        };
        // Consecutive limits on the (provider, model) raise the attempt
        // whatever the runtime; a quiet period of two windows after the last
        // one resets it.
        let mut attempt = 1;
        if let Some(last) = self.records.iter().rev().find(|x| {
            let c = x.core();
            c.provider_id() == p && c.model_id() == Some(m)
        }) {
            let since = elapsed_whole_seconds(last.observed_at(), &obs.observed_at)?;
            if since < 0 {
                return Err(AvailabilityError::OutOfOrder);
            }
            let window = window_of(last);
            if (since as u64) < window.saturating_mul(2) {
                attempt = note_attempt(last).saturating_add(1);
            }
        }
        // A provider's retry-after is authoritative; otherwise the backoff
        // window is jittered inside its bounds.
        let (window_seconds, window_note) = match obs.retry_after_seconds {
            Some(window) => (window, "window_source=RETRY_AFTER".to_owned()),
            None => {
                let (low, ceiling) = self.policy.window_range(attempt);
                let drawn = self
                    .jitter
                    .pick(p, m, attempt, ceiling - low)
                    .min(ceiling - low);
                (
                    low + drawn,
                    format!(
                        "window_source=BACKOFF;backoff_ceiling={ceiling};jitter_seconds={drawn}"
                    ),
                )
            }
        };
        let record = |model: Option<&str>, runtime: Option<&str>, kind, note: String| {
            AvailabilityState::new(
                AvailabilityStateNonTemporalCore::try_new(
                    p.to_owned(),
                    model.map(str::to_owned),
                    runtime.map(str::to_owned),
                    kind,
                    Some(Some(window_seconds)),
                    Some(obs.signal_source),
                    Some(note),
                )
                .expect("registry identifiers are non-empty"),
                obs.observed_at.clone(),
            )
        };
        let note = format!(
            "failure_class={};attempt={attempt};quota_scope={};{window_note}",
            obs.failure.as_str(),
            quota_scope.as_str()
        );
        let mut records = vec![record(Some(m), Some(r), state, note.clone())];
        match (state, quota_scope) {
            (AvailabilityStateKind::RateLimited, QuotaScope::Provider) => {
                records.push(record(None, None, state, note));
            }
            (AvailabilityStateKind::RateLimited, QuotaScope::Model) => {
                records.push(record(Some(m), None, state, note));
            }
            (AvailabilityStateKind::RateLimited, QuotaScope::Unknown)
            | (AvailabilityStateKind::ProviderDown, _) => {
                records.push(record(None, None, AvailabilityStateKind::Degraded, note));
            }
            _ => {}
        }
        Ok(ObservationEffect::Limited {
            state,
            window_seconds,
            attempt,
            exhausted: attempt > self.policy.max_attempts,
            quota_scope,
            records,
        })
    }

    /// Appends an assessed effect's records (no-op for holds/non-signals).
    pub fn commit(&mut self, effect: &ObservationEffect) {
        if let ObservationEffect::Limited { records, .. } = effect {
            self.records.extend(records.iter().cloned());
        }
    }

    /// The availability to route on for one exact candidate at `now`, given
    /// the caller's base evidence for it. Active limits win (longest
    /// remaining window); an active provider DEGRADED, or an elapsed limit not
    /// superseded by newer base evidence, caps an AVAILABLE base at DEGRADED.
    pub fn effective(
        &self,
        base: &AvailabilityState,
        now: &ModelRoutingDateTimeV1,
    ) -> Result<AvailabilityState, AvailabilityError> {
        let c = base.core();
        let (Some(model), Some(runtime)) = (c.model_id(), c.runtime_id()) else {
            return Ok(base.clone());
        };
        // A safety, policy or authentication refusal is its own axis: a
        // limit never overwrites it, so it can neither turn into a WAIT nor
        // into failover (I-12) and keeps its authoritative reason.
        if matches!(
            c.state(),
            AvailabilityStateKind::SafetyCheckPending
                | AvailabilityStateKind::PolicyBlocked
                | AvailabilityStateKind::AuthRequired
        ) {
            return Ok(base.clone());
        }
        let mut worst: Option<(u64, &AvailabilityState)> = None;
        let mut degrade = false;
        for record in self
            .records
            .iter()
            .filter(|x| covers(x, c.provider_id(), model, runtime))
        {
            let elapsed = elapsed_whole_seconds(record.observed_at(), now)?;
            // Unsigned throughout: a window of any u64 stays active for as
            // long as it really lasts and never wraps into the past.
            let remaining = window_of(record).saturating_sub(elapsed.max(0).unsigned_abs());
            let kind = record.core().state();
            if remaining > 0 && is_limit(kind) {
                if worst.is_none_or(|(w, _)| remaining > w) {
                    worst = Some((remaining, record));
                }
            } else if remaining > 0
                || (is_limit(kind)
                    && compare_instants(base.observed_at(), record.observed_at())?
                        != std::cmp::Ordering::Greater)
            {
                degrade = true;
            }
        }
        if let Some((remaining, record)) = worst {
            return Ok(AvailabilityState::new(
                AvailabilityStateNonTemporalCore::try_new(
                    c.provider_id().to_owned(),
                    Some(model.to_owned()),
                    Some(runtime.to_owned()),
                    record.core().state(),
                    Some(Some(remaining)),
                    record.core().signal_source(),
                    Some("derived_at_decision".to_owned()),
                )
                .expect("identifiers come from the base"),
                now.clone(),
            ));
        }
        if degrade && c.state() == AvailabilityStateKind::Available {
            return Ok(AvailabilityState::new(
                AvailabilityStateNonTemporalCore::try_new(
                    c.provider_id().to_owned(),
                    Some(model.to_owned()),
                    Some(runtime.to_owned()),
                    AvailabilityStateKind::Degraded,
                    c.retry_after_seconds(),
                    c.signal_source(),
                    Some("degraded_after_limit".to_owned()),
                )
                .expect("identifiers come from the base"),
                base.observed_at().clone(),
            ));
        }
        Ok(base.clone())
    }

    /// Rebuilds a manager from its persisted effect records.
    pub fn reload(
        reader: &SqliteStateRepository,
        project_id: &str,
        policy: BackoffPolicy,
    ) -> Result<Self, RoutingPersistError<std::convert::Infallible>> {
        let records = reader
            .list_decision_records(
                project_id,
                DecisionRecordKind::AvailabilityObservation,
                None,
            )?
            .iter()
            .map(|r| {
                decode_availability_state(r.record_json()).map_err(RoutingPersistError::Decode)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            policy,
            jitter: Arc::new(RandomJitter),
            records,
        })
    }

    /// Persists an assessed effect — every record, its event and the D5
    /// demotion fact — as ONE publication, then commits it. The manager
    /// advances only after the port reports success, so a failed publish
    /// leaves memory equal to what a restart rebuilds, and the identical call
    /// retried after a lost acknowledgement returns the original receipts
    /// without a second demotion fact. Holds and non-signals persist nothing.
    pub fn persist_and_commit<P: RoutingStatePort>(
        &mut self,
        port: &mut P,
        scope: &EventScope,
        effect: &ObservationEffect,
    ) -> Result<PersistedAvailability, RoutingPersistError<P::Error>> {
        let ObservationEffect::Limited { records, state, .. } = effect else {
            return Ok(PersistedAvailability::default());
        };
        let mut publication = Publication::default();
        for (i, record) in records.iter().enumerate() {
            let sequence = self.records.len() + i + 1;
            let stored = DecisionRecordV1::parse(
                &scope.project_id,
                DecisionRecordKind::AvailabilityObservation,
                &format!("availability-{sequence:020}"),
                &encode_availability_state(record),
            )?;
            let core = record.core();
            let event = if core.state() == AvailabilityStateKind::Degraded
                || core.state() == AvailabilityStateKind::ProviderDown
            {
                EventType::ProviderDegraded
            } else {
                EventType::RateLimitObserved
            };
            publication.events.push(record_event(
                scope,
                event,
                EventSubject {
                    kind: SubjectKind::Provider,
                    id: core.provider_id().to_owned(),
                },
                &stored,
                stored.record_id(),
            )?);
            if i == 0 {
                publication.fact = Some(AvailabilityFactV1::new(
                    scope.project_id.clone(),
                    core.provider_id().to_owned(),
                    core.runtime_id().unwrap_or_default().to_owned(),
                    core.model_id().unwrap_or_default().to_owned(),
                    AvailabilityFactKindV1::Demotion,
                    state.as_str().to_owned(),
                    AvailabilityFactSourceV1::RuntimeFailureClass,
                    record_reference(&stored),
                    None,
                )?);
            }
            publication.records.push(stored);
        }
        let receipt = publish(port, &publication)?;
        self.commit(effect);
        Ok(PersistedAvailability {
            events: receipt.events,
            demotion: receipt.fact,
            replayed: receipt.replayed,
        })
    }
}

/// What persisting one effect produced.
#[derive(Debug, Default)]
pub struct PersistedAvailability {
    pub events: Vec<EventPersistReceiptV1>,
    /// The D5 demotion fact for the exact candidate.
    pub demotion: Option<AvailabilityFactReceiptV1>,
    /// This was an exact retry of an effect already made durable.
    pub replayed: bool,
}

fn note_attempt(record: &AvailabilityState) -> u32 {
    record
        .core()
        .note()
        .and_then(|n| n.split(';').find_map(|p| p.strip_prefix("attempt=")))
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}
