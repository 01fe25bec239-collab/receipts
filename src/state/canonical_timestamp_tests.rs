use crate::{CanonicalTimestampV1, StateError};

fn parsed(value: &str) -> CanonicalTimestampV1 {
    CanonicalTimestampV1::parse(value).expect(value)
}

#[test]
fn canonical_timestamp_accepts_calendar_boundaries_and_orders_semantically() {
    for value in [
        "0000-01-01T00:00:00.000000000Z",
        "2000-02-29T23:59:59.999999999Z",
        "2024-02-29T12:00:00.000000000Z",
        "9999-12-31T23:59:59.999999999Z",
    ] {
        assert_eq!(parsed(value).as_str(), value);
    }
    let ordered = [
        "2025-12-31T23:59:59.999999998Z",
        "2025-12-31T23:59:59.999999999Z",
        "2026-01-01T00:00:00.000000000Z",
        "2026-01-01T00:00:01.000000000Z",
        "2026-01-01T00:01:00.000000000Z",
        "2026-01-01T01:00:00.000000000Z",
        "2026-01-02T00:00:00.000000000Z",
        "2026-02-01T00:00:00.000000000Z",
        "2027-01-01T00:00:00.000000000Z",
    ];
    for pair in ordered.windows(2) {
        assert!(parsed(pair[0]) < parsed(pair[1]), "{pair:?}");
    }
}

#[test]
fn canonical_timestamp_rejects_invalid_calendar_and_time_values() {
    for value in [
        "2023-02-29T00:00:00.000000000Z",
        "2026-00-01T00:00:00.000000000Z",
        "2026-13-01T00:00:00.000000000Z",
        "2026-04-31T00:00:00.000000000Z",
        "2026-01-00T00:00:00.000000000Z",
        "2026-01-01T24:00:00.000000000Z",
        "2026-01-01T00:60:00.000000000Z",
        "2026-01-01T00:00:60.000000000Z",
    ] {
        assert!(matches!(
            CanonicalTimestampV1::parse(value),
            Err(StateError::CanonicalTimestampInvalid { .. })
        ));
    }
}

#[test]
fn canonical_timestamp_rejects_every_alternate_representation() {
    for value in [
        "2026-08-18T09:30:00.00000000Z",
        "2026-08-18T09:30:00.0000000000Z",
        "2026-08-18T09:30:00.000000000z",
        "2026-08-18T09:30:00.000000000+00:00",
        "2026-08-18T09:30:00.000000000-00:00",
        "2026-08-18T09:30:00.000000000+05:30",
        "2026-08-18T09:30:00.000000000",
        " 2026-08-18T09:30:00.000000000Z",
        "2026-08-18T09:30:00.000000000Z ",
        "2026/08/18T09:30:00.000000000Z",
        "20260818T093000.000000000Z",
        "２０２６-08-18T09:30:00.000000000Z",
    ] {
        assert!(CanonicalTimestampV1::parse(value).is_err(), "{value:?}");
    }
}
