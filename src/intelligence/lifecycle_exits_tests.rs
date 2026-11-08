//! N6 exits against real D5 State ordering. No authenticated producer
//! exists, so every case refuses with its own reason.

use receipts_runtime_adapters::FailureClass;
use receipts_state::{
    AvailabilityFactKindV1 as Kind, AvailabilityFactSourceV1 as Source, AvailabilityFactV1,
    AvailabilityLifecycleStatusV1,
};

use super::lifecycle_exits::{LifecycleExitRefusal as X, assess_side_state_exit};
use super::{DemotionReason, LifecycleEvidence, LifecycleState, LifecycleTransition};
use crate::availability::{AvailabilityManager, AvailabilityObservation, BackoffPolicy};
use crate::persistence::RoutingStatePort;
use crate::registry::persistence_tests::persist_ops;
use crate::test_state::*;
use crate::{AvailabilitySignalSource, EvidenceConfidence};

fn fact(kind: Kind, source: Source, follows: Option<u64>) -> AvailabilityFactV1 {
    AvailabilityFactV1::new(
        PROJECT.into(),
        "prov-a".into(),
        "rt-a".into(),
        "model-a".into(),
        kind,
        "synthetic".into(),
        source,
        "state-query:synthetic".into(),
        follows,
    )
    .unwrap()
}

/// A ROUTABLE model demoted to UNAVAILABLE with a durable D5 demotion
/// (sequence 1) recorded by the availability manager.
fn demoted() -> (CoreHarness, crate::registry::PersistentRegistry) {
    let mut core = CoreHarness::new("lifecycle-exit");
    let mut reg = persist_ops(&mut core, &[Spec::frontier("prov-a", "model-a", "rt-a")]);
    let (p, m, r) = ids("prov-a", "model-a", "rt-a");
    let mut manager = AvailabilityManager::new(BackoffPolicy::try_new(10, 60, 3).unwrap());
    let effect = manager
        .assess(&AvailabilityObservation {
            provider_id: p.clone(),
            model_id: m.clone(),
            runtime_id: r,
            failure: FailureClass::RateLimited,
            retry_after_seconds: Some(30),
            signal_source: AvailabilitySignalSource::RateLimitHeader,
            observed_at: ts(NOW),
            quota: None,
        })
        .unwrap();
    let out = manager
        .persist_and_commit(&mut core, &CoreHarness::scope(), &effect)
        .unwrap();
    assert_eq!(out.demotion.unwrap().sequence, 1);
    reg.try_transition(
        &mut core,
        &CoreHarness::scope(),
        &p,
        &m,
        LifecycleTransition {
            target: LifecycleState::Unavailable,
            evidence: LifecycleEvidence::Demotion(DemotionReason::RateLimited),
            observation: obs(EvidenceConfidence::LocalEmpirical, NOW),
        },
    )
    .unwrap();
    (core, reg)
}

fn assess(core: &CoreHarness, reg: &crate::registry::PersistentRegistry, seq: Option<u64>) -> X {
    let lifecycle = core
        .reader()
        .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-a")
        .unwrap();
    match assess_side_state_exit(reg.registry(), "prov-a", "model-a", &lifecycle, seq) {
        Err(refusal) => refusal,
    }
}

#[test]
fn pre_demotion_proof_cannot_repromote() {
    let (core, reg) = demoted();
    assert_eq!(
        assess(&core, &reg, Some(1)),
        X::PreDemotionEvidence {
            evidence_sequence: 1,
            demotion_sequence: 1
        }
    );
    assert_eq!(
        assess(&core, &reg, Some(0)),
        X::PreDemotionEvidence {
            evidence_sequence: 0,
            demotion_sequence: 1
        }
    );
    assert_eq!(assess(&core, &reg, None), X::MissingEvidence);
    // The model stays in its side state after every refusal.
    assert_eq!(
        reg.registry()
            .model("prov-a", "model-a")
            .unwrap()
            .lifecycle_state(),
        LifecycleState::Unavailable
    );
}

#[test]
fn unavailable_to_unassessed_requires_fresh_receipt() {
    let (mut core, reg) = demoted();
    // A forged sequence State never assigned.
    assert_eq!(
        assess(&core, &reg, Some(9)),
        X::EvidenceNotDurable {
            evidence_sequence: 9,
            latest_sequence: 1
        }
    );
    // A durable post-demotion claim from an unauthenticated source.
    let claim = core
        .append_availability_fact(&fact(
            Kind::PromotionUnauthenticated,
            Source::ProbeReport,
            Some(1),
        ))
        .unwrap();
    assert_eq!(claim.status, AvailabilityLifecycleStatusV1::Demoted);
    assert_eq!(
        assess(&core, &reg, Some(claim.sequence)),
        X::UnauthenticatedRecovery {
            evidence_sequence: 2
        }
    );
    // A stale follows_sequence is refused by State before it is durable.
    assert!(
        core.append_availability_fact(&fact(
            Kind::PromotionUnauthenticated,
            Source::UnauthenticatedClaim,
            Some(1)
        ))
        .is_err()
    );
    // A partition with no durable demotion orders nothing.
    let lifecycle = core
        .reader()
        .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-other")
        .unwrap();
    assert_eq!(
        assess_side_state_exit(reg.registry(), "prov-a", "model-a", &lifecycle, Some(1))
            .unwrap_err(),
        X::NoDurableDemotion
    );
}

#[test]
fn retry_after_alone_is_not_a_receipt() {
    let (mut core, reg) = demoted();
    let elapsed = core
        .append_availability_fact(&fact(
            Kind::RetryAfterElapsed,
            Source::RetryAfterExpiry,
            Some(1),
        ))
        .unwrap();
    assert_eq!(elapsed.status, AvailabilityLifecycleStatusV1::Demoted);
    assert_eq!(
        assess(&core, &reg, Some(elapsed.sequence)),
        X::UnauthenticatedRecovery {
            evidence_sequence: 2
        }
    );
    core.reopen();
    assert_eq!(
        assess(&core, &reg, Some(2)),
        X::UnauthenticatedRecovery {
            evidence_sequence: 2
        },
        "durable order survives restart"
    );
}

#[test]
fn primary_states_have_no_exit_edge() {
    let mut core = CoreHarness::new("lifecycle-primary");
    let reg = persist_ops(&mut core, &[Spec::frontier("prov-a", "model-a", "rt-a")]);
    let lifecycle = core
        .reader()
        .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-a")
        .unwrap();
    assert_eq!(
        assess_side_state_exit(reg.registry(), "prov-a", "model-a", &lifecycle, Some(1))
            .unwrap_err(),
        X::NotInSideState(LifecycleState::Routable)
    );
    assert_eq!(
        assess_side_state_exit(reg.registry(), "prov-a", "missing", &lifecycle, Some(1))
            .unwrap_err(),
        X::ModelNotFound
    );
}
