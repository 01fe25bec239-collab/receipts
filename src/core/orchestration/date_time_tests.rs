use super::{OrchestrationDateTimeError, OrchestrationDateTimeV1};
use std::collections::HashSet;

#[test]
fn mandatory_valid_examples() {
    for input in [
        "1985-04-12T23:20:50.52Z",
        "1996-12-19T16:39:57-08:00",
        "1937-01-01T12:00:27.87+00:20",
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-00:00",
        "2024-02-29T23:59:59Z",
        "0000-01-01T00:00:00Z",
        "1990-12-31T23:59:60Z",
        "2026-09-08T00:00:00.123456789123456789Z",
        "2026-09-08T00:00:00+23:59",
        "2026-09-08T00:00:00-23:59",
        "9999-12-31T23:59:60.0z",
        // Lexical 60 is valid even without a historically announced leap second.
        "2026-09-08T12:34:60+05:30",
    ] {
        assert_eq!(
            OrchestrationDateTimeV1::try_new(input).unwrap().as_str(),
            input
        );
    }
}

#[test]
fn mandatory_invalid_examples() {
    for input in [
        "",
        " 2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z ",
        "2026-09-08 00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:60:00Z",
        "2026-09-08T00:00:61Z",
        "2026-13-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-09-00T00:00:00Z",
        "2026-09-32T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-09-08T00:00:00.Z",
        "2026-09-08T00:00:00.z",
        "2026-09-08T00:00:00.+00:00",
        "2026-09-08T00:00:00,1Z",
        "2026-09-08T00:00:00.abZ",
        "2026-09-08T00:00:00+0000",
        "2026-09-08T00:00:00-0800",
        "2026-09-08T00:00:00+24:00",
        "2026-09-08T00:00:00+00:60",
        "2026-09-08T00:00:00+1:00",
        "2026-09-08T00:00:00+01:0",
        "26-09-08T00:00:00Z",
        "026-09-08T00:00:00Z",
        "02026-09-08T00:00:00Z",
        "+2026-09-08T00:00:00Z",
        "-2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00",
        "2026-9-08T00:00:00Z",
        "2026-09-8T00:00:00Z",
        "2026-09-08T0:00:00Z",
        "2026-09-08T00:0:00Z",
        "2026-09-08T00:00:0Z",
        "2026-09-08T00:00:00.",
        "2026-09-08\n00:00:00Z",
        "2026-09-08\t00:00:00Z",
        "2026-09-08T00:00:\t00Z",
        "2026-09-08T00:00:00Z\n",
        "2026-09-08T00:00:00Z\0",
        "2026/09/08T00:00:00Z",
        "2026-09-08X00:00:00Z",
        "2026-09-08T00:00:00ZZ",
        "2026-09-08T00:00:00.1.2Z",
        "2026-09-08T00:00:00.1",
        "2026-09-08T00:00:00+aa:00",
        "2026-09-08T00:00:00+00:aa",
    ] {
        assert_eq!(
            OrchestrationDateTimeV1::try_new(input),
            Err(OrchestrationDateTimeError),
            "{input:?}"
        );
    }
}

#[test]
fn calendar_boundaries() {
    for (date, accepted) in [
        ("2000-02-29", true),
        ("1900-02-29", false),
        ("2004-02-29", true),
        ("2100-02-29", false),
        ("2026-01-31", true),
        ("2026-04-31", false),
        ("2026-06-30", true),
        ("2026-06-31", false),
        ("0000-02-29", true),
        ("0001-02-29", false),
        ("2026-02-28", true),
        ("2024-02-30", false),
    ] {
        assert_eq!(
            OrchestrationDateTimeV1::try_new(format!("{date}T00:00:00Z")).is_ok(),
            accepted,
            "{date}"
        );
    }
    for (month, last_day) in [
        (1, 31),
        (2, 28),
        (3, 31),
        (4, 30),
        (5, 31),
        (6, 30),
        (7, 31),
        (8, 31),
        (9, 30),
        (10, 31),
        (11, 30),
        (12, 31),
    ] {
        for day in 0..=32 {
            let input = format!("2026-{month:02}-{day:02}T00:00:00Z");
            assert_eq!(
                OrchestrationDateTimeV1::try_new(input.clone()).is_ok(),
                (1..=last_day).contains(&day),
                "{input}"
            );
        }
    }
}

#[test]
fn representations_remain_exact_and_lexically_distinct() {
    let inputs = [
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00Z",
        "2026-09-08T00:00:00z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.1Z",
        "2026-09-08T00:00:00.100000000Z",
        "2026-09-08t00:00:00.100z",
        "2026-09-08T00:00:00.123456789123456789Z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-08:00",
    ];
    let values: Vec<_> = inputs
        .iter()
        .map(|input| {
            let value = OrchestrationDateTimeV1::try_new(*input).unwrap();
            assert_eq!(value.as_str().as_bytes(), input.as_bytes());
            assert_eq!(
                value,
                OrchestrationDateTimeV1::try_new(input.to_string()).unwrap()
            );
            value
        })
        .collect();
    for (index, value) in values.iter().enumerate() {
        assert_eq!(value.clone(), *value);
        for other in &values[index + 1..] {
            assert_ne!(value, other);
        }
    }
    let mut set: HashSet<_> = values.iter().cloned().collect();
    assert_eq!(set.len(), inputs.len());
    for value in values {
        assert!(!set.insert(value));
    }
}

#[test]
fn fractional_precision_has_no_artificial_limit() {
    for length in [1, 3, 9, 18, 10_000] {
        let input = format!("2026-09-08T00:00:00.{}-00:00", "0".repeat(length));
        let value = OrchestrationDateTimeV1::try_new(input.clone()).unwrap();
        assert_eq!(value.as_str(), input);
    }
}

#[test]
fn non_ascii_and_truncated_inputs_reject_without_panicking() {
    let valid = "2026-09-08T00:00:00.123+05:30";
    for lookalike in ["２", "٢", "−", "：", "🦀", "é", "\u{0301}"] {
        // Insert and replace at every position, including structural boundaries.
        for index in 0..=valid.len() {
            let mut input = valid.to_owned();
            input.insert_str(index, lookalike);
            assert_eq!(
                OrchestrationDateTimeV1::try_new(input),
                Err(OrchestrationDateTimeError)
            );
            if index < valid.len() {
                let mut input = valid.to_owned();
                input.replace_range(index..index + 1, lookalike);
                assert_eq!(
                    OrchestrationDateTimeV1::try_new(input),
                    Err(OrchestrationDateTimeError)
                );
            }
        }
    }
    for end in 0..valid.len() {
        assert_eq!(
            OrchestrationDateTimeV1::try_new(&valid[..end]),
            Err(OrchestrationDateTimeError)
        );
    }
}
