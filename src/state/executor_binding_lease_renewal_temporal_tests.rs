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

fn watermark(
    repo: &crate::repository::SqliteStateRepository,
) -> Option<crate::TrustedTimeWatermarkV1> {
    repo.find_trusted_time_watermark("project-lease")
        .expect("watermark")
}

#[test]
fn refused_renewal_leaves_watermark_unchanged() {
    const EARLY: &str = "2026-08-18T09:00:00.000000000Z";
    // (case, stored deadline, prior fence?, sample, new deadline)
    let cases: Vec<(&str, &str, bool, TrustedTimeSampleV1, &str)> = vec![
        ("at-deadline", DEADLINE, true, clock(DEADLINE).0, RENEWED),
        ("after-deadline", DEADLINE, true, clock(AFTER).0, RENEWED),
        ("not-later", DEADLINE, true, clock(BEFORE).0, DEADLINE),
        ("earlier", DEADLINE, true, clock(BEFORE).0, EARLY),
        (
            "noncanonical-new",
            DEADLINE,
            true,
            clock(BEFORE).0,
            "2026-08-18T11:00:00Z",
        ),
        (
            "noncanonical-stored",
            "2026-08-18T10:00:00Z",
            true,
            clock(BEFORE).0,
            RENEWED,
        ),
        (
            "first-fence-refused",
            DEADLINE,
            false,
            clock(AFTER).0,
            RENEWED,
        ),
        (
            "discontinuous",
            DEADLINE,
            true,
            TrustedTimeSampleV1 {
                canonical_utc_timestamp: BEFORE.to_string(),
                clock_source_id: "other-source".to_string(),
                clock_contract_version: "v1".to_string(),
            },
            RENEWED,
        ),
    ];
    for (tag, stored, prior, sample, new_deadline) in cases {
        let (_tmp, mut repo, _) = seeded(tag, stored);
        if prior {
            fence_binding_time(&mut repo, &clock(EARLY), BINDING).expect("prior fence");
        }
        let before = (
            watermark(&repo),
            repo.find_executor_binding(BINDING).unwrap(),
        );
        assert!(
            repo.renew_executor_binding_lease(&FakeTrustedClock(sample), BINDING, new_deadline)
                .is_err(),
            "{tag}"
        );
        assert_eq!(
            (
                watermark(&repo),
                repo.find_executor_binding(BINDING).unwrap()
            ),
            before,
            "{tag}"
        );
    }
    // Released binding: a later sample must not advance the fence either.
    let (_tmp, mut repo, _) = seeded("released", DEADLINE);
    fence_binding_time(&mut repo, &clock(EARLY), BINDING).unwrap();
    repo.release_executor_binding(BINDING, EARLY, ReleaseReason::UserRequest)
        .unwrap();
    let before = watermark(&repo);
    assert!(matches!(
        repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED),
        Err(StateError::ExecutorBindingAlreadyReleased { .. })
    ));
    assert_eq!(watermark(&repo), before);
    // A successful renewal commits lease and accepted time together.
    let (_tmp, mut repo, _) = seeded("accepted", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .unwrap();
    assert_eq!(watermark(&repo).unwrap().last_accepted_trusted_time, BEFORE);
    assert_eq!(
        repo.find_executor_binding(BINDING)
            .unwrap()
            .unwrap()
            .lease_expires_at,
        RENEWED
    );
}

#[test]
fn noncanonical_lease_timestamp_refused() {
    for (tag, stored, new) in [
        ("nc-new", DEADLINE, "2026-08-18T11:00:00.000Z"),
        (
            "nc-new-offset",
            DEADLINE,
            "2026-08-18T11:00:00.000000000+00:00",
        ),
        ("nc-stored", "2026-08-18T10:00:00.000Z", RENEWED),
    ] {
        let (_tmp, mut repo, _) = seeded(tag, stored);
        assert!(matches!(
            repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, new),
            Err(StateError::CanonicalTimestampInvalid { .. })
        ));
        // The unsupported stored deadline is not rewritten.
        assert_eq!(
            repo.find_executor_binding(BINDING)
                .unwrap()
                .unwrap()
                .lease_expires_at,
            stored
        );
    }
}

#[test]
fn renewal_older_than_watermark_refused() {
    let (_tmp, mut repo, _) = seeded("renew-older", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .unwrap();
    let before = watermark(&repo);
    assert!(matches!(
        repo.renew_executor_binding_lease(
            &clock("2026-08-18T09:59:59.999999998Z"),
            BINDING,
            "2026-08-18T12:00:00.000000000Z"
        ),
        Err(StateError::TrustedClockRegression { .. })
    ));
    assert_eq!(watermark(&repo), before);
    assert_eq!(
        repo.find_executor_binding(BINDING)
            .unwrap()
            .unwrap()
            .lease_expires_at,
        RENEWED
    );
}

#[test]
fn v10_store_open_reports_unsupported_legacy_schema() {
    use crate::error::{GraphFailureCode, GraphPhase};
    for version in 1..=10 {
        let tmp = crate::tests::TempDir::new("legacy-v1-v10");
        drop(
            crate::repository::SqliteStateRepository::open_with_migrations(
                tmp.db_path(),
                &crate::migrations::registered()[..version],
            )
            .unwrap(),
        );
        let bytes = std::fs::read(tmp.db_path()).unwrap();
        assert!(matches!(
            crate::repository::SqliteStateRepository::open(tmp.db_path()),
            Err(StateError::SchemaVersionMismatch { found, supported: 22 }) if found as usize == version
        ));
        assert!(matches!(
            crate::repository::SqliteStateRepository::migrate_existing_to_current(tmp.db_path()),
            Err(StateError::GraphPersistence {
                phase: GraphPhase::Migration,
                code: GraphFailureCode::UnsupportedSchema,
                field: None
            })
        ));
        assert!(crate::StateReader::open_existing(tmp.db_path()).is_err());
        assert_eq!(
            std::fs::read(tmp.db_path()).unwrap(),
            bytes,
            "v{version} not rewritten"
        );
    }
}
