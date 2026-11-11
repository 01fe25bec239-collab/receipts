//! N6 side-state exits under D5.
//!
//! A model in DEPRECATED, DISABLED, UNAVAILABLE or USER_BLOCKED may leave that
//! state only on a trusted post-demotion receipt in State's durable D5 order.
//! The evidence offered is the State sequence of an availability fact; it is
//! checked against State's own lifecycle for the exact partition:
//! no durable demotion, a sequence at or before the last demotion
//! (pre-demotion proof), a sequence State never assigned (forged or not
//! durable), and any later fact State still classifies as `Demoted`
//! (unauthenticated claims, retry-after expiry) all refuse.
//!
//! No authenticated post-demotion producer exists (adopted D5 deferral, after
//! W3), so State never reports recovery and no exit can succeed: the success
//! type is [`Infallible`]. When State gains a recovered status, the
//! exhaustive match below stops compiling and the real producer task must
//! wire the exit edge here.

use std::convert::Infallible;

use super::LifecycleState;
use crate::registry::Registry;
use receipts_state::{AvailabilityLifecycleStatusV1, AvailabilityLifecycleV1};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleExitRefusal {
    ModelNotFound,
    /// Only side states have exit edges; primary states use normal promotion.
    NotInSideState(LifecycleState),
    /// No receipt sequence was offered.
    MissingEvidence,
    /// State holds no demotion for this partition, so nothing orders the proof.
    NoDurableDemotion,
    /// The evidence predates (or is) the last durable demotion.
    PreDemotionEvidence {
        evidence_sequence: u64,
        demotion_sequence: u64,
    },
    /// State never assigned this sequence: forged or not durable.
    EvidenceNotDurable {
        evidence_sequence: u64,
        latest_sequence: u64,
    },
    /// Durable and after the demotion, but State still derives `Demoted`:
    /// an unauthenticated claim or a retry-after expiry is not a receipt.
    UnauthenticatedRecovery {
        evidence_sequence: u64,
    },
}

/// Assesses one side-state exit. `state_lifecycle` is State's current
/// lifecycle for the partition the demotion was recorded in.
pub fn assess_side_state_exit(
    registry: &Registry,
    provider: &str,
    model: &str,
    state_lifecycle: &AvailabilityLifecycleV1,
    evidence_sequence: Option<u64>,
) -> Result<Infallible, LifecycleExitRefusal> {
    use LifecycleExitRefusal::*;
    let record = registry.model(provider, model).ok_or(ModelNotFound)?;
    if !record.lifecycle_state().is_side_state() {
        return Err(NotInSideState(record.lifecycle_state()));
    }
    let evidence_sequence = evidence_sequence.ok_or(MissingEvidence)?;
    match state_lifecycle.status {
        AvailabilityLifecycleStatusV1::Unknown => Err(NoDurableDemotion),
        AvailabilityLifecycleStatusV1::Demoted => {
            let demotion_sequence = state_lifecycle
                .last_demotion_sequence
                .ok_or(NoDurableDemotion)?;
            if evidence_sequence <= demotion_sequence {
                Err(PreDemotionEvidence {
                    evidence_sequence,
                    demotion_sequence,
                })
            } else if evidence_sequence > state_lifecycle.latest_sequence {
                Err(EvidenceNotDurable {
                    evidence_sequence,
                    latest_sequence: state_lifecycle.latest_sequence,
                })
            } else {
                Err(UnauthenticatedRecovery { evidence_sequence })
            }
        }
    }
}
