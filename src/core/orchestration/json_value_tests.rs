use super::{
    OrchestrationJsonNumberError, OrchestrationJsonNumberV1 as Number,
    OrchestrationJsonObjectV1 as Object, OrchestrationJsonValueV1 as Value,
};
use std::collections::{BTreeMap, HashSet};

fn number(input: &str) -> Value {
    Value::Number(Number::try_new(input).unwrap())
}

#[test]
fn all_six_variants_and_empty_containers() {
    let values = [
        Value::Null,
        Value::Boolean(false),
        number("0"),
        Value::String(String::new()),
        Value::Array(Vec::new()),
        Value::Object(Object::default()),
    ];
    assert_eq!(values.iter().cloned().collect::<HashSet<_>>().len(), 6);
    assert_ne!(Value::Boolean(true), Value::Boolean(false));
    assert!(Object::default().as_map().is_empty());
    assert_eq!(Object::new(BTreeMap::new()), Object::default());
    assert_eq!(values[4], Value::Array(vec![]));
}

#[test]
fn objects_preserve_exact_keys_and_mapping_identity() {
    let keys = [
        "",
        "name",
        "Name",
        " name ",
        "é",
        "e\u{301}",
        "日本語",
        "🙂",
    ];
    let nested = Value::Array(vec![Value::Object(Object::default()), number("1")]);
    let mut first = Object::default();
    let mut second = Object::default();
    for key in keys {
        assert_eq!(first.insert(key.to_owned(), nested.clone()), None);
    }
    for key in keys.into_iter().rev() {
        second.insert(key.to_owned(), nested.clone());
    }
    assert_eq!(first, second);
    assert_eq!(first.as_map().len(), keys.len());
    for key in keys {
        assert_eq!(first.as_map().get(key), Some(&nested));
    }
    let mut sorted = keys.to_vec();
    sorted.sort();
    assert_eq!(
        first
            .as_map()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        sorted
    );
    assert_eq!(Object::new(first.as_map().clone()), second);
    assert_eq!(HashSet::from([first.clone(), second]).len(), 1);
    assert_eq!(first.insert("name".into(), Value::Null), Some(nested));
    assert_eq!(first.as_map().len(), keys.len());
    assert_eq!(first.as_map().get("name"), Some(&Value::Null));
}

#[test]
fn arrays_preserve_order_duplicates_and_recursive_composition() {
    assert_ne!(
        Value::Array(vec![number("1"), number("2")]),
        Value::Array(vec![number("2"), number("1")])
    );
    let duplicate = Value::Array(vec![number("1"), number("1")]);
    let Value::Array(elements) = &duplicate else {
        panic!("expected array")
    };
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0], number("1"));
    assert_eq!(elements[1], number("1"));
    let mut object = Object::default();
    object.insert("array".into(), duplicate.clone());
    object.insert("object".into(), Value::Object(Object::default()));
    let nested = Value::Array(vec![
        Value::Object(object.clone()),
        Value::Array(vec![duplicate]),
    ]);
    let Value::Array(elements) = nested.clone() else {
        panic!("expected array")
    };
    assert_eq!(elements[0], Value::Object(object));
    assert!(matches!(&elements[1], Value::Array(inner) if inner.len() == 1));
    assert_eq!(nested.clone(), nested);
}

#[test]
fn mandatory_valid_number_matrix_round_trips_exactly() {
    for input in [
        "0",
        "-0",
        "1",
        "-1",
        "10",
        "123456789012345678901234567890",
        "0.0",
        "-0.0",
        "1.25",
        "-123.456",
        "1e0",
        "1E0",
        "1e+0",
        "1e-0",
        "-1.25e10",
        "-1.25E-10",
        "1e999999999999999999999",
        "1E+0",
    ] {
        assert_eq!(
            Number::try_new(input).unwrap().as_str().as_bytes(),
            input.as_bytes()
        );
    }
}

#[test]
fn number_components_have_no_artificial_range_or_precision_limits() {
    for length in [1, 128, 10_000] {
        for input in [
            "9".repeat(length),
            format!("-0.{}", "1".repeat(length)),
            format!("1E-{}", "9".repeat(length)),
        ] {
            assert_eq!(Number::try_new(input.clone()).unwrap().as_str(), input);
        }
    }
}

#[test]
fn invalid_number_matrix() {
    for input in [
        "",
        "+1",
        "01",
        "-01",
        "00",
        "1.",
        ".1",
        "-.1",
        "1e",
        "1E",
        "1e+",
        "1e-",
        "NaN",
        "Infinity",
        "-Infinity",
        "inf",
        "0x10",
        " 1",
        "1 ",
        "1 2",
        "--",
        "-",
        "e1",
        "1ee2",
        "1e+-2",
        "1..0",
        "- 1",
        "１",
        "١",
        "−1",
        "1\n2",
        "1\t2",
        "\r1",
        "1\r",
        "\n1",
        "1\n",
        "\r\n1",
        "1\r\n",
        "1\0",
        "日本é🙂",
        "🙂",
        "0.e1",
        "0e.1",
        "1e--2",
        "1e++2",
        "1e2.3",
    ] {
        assert_eq!(
            Number::try_new(input),
            Err(OrchestrationJsonNumberError),
            "{input:?}"
        );
    }
}

#[test]
fn non_ascii_and_truncated_numbers_do_not_panic() {
    let valid = "-1.25E+10";
    for text in ["２", "٢", "−", "é", "\u{301}", "日本語", "🙂"] {
        for index in 0..=valid.len() {
            let mut input = valid.to_owned();
            input.insert_str(index, text);
            assert_eq!(Number::try_new(input), Err(OrchestrationJsonNumberError));
            if index < valid.len() {
                let mut input = valid.to_owned();
                input.replace_range(index..index + 1, text);
                assert_eq!(Number::try_new(input), Err(OrchestrationJsonNumberError));
            }
        }
    }
    for end in 0..=valid.len() {
        assert_eq!(
            Number::try_new(&valid[..end]).is_ok(),
            matches!(end, 2 | 4 | 5 | 8 | 9)
        );
    }
}

#[test]
fn number_identity_is_lexical_in_values_and_hashing() {
    let inputs = ["1", "1.0", "1e0", "1E0", "1E+0", "0", "-0", "0.0", "-0.0"];
    let numbers: Vec<_> = inputs
        .iter()
        .map(|input| Number::try_new(*input).unwrap())
        .collect();
    for (index, value) in numbers.iter().enumerate() {
        assert_eq!(value.as_str(), inputs[index]);
        for other in &numbers[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(Value::Number(value.clone()), Value::Number(other.clone()));
        }
    }
    assert_eq!(
        numbers.into_iter().collect::<HashSet<_>>().len(),
        inputs.len()
    );
}

#[test]
fn strings_remain_exact_without_escape_processing() {
    for input in [
        "",
        "é日本語🙂",
        "e\u{301}",
        " \t\r\n ",
        "Name",
        "name",
        "\"",
        "\\",
        "a\\nb",
        "a\nb",
    ] {
        let Value::String(value) = Value::String(input.to_owned()) else {
            panic!("expected string")
        };
        assert_eq!(value.as_bytes(), input.as_bytes());
    }
    assert_ne!(Value::String("Name".into()), Value::String("name".into()));
    assert_ne!(Value::String("a\\nb".into()), Value::String("a\nb".into()));
    assert_ne!(Value::String("é".into()), Value::String("e\u{301}".into()));
}
