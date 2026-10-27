//! Pure latest-instant resolver over append-only capability evidence. It reads
//! one exact subject+capability list, uses `compare_instants` for every time
//! comparison and supplies no clock, TTL, authority ranking or fallback. Its
//! result is not dispatch or authenticity authority.
use super::{CapabilityEvidence, CapabilityId, CapabilitySubject, CapabilityValue, Registry};
use crate::policy_eligibility::{
    ModelRoutingDateTimeComparisonError, ModelRoutingDateTimeV1, compare_instants,
};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// observed_at is after the caller's now (equal is eligible; no skew).
    Future,
    /// No source_ref: cannot support a capability claim.
    Unsourced,
}

/// Classification of the selected latest instant against the caller's cutoff.
/// Stale outcomes keep their value, conflict and provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// freshness_cutoff is after now.
    CutoffAfterNow,
    Comparison(ModelRoutingDateTimeComparisonError),
}
impl From<ModelRoutingDateTimeComparisonError> for ResolveError {
    fn from(e: ModelRoutingDateTimeComparisonError) -> Self {
        Self::Comparison(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectedEvidence<'a> {
    pub evidence: &'a CapabilityEvidence,
    pub reason: RejectionReason,
}

/// `evidence` is every eligible record at the latest eligible instant, in
/// append order, with its provenance untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityOutcome<'a> {
    Established {
        value: bool,
        evidence: Vec<&'a CapabilityEvidence>,
    },
    /// Latest instant asserts UNKNOWN/omitted: never capability success.
    Unknown {
        evidence: Vec<&'a CapabilityEvidence>,
    },
    /// Different values at the latest instant; no winner chosen.
    Conflicted {
        evidence: Vec<&'a CapabilityEvidence>,
    },
    NoUsableEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityResolution<'a> {
    pub outcome: CapabilityOutcome<'a>,
    /// None only when no evidence was selected.
    pub freshness: Option<Freshness>,
    pub rejected: Vec<RejectedEvidence<'a>>,
}

impl Registry {
    /// The latest eligible instant (observed_at <= now, sourced) is selected
    /// first; it is Fresh iff freshness_cutoff <= it. freshness_cutoff > now is
    /// refused. Any unsupported comparison (second 60) fails the resolution.
    pub fn resolve_capability<'a>(
        &'a self,
        subject: &CapabilitySubject,
        capability: &CapabilityId,
        now: &ModelRoutingDateTimeV1,
        freshness_cutoff: &ModelRoutingDateTimeV1,
    ) -> Result<CapabilityResolution<'a>, ResolveError> {
        if compare_instants(freshness_cutoff, now)? == Ordering::Greater {
            return Err(ResolveError::CutoffAfterNow);
        }
        let mut rejected = Vec::new();
        let mut latest: Vec<&CapabilityEvidence> = Vec::new();
        for e in self
            .capability_evidence(subject, capability.as_str())
            .unwrap_or_default()
        {
            let at = &e.observation.observed_at;
            let reason = if compare_instants(at, now)? == Ordering::Greater {
                Some(RejectionReason::Future)
            } else if e.observation.source_ref.is_none() {
                Some(RejectionReason::Unsourced)
            } else {
                None
            };
            if let Some(reason) = reason {
                rejected.push(RejectedEvidence {
                    evidence: e,
                    reason,
                });
                continue;
            }
            match latest.first() {
                None => latest.push(e),
                Some(l) => match compare_instants(at, &l.observation.observed_at)? {
                    Ordering::Greater => latest = vec![e],
                    Ordering::Equal => latest.push(e),
                    Ordering::Less => {}
                },
            }
        }
        let value_of = |e: &CapabilityEvidence| match e.value {
            Some(CapabilityValue::Boolean(b)) => Some(b),
            Some(CapabilityValue::Unknown) | None => None,
        };
        let freshness = match latest.first() {
            None => None,
            Some(l)
                if compare_instants(&l.observation.observed_at, freshness_cutoff)?
                    == Ordering::Less =>
            {
                Some(Freshness::Stale)
            }
            Some(_) => Some(Freshness::Fresh),
        };
        let outcome = match latest.first() {
            None => CapabilityOutcome::NoUsableEvidence,
            Some(first) => {
                let first_value = value_of(first);
                if latest.iter().any(|e| value_of(e) != first_value) {
                    CapabilityOutcome::Conflicted { evidence: latest }
                } else if let Some(value) = first_value {
                    CapabilityOutcome::Established {
                        value,
                        evidence: latest,
                    }
                } else {
                    CapabilityOutcome::Unknown { evidence: latest }
                }
            }
        };
        Ok(CapabilityResolution {
            outcome,
            freshness,
            rejected,
        })
    }
}
