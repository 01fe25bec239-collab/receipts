use crate::{
    AssuranceProfile, ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleReviewScope,
    ReviewDateTimeV1, ReviewRequest, ReviewRequestAttemptNumber, ReviewRequestConstructionError,
    ReviewRequestNonNegativeInteger, ReviewRequestNonTemporalCore,
};

fn core(epoch: &str, attempt: Option<ReviewRequestAttemptNumber>) -> ReviewRequestNonTemporalCore {
    ReviewRequestNonTemporalCore::new(
        "request".into(),
        "task".into(),
        "attempt".into(),
        "a".repeat(40),
        "b".repeat(40),
        "objective".into(),
        vec![
            ReviewCapsuleCriterion::new(
                "criterion".into(),
                "description".into(),
                ReviewCapsuleCriterionKind::Deterministic,
                None,
                None,
            )
            .unwrap(),
        ],
        vec!["src/review/**".into()],
        AssuranceProfile::Light,
        ReviewCapsuleReviewScope::Full,
        ReviewRequestNonNegativeInteger::from_decimal(epoch).unwrap(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        attempt,
    )
    .unwrap()
}

#[test]
fn absence_is_valid_and_remains_distinct_from_presence_without_defaults() {
    let supplied_core = core("0", None);
    assert_eq!(supplied_core.request_id(), "request");
    let absent = ReviewRequest::new(supplied_core.clone(), None);
    let timestamp = ReviewDateTimeV1::try_new("2026-09-08T00:00:00Z").unwrap();
    let present = ReviewRequest::new(supplied_core.clone(), Some(timestamp.clone()));
    let _: &ReviewRequestNonTemporalCore = absent.core();
    let _: Option<&ReviewDateTimeV1> = present.requested_at();
    assert_eq!(absent.core(), &supplied_core);
    assert_eq!(present.core(), &supplied_core);
    assert_eq!(absent.requested_at(), None);
    assert_eq!(absent.clone().requested_at(), None);
    assert_eq!(present.requested_at(), Some(&timestamp));
    assert_ne!(absent.requested_at(), present.requested_at());
}

#[test]
fn present_values_preserve_every_accepted_lexical_distinction() {
    let long = format!("2026-09-08T00:00:00.{}Z", "1234567890".repeat(10_000));
    for input in [
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-08:00",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.123456789123456789Z",
        "2026-09-08T00:00:00.100000000Z",
        &long,
    ] {
        let timestamp = ReviewDateTimeV1::try_new(input).unwrap();
        let value = ReviewRequest::new(core("0", None), Some(timestamp));
        assert_eq!(value.requested_at().unwrap().as_str(), input);
        assert_eq!(value.clone().requested_at().unwrap().as_str(), input);
    }
}

#[test]
fn invalid_datetime_fails_before_composition() {
    for input in [
        "",
        "null",
        "invalid",
        "2026-02-30T00:00:00Z",
        "2026-09-08T00:00:00",
    ] {
        let result = ReviewDateTimeV1::try_new(input)
            .map(|timestamp| ReviewRequest::new(core("0", None), Some(timestamp)));
        assert!(result.is_err(), "accepted invalid timestamp: {input}");
    }
}

#[test]
fn composition_preserves_full_integer_domains() {
    for digits in [
        "9223372036854775808".to_owned(),
        "18446744073709551616".to_owned(),
        "9".repeat(10_000),
    ] {
        let attempt = ReviewRequestAttemptNumber::from_decimal(&digits).unwrap();
        let supplied_core = core(&digits, Some(attempt));
        for timestamp in [
            None,
            Some(ReviewDateTimeV1::try_new("2026-09-08T00:00:00Z").unwrap()),
        ] {
            let value = ReviewRequest::new(supplied_core.clone(), timestamp);
            assert_eq!(value.core(), &supplied_core);
            assert_eq!(value.core().context_epoch().decimal_digits(), digits);
            assert_eq!(
                value.core().attempt_number().unwrap().decimal_digits(),
                digits
            );
        }
    }
    assert_eq!(
        ReviewRequestAttemptNumber::from_decimal("0"),
        Err(ReviewRequestConstructionError::ZeroAttemptNumber)
    );
}
