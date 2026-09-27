use std::cmp::Ordering;
use std::collections::HashSet;

use super::*;

fn timestamp(value: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(value.into()).unwrap()
}

fn compare(left: &str, right: &str) -> Result<Ordering, ModelRoutingDateTimeComparisonError> {
    compare_instants(&timestamp(left), &timestamp(right))
}

/// Ordinary-second values with hand-derived UTC ranks; equal rank means the
/// same instant. The UTC instant each rank denotes is noted beside it.
const RANKED: &[(u32, &str)] = &[
    (0, "0000-01-01T00:00:00+23:59"), // -0001-12-31T00:01Z, before local range
    (1, "0000-01-01T00:00:00+23:00"), // -0001-12-31T01:00Z
    (1, "0000-01-01T00:59:00+23:59"),
    (2, "0000-01-01T00:00:00Z"),
    (2, "0000-01-01t00:00:00.000z"),
    (2, "0000-01-01T01:00:00+01:00"),
    (2, "0000-01-01T00:00:00-00:00"),
    (3, "0000-02-29T01:00:00Z"), // year 0000 is a leap year
    (3, "0000-02-28T23:00:00-02:00"),
    (4, "0000-03-01T01:00:00Z"),
    (4, "0000-02-29T23:00:00-02:00"),
    (5, "1900-03-01T01:00:00Z"), // century, not leap
    (5, "1900-02-28T23:00:00-02:00"),
    (6, "2000-01-01T01:00:00Z"),
    (6, "1999-12-31T23:00:00-02:00"),
    (7, "2000-02-29T01:00:00Z"), // 400-year century, leap
    (7, "2000-02-28T23:00:00-02:00"),
    (8, "2000-03-01T01:00:00Z"),
    (8, "2000-02-29T23:00:00-02:00"),
    (9, "2026-09-08T11:00:00Z"),
    (9, "2026-09-08T12:00:00+01:00"),
    (10, "2026-09-08T12:00:00Z"),
    (10, "2026-09-08T12:00:00.000-00:00"),
    (10, "2026-09-08T12:00:00+00:00"),
    (10, "2026-09-08t12:00:00z"),
    (10, "2026-09-08T17:30:00+05:30"),
    (10, "2026-09-08T06:30:00-05:30"),
    (11, "2026-09-08T12:00:00.000000001Z"),
    (12, "2026-09-08T12:00:00.0000000011Z"), // below one nanosecond
    (12, "2026-09-08T12:00:00.00000000110-00:00"),
    (12, "2026-09-08T13:00:00.0000000011+01:00"),
    (13, "2026-09-08T12:00:00.5Z"),
    (13, "2026-09-08T12:00:00.50000+00:00"),
    (14, "2026-09-08T12:00:00.51Z"),
    (15, "2026-09-08T12:00:00.6Z"),
    (16, "2026-09-08T12:00:00.999999999999Z"),
    (17, "2026-09-08T12:00:01Z"),
    (17, "2026-09-08T11:00:01-01:00"),
    (18, "2026-09-08T12:00:00-01:00"), // 13:00Z; lexically before rank 17
    (19, "2026-09-08T00:00:00-23:59"), // 23:59Z, both extreme offsets
    (19, "2026-09-09T23:58:00+23:59"),
    (20, "2027-01-01T00:30:00Z"),
    (20, "2026-12-31T23:30:00-01:00"),
    (21, "2100-03-01T01:00:00Z"), // century, not leap
    (21, "2100-02-28T23:00:00-02:00"),
    (22, "9999-12-31T23:59:59Z"),
    (23, "9999-12-31T23:59:59.999+00:00"),
    (24, "9999-12-31T23:00:00-23:00"), // 10000-01-01T22:00Z, after local range
    (24, "9999-12-31T23:01:00-22:59"),
    (25, "9999-12-31T23:59:59-23:59"), // 10000-01-01T23:58:59Z
    (26, "9999-12-31T23:59:59.9-23:59"),
];

#[test]
fn every_pair_orders_by_independently_ranked_utc_instant() {
    for (left_rank, left) in RANKED {
        for (right_rank, right) in RANKED {
            assert_eq!(
                compare(left, right),
                Ok(left_rank.cmp(right_rank)),
                "{left} vs {right}"
            );
        }
    }
}

#[test]
fn instant_order_is_not_lexical_order() {
    let (earlier, later) = ("2026-09-08T12:00:01Z", "2026-09-08T12:00:00-01:00");
    assert!(earlier > later);
    assert_eq!(compare(earlier, later), Ok(Ordering::Less));
    assert_eq!(compare(later, earlier), Ok(Ordering::Greater));
}

#[test]
fn negative_zero_offset_is_utc_instant_but_keeps_lexical_identity() {
    let unknown_local = timestamp("2026-09-08T12:00:00-00:00");
    let utc = timestamp("2026-09-08T12:00:00Z");
    let plus_zero = timestamp("2026-09-08T12:00:00+00:00");
    assert_eq!(compare_instants(&unknown_local, &utc), Ok(Ordering::Equal));
    assert_eq!(
        compare_instants(&unknown_local, &plus_zero),
        Ok(Ordering::Equal)
    );

    assert_ne!(unknown_local, utc);
    assert_ne!(unknown_local, plus_zero);
    assert_eq!(unknown_local.as_str(), "2026-09-08T12:00:00-00:00");
    let set: HashSet<_> = [unknown_local.clone(), utc, plus_zero]
        .into_iter()
        .collect();
    assert_eq!(set.len(), 3);
    assert!(set.contains(&unknown_local));
}

#[test]
fn fractions_compare_exactly_beyond_ten_thousand_digits() {
    let at = |fraction: &str| format!("2026-09-08T12:00:00.{fraction}Z");
    let zeros = "0".repeat(10_000);
    let tiny = at(&format!("{zeros}1"));
    let tiny_padded = at(&format!("{zeros}1{zeros}"));
    let tiny_plus = at(&format!("{zeros}1{zeros}1"));
    let tiny_double = at(&format!("{zeros}2"));
    let zero = at(&format!("{zeros}0"));
    let (tiny, zero) = (tiny.as_str(), zero.as_str());
    for (left, right, expected) in [
        (tiny, tiny_padded.as_str(), Ordering::Equal),
        (tiny, tiny_plus.as_str(), Ordering::Less),
        (tiny_plus.as_str(), tiny_double.as_str(), Ordering::Less),
        (zero, tiny, Ordering::Less),
        (zero, "2026-09-08T12:00:00Z", Ordering::Equal),
        (zero, "2026-09-08T13:00:00+01:00", Ordering::Equal),
        (tiny, "2026-09-08T12:00:00.000000001Z", Ordering::Less),
    ] {
        assert_eq!(compare(left, right), Ok(expected));
        assert_eq!(compare(right, left), Ok(expected.reverse()));
    }
    let almost_next = at(&"9".repeat(20_000));
    assert_eq!(
        compare(&almost_next, "2026-09-08T12:00:01Z"),
        Ok(Ordering::Less)
    );
}

/// Walks every accepted calendar day with its own month table and leap rule,
/// independent of the comparator, and checks that each local midnight at
/// 23:59 at `-00:01` equals the next day's 00:00Z and follows the previous day.
#[test]
fn every_calendar_day_follows_its_predecessor() {
    let (mut year, mut month, mut day) = (0_u32, 1_u32, 1_u32);
    let mut previous = timestamp("0000-01-01T00:00:00+23:59");
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let month_days = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let (next_year, next_month, next_day) = if day < month_days[month as usize - 1] {
            (year, month, day + 1)
        } else if month < 12 {
            (year, month + 1, 1)
        } else {
            (year + 1, 1, 1)
        };
        let current = timestamp(&format!("{year:04}-{month:02}-{day:02}T23:59:00-00:01"));
        assert_eq!(compare_instants(&previous, &current), Ok(Ordering::Less));
        if next_year == 10_000 {
            break;
        }
        let next = timestamp(&format!(
            "{next_year:04}-{next_month:02}-{next_day:02}T00:00:00Z"
        ));
        assert_eq!(compare_instants(&current, &next), Ok(Ordering::Equal));
        (year, month, day, previous) = (next_year, next_month, next_day, current);
    }
    assert_eq!((year, month, day), (9_999, 12, 31));
}

#[test]
fn second_sixty_is_refused_for_either_or_both_operands() {
    let refused = Err(ModelRoutingDateTimeComparisonError::UnsupportedLeapSecond);
    let leap = "1990-12-31T23:59:60Z";
    let shifted_leap = "1990-12-31T15:59:60.5-08:00";
    let lexical_only = "2026-09-08T12:34:60z";
    let ordinary = "2026-09-08T12:00:00Z";
    for other in [ordinary, "1990-12-31T23:59:59Z", "1991-01-01T00:00:00Z"] {
        assert_eq!(compare(leap, other), refused);
        assert_eq!(compare(other, leap), refused);
    }
    for value in [
        leap,
        shifted_leap,
        lexical_only,
        "9999-12-31T23:59:60-23:59",
    ] {
        assert_eq!(compare(value, value), refused, "{value}");
        let same = timestamp(value);
        assert_eq!(compare_instants(&same, &same), refused);
    }
    assert_eq!(compare(leap, shifted_leap), refused);
    assert_eq!(compare(shifted_leap, lexical_only), refused);
    assert_eq!(timestamp(leap).as_str(), leap);
}
