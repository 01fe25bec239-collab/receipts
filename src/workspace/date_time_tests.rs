use crate::{WorkspaceDateTimeError, WorkspaceDateTimeV1};
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

#[test]
fn accepts_authorized_grammar_and_preserves_bytes() {
    for value in [
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
        "9999-12-31t23:59:60.0z",
    ] {
        assert_eq!(WorkspaceDateTimeV1::try_new(value).unwrap().as_str(), value);
    }
}

#[test]
fn validates_gregorian_month_lengths_and_century_boundaries() {
    for (year, february_days) in [
        (2000, 29),
        (1900, 28),
        (2004, 29),
        (2100, 28),
        (0, 29),
        (1, 28),
    ] {
        for (month, days) in [
            (1, 31),
            (2, february_days),
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
                let value = format!("{year:04}-{month:02}-{day:02}T00:00:00Z");
                assert_eq!(
                    WorkspaceDateTimeV1::try_new(&value).is_ok(),
                    (1..=days).contains(&day),
                    "{value}"
                );
            }
        }
    }
}

#[test]
fn rejects_malformed_inputs_without_panicking() {
    for value in [
        "",
        "2026",
        "2026-09-08",
        "2026-09-08T00:00:00",
        " 2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z ",
        "\t2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z\n",
        "2026-09-08 00:00:00Z",
        "2026-09-08T00: 0:00Z",
        "2026-09-08T00:00:00+24:00",
        "2026-09-08T00:00:00+00:60",
        "2026-09-08T00:00:00-24:00",
        "2026-09-08T00:00:00-00:60",
        "2026-09-08T00:00:00+5:30",
        "2026-09-08T00:00:00+0530",
        "2026-09-08T00:00:00+00",
        "2026-09-08T00:00:00+aa:00",
        "2026-09-08T00:00:00+00:aa",
        "2026-09-08T00:00:00+00:00:00",
        "2026-00-08T00:00:00Z",
        "2026-13-08T00:00:00Z",
        "2026-09-00T00:00:00Z",
        "2026-09-31T00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:60:00Z",
        "2026-09-08T00:00:61Z",
        "2026-09-08T00:00:00.Z",
        "2026-09-08T00:00:00,1Z",
        "2026-09-08T00:00:00.aZ",
        "2026-09-08T00:00:00.1aZ",
        "2026-09-08T00:00:00.1",
        "2026-09-08T00:00:00.1.2Z",
        "2026-09-08T00:00:00.1 Z",
        "２０２６-09-08T00:00:00Z",
        "2026-09-08Ｔ00:00:00Z",
        "2026-09-08T00:00:00Ｚ",
        "2026-09-08T00:00:00.١Z",
        "2026-09-08T00:00:00−00:00",
        "2026-09-08T00:00:00Z\u{a0}",
        "026-09-08T00:00:00Z",
        "02026-09-08T00:00:00Z",
        "+2026-09-08T00:00:00Z",
        "-001-09-08T00:00:00Z",
        "2026-9-08T00:00:00Z",
        "2026-09-8T00:00:00Z",
        "2026-09-0800:00:00Z",
        "2026-09-08X00:00:00Z",
        "2026-09-08T00:00:00Zgarbage",
        "2026-09-08T00:00:00ZZ",
        "2026-09-08T00:00:00Z\0",
        "2026-09-08T00:\x0000:00Z",
        "2026-09-08T00:00:00+00:00Z",
    ] {
        assert_eq!(
            WorkspaceDateTimeV1::try_new(value),
            Err(WorkspaceDateTimeError),
            "{value:?}"
        );
    }
}

#[test]
fn every_truncation_and_single_byte_mutation_is_panic_free() {
    for value in ["2026-09-08T00:00:00Z", "2026-09-08t00:00:00.123-23:59"] {
        for end in 0..value.len() {
            assert_eq!(
                WorkspaceDateTimeV1::try_new(&value[..end]),
                Err(WorkspaceDateTimeError)
            );
        }
        for index in 0..value.len() {
            for byte in 0..=127 {
                let mut bytes = value.as_bytes().to_vec();
                bytes[index] = byte;
                let input = String::from_utf8(bytes).unwrap();
                let _ = WorkspaceDateTimeV1::try_new(input);
            }
            let mut input = value.to_owned();
            input.replace_range(index..index + 1, "界");
            assert_eq!(
                WorkspaceDateTimeV1::try_new(input),
                Err(WorkspaceDateTimeError)
            );
        }
    }
}

#[test]
fn fractional_precision_has_no_artificial_maximum() {
    for length in [1, 3, 9, 18, 10_000] {
        for offset in ["Z", "z", "+23:59", "-00:00"] {
            let value = format!("2026-09-08T00:00:00.{}{offset}", "7".repeat(length));
            assert_eq!(
                WorkspaceDateTimeV1::try_new(&value).unwrap().as_str(),
                value
            );
        }
    }
}

#[test]
fn equality_and_hashing_are_lexical() {
    let spellings = [
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00Z",
        "2026-09-08T00:00:00z",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.1Z",
        "2026-09-08T00:00:00.100Z",
        "2026-09-08T01:00:00+01:00",
    ];
    let mut values = HashSet::new();
    for spelling in spellings {
        let value = WorkspaceDateTimeV1::try_new(spelling).unwrap();
        assert_eq!(value, value.clone());
        let mut value_hash = DefaultHasher::new();
        let mut lexical_hash = DefaultHasher::new();
        value.hash(&mut value_hash);
        spelling.hash(&mut lexical_hash);
        assert_eq!(value_hash.finish(), lexical_hash.finish());
        assert!(values.insert(value.clone()));
        assert!(!values.insert(value));
    }
    assert_eq!(values.len(), spellings.len());
}

#[test]
fn validation_error_is_narrow_and_standard() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<WorkspaceDateTimeError>();
    assert_eq!(
        WorkspaceDateTimeError.to_string(),
        "invalid Workspace V1 date-time"
    );
}
