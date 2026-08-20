use crate::error::StateError;
use crate::executor_binding::ReleaseReason;
use crate::executor_binding_lease_expiry::{
    ExecutorLeaseExpiryOutcomeV1, apply_renewal_with_fenced_time, fence_binding_time,
};
use crate::executor_binding_lease_expiry_tests::{BINDING, DEADLINE, clock, request, seeded};
use crate::tests::FakeTrustedClock;
use crate::trusted_time::TrustedTimeSampleV1;

const BEFORE: &str = "2026-08-18T09:59:59.999999999Z";
const AFTER: &str = "2026-08-18T10:00:00.000000001Z";
const RENEWED: &str = "2026-08-18T11:00:00.000000000Z";

#[test]
fn renewal_before_deadline_changes_only_deadline_and_preserves_bound_identity() {
    let (_tmp, mut repo, original) = seeded("renew-before", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .expect("eligible renewal");
    let renewed = repo
        .find_executor_binding(BINDING)
        .expect("find")
        .expect("present");
    assert_eq!(renewed.lease_expires_at, RENEWED);
    assert_eq!(renewed.provider_id, original.provider_id);
    assert_eq!(renewed.model_id, original.model_id);
    assert_eq!(renewed.runtime_id, original.runtime_id);
    assert_eq!((renewed.released_at, renewed.release_reason), (None, None));
}

#[test]
fn renewal_at_or_after_deadline_is_refused_without_expiring() {
    for (tag, at) in [("renew-equal", DEADLINE), ("renew-after", AFTER)] {
        let (_tmp, mut repo, _) = seeded(tag, DEADLINE);
        assert!(matches!(
            repo.renew_executor_binding_lease(&clock(at), BINDING, RENEWED),
            Err(StateError::ExecutorLeaseRenewalRefused { .. })
        ));
        let binding = repo
            .find_executor_binding(BINDING)
            .expect("find")
            .expect("present");
        assert_eq!(binding.lease_expires_at, DEADLINE);
        assert_eq!((binding.released_at, binding.release_reason), (None, None));
    }
}

#[test]
fn renewal_after_any_release_is_refused_and_never_resurrects() {
    let (_tmp, mut repo, binding) = seeded("renew-after-expiry", DEADLINE);
    assert_eq!(
        repo.expire_executor_binding_lease(
            &clock(DEADLINE),
            request(&binding, DEADLINE, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        )
        .expect("expiry"),
        ExecutorLeaseExpiryOutcomeV1::Released
    );
    assert!(matches!(
        repo.renew_executor_binding_lease(&clock(DEADLINE), BINDING, RENEWED),
        Err(StateError::ExecutorBindingAlreadyReleased { .. })
    ));

    let (_tmp, mut repo, _) = seeded("renew-after-other", DEADLINE);
    repo.release_executor_binding(
        BINDING,
        "2026-08-18T09:30:00.000000000Z",
        ReleaseReason::UserRequest,
    )
    .expect("other release");
    assert!(matches!(
        repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED),
        Err(StateError::ExecutorBindingAlreadyReleased { .. })
    ));
}

#[test]
fn renewal_rejects_regressed_stale_and_discontinuous_trusted_time() {
    let (_tmp, mut repo, _) = seeded("renew-time-fences", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .expect("first fence");
    assert!(matches!(
        repo.renew_executor_binding_lease(
            &clock("2026-08-18T09:00:00.000000000Z"),
            BINDING,
            RENEWED
        ),
        Err(StateError::TrustedClockRegression { .. })
    ));
    for altered in [
        TrustedTimeSampleV1 {
            canonical_utc_timestamp: BEFORE.to_string(),
            clock_source_id: "changed-source".to_string(),
            clock_contract_version: "v1".to_string(),
        },
        TrustedTimeSampleV1 {
            canonical_utc_timestamp: BEFORE.to_string(),
            clock_source_id: "test-clock".to_string(),
            clock_contract_version: "v2".to_string(),
        },
    ] {
        assert!(matches!(
            repo.renew_executor_binding_lease(&FakeTrustedClock(altered), BINDING, RENEWED),
            Err(StateError::TrustedClockContinuityUnbound { .. })
        ));
    }

    let (_tmp, mut repo, _) = seeded("renew-stale", DEADLINE);
    let t1 = fence_binding_time(&mut repo, &clock("2026-08-18T09:00:00.000000000Z"), BINDING)
        .expect("t1 fence");
    fence_binding_time(&mut repo, &clock("2026-08-18T09:00:00.000000001Z"), BINDING)
        .expect("t2 fence");
    assert!(matches!(
        apply_renewal_with_fenced_time(&mut repo, t1, BINDING, RENEWED),
        Err(StateError::TrustedTimeSampleStale { .. })
    ));
}

#[test]
fn serialized_current_state_decides_renewal_vs_expiry() {
    let (_tmp, mut repo, binding) = seeded("renew-wins", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .expect("renewal wins first");
    let later = "2026-08-18T10:30:00.000000000Z";
    assert_eq!(
        repo.expire_executor_binding_lease(
            &clock(later),
            request(&binding, later, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        )
        .expect("expiry rereads renewed deadline"),
        ExecutorLeaseExpiryOutcomeV1::NotExpired
    );

    let (_tmp, mut repo, binding) = seeded("expiry-wins", DEADLINE);
    repo.expire_executor_binding_lease(
        &clock(DEADLINE),
        request(&binding, DEADLINE, "01ARZ3NDEKTSV4RRFFQ69G5FAV"),
    )
    .expect("expiry first");
    assert!(matches!(
        repo.renew_executor_binding_lease(&clock(DEADLINE), BINDING, RENEWED),
        Err(StateError::ExecutorBindingAlreadyReleased { .. })
    ));
}

#[test]
fn malformed_current_deadline_fails_closed_after_preserving_watermark() {
    let (_tmp, mut repo, binding) = seeded("renew-malformed-deadline", "not-a-timestamp");
    assert!(matches!(
        repo.expire_executor_binding_lease(
            &clock(DEADLINE),
            request(&binding, DEADLINE, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        ),
        Err(StateError::CanonicalTimestampInvalid { .. })
    ));
    assert!(
        repo.find_trusted_time_watermark("project-lease")
            .expect("watermark")
            .is_some()
    );
    assert_eq!(
        repo.find_executor_binding(BINDING)
            .expect("find")
            .expect("present")
            .release_reason,
        None
    );
}
