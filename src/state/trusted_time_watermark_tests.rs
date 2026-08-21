use crate::error::StateError;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;
use crate::trusted_time::{
    TrustedTimeSampleV1, accept_sample, require_current_sample, validate_sample,
};

fn sample(at: &str, source: &str, version: &str) -> TrustedTimeSampleV1 {
    TrustedTimeSampleV1 {
        canonical_utc_timestamp: at.to_string(),
        clock_source_id: source.to_string(),
        clock_contract_version: version.to_string(),
    }
}

fn fence(
    repo: &mut SqliteStateRepository,
    project_id: &str,
    value: TrustedTimeSampleV1,
) -> Result<(), StateError> {
    let (value, timestamp) = validate_sample(value)?;
    repo.run_serialized_transaction(|uow| accept_sample(uow.tx(), project_id, &value, &timestamp))
}

#[test]
fn watermark_initializes_accepts_equal_advances_and_survives_reopen() {
    let tmp = TempDir::new("watermark-basic");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("open");
    let t1 = "2026-08-18T09:30:00.000000000Z";
    let t2 = "2099-12-31T23:59:59.999999999Z";
    fence(&mut repo, "p1", sample(t1, "clock-a", "v1")).expect("initialize");
    fence(&mut repo, "p1", sample(t1, "clock-a", "v1")).expect("equality");
    fence(&mut repo, "p1", sample(t2, "clock-a", "v1")).expect("large jump");
    assert_eq!(
        repo.find_trusted_time_watermark("p1")
            .expect("read")
            .expect("present")
            .last_accepted_trusted_time,
        t2
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_trusted_time_watermark("p1")
            .expect("read")
            .expect("present")
            .last_accepted_trusted_time,
        t2
    );
}

#[test]
fn watermark_rejects_regression_and_source_or_version_discontinuity_per_project() {
    let tmp = TempDir::new("watermark-fences");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("open");
    let t1 = "2026-08-18T09:30:00.000000000Z";
    fence(&mut repo, "p1", sample(t1, "clock-a", "v1")).expect("p1");
    fence(
        &mut repo,
        "p2",
        sample("2000-01-01T00:00:00.000000000Z", "clock-b", "v7"),
    )
    .expect("independent p2");
    assert!(matches!(
        fence(
            &mut repo,
            "p1",
            sample("2026-08-18T09:29:59.999999999Z", "clock-a", "v1")
        ),
        Err(StateError::TrustedClockRegression { .. })
    ));
    for changed in [sample(t1, "clock-b", "v1"), sample(t1, "clock-a", "v2")] {
        assert!(matches!(
            fence(&mut repo, "p1", changed),
            Err(StateError::TrustedClockContinuityUnbound { .. })
        ));
    }
}

#[test]
fn malformed_samples_and_watermarks_fail_closed_and_stale_fences_are_rejected() {
    let tmp = TempDir::new("watermark-invalid");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("open");
    assert!(fence(&mut repo, "p1", sample("not-time", "clock-a", "v1")).is_err());
    assert!(matches!(
        fence(
            &mut repo,
            "p1",
            sample("2026-08-18T09:30:00.000000000Z", "", "v1")
        ),
        Err(StateError::TrustedClockSampleInvalid { .. })
    ));
    assert!(matches!(
        fence(
            &mut repo,
            "p1",
            sample("2026-08-18T09:30:00.000000000Z", "clock-a", "")
        ),
        Err(StateError::TrustedClockSampleInvalid { .. })
    ));
    let t1 = sample("2026-08-18T09:30:00.000000000Z", "clock-a", "v1");
    let t2 = sample("2026-08-18T09:30:00.000000001Z", "clock-a", "v1");
    fence(&mut repo, "p1", t1.clone()).expect("t1");
    fence(&mut repo, "p1", t2).expect("t2");
    let (_, parsed_t1) = validate_sample(t1.clone()).expect("parse t1");
    assert!(matches!(
        repo.run_serialized_transaction(|uow| {
            require_current_sample(uow.tx(), "p1", &t1, &parsed_t1)
        }),
        Err(StateError::TrustedTimeSampleStale { .. })
    ));
    repo.run_transaction(|uow| {
        uow.execute(
            "UPDATE trusted_time_watermark SET last_accepted_trusted_time = ?1 WHERE project_id = ?2",
            &[&"malformed", &"p1"],
        )?;
        Ok(())
    })
    .expect("corrupt for decode probe");
    assert!(matches!(
        repo.find_trusted_time_watermark("p1"),
        Err(StateError::TrustedTimeWatermarkDecodeFailed { .. })
    ));
}
