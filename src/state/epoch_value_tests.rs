use crate::{StateEpochValueV1, StateError};

fn epoch(value: &str) -> StateEpochValueV1 {
    StateEpochValueV1::try_from(value).expect("canonical epoch")
}

#[test]
fn grammar_is_exact_and_unbounded() {
    for value in [
        "0",
        "1",
        "9",
        "10",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
    ] {
        assert_eq!(epoch(value).as_str(), value);
    }
    assert_eq!(
        epoch(&format!("1{}", "0".repeat(9_999))).as_str().len(),
        10_000
    );
    for value in [
        "", "00", "01", "+1", "-0", "-1", "1.0", "1e0", "1E0", " 1", "1 ", "1 0", "١",
    ] {
        assert!(
            StateEpochValueV1::try_from(value).is_err(),
            "accepted {value:?}"
        );
    }
}

#[test]
fn ordering_and_successor_are_decimal_not_machine_integer_based() {
    let values = ["0", "1", "2", "9", "10", "19", "20", "99", "100"];
    for pair in values.windows(2) {
        assert!(epoch(pair[0]) < epoch(pair[1]));
    }
    for (value, successor) in [
        ("0", "1"),
        ("8", "9"),
        ("9", "10"),
        ("19", "20"),
        ("99", "100"),
        ("1999", "2000"),
        ("9223372036854775807", "9223372036854775808"),
    ] {
        assert_eq!(
            epoch(value).successor().expect("successor"),
            epoch(successor)
        );
    }
    assert_eq!(
        epoch(&"9".repeat(10_000)).successor().expect("successor"),
        epoch(&format!("1{}", "0".repeat(10_000)))
    );
}

#[test]
fn finite_compatibility_is_explicit_and_fallible() {
    assert!(StateEpochValueV1::try_from(-1_i64).is_err());
    assert_eq!(
        StateEpochValueV1::try_from(0_i64)
            .unwrap()
            .try_to_i64()
            .unwrap(),
        0
    );
    assert_eq!(
        StateEpochValueV1::try_from(i64::MAX)
            .unwrap()
            .try_to_i64()
            .unwrap(),
        i64::MAX
    );
    assert!(matches!(
        epoch("9223372036854775808").try_to_i64(),
        Err(StateError::StateEpochValueOutOfI64Range { .. })
    ));
}
