use crate::json_integer::is_integer;
use serde_json::value::RawValue;
fn integral(s: &str) -> bool {
    is_integer(&serde_json::from_str::<Box<RawValue>>(s).unwrap())
}

#[test]
fn bounded_independent_rational_oracle() {
    // Independent bounded integer arithmetic: coefficient * 10^e / 10^f.
    for coefficient in -110i128..=110 {
        for fractional in 0..=3u32 {
            for exponent in -5..=5i32 {
                let negative = coefficient < 0;
                let magnitude = coefficient.abs();
                let digits = format!("{magnitude:0width$}", width = fractional as usize + 1);
                let split = digits.len() - fractional as usize;
                let number = if fractional == 0 {
                    digits
                } else {
                    format!("{}.{}", &digits[..split], &digits[split..])
                };
                let raw = format!("{}{number}e{exponent:+}", if negative { "-" } else { "" });
                let (numerator, denominator) = if exponent >= 0 {
                    (
                        coefficient * 10i128.pow(exponent as u32),
                        10i128.pow(fractional),
                    )
                } else {
                    (
                        coefficient,
                        10i128.pow(fractional + exponent.unsigned_abs()),
                    )
                };
                assert_eq!(integral(&raw), numerator % denominator == 0, "{raw}");
            }
        }
    }
}
#[test]
fn huge_text_witnesses_and_token_types() {
    for (s, want) in [
        ("1e1000001", true),
        ("-1e1000001", true),
        ("1e-1000001", false),
        ("-1e-1000001", false),
        ("1e9223372036854775808", true),
        ("1e-9223372036854775808", false),
        ("-0e-9223372036854775808", true),
        ("100.0e-2", true),
        ("100.0e-3", false),
        ("1.000", true),
        ("1e-000", true),
        ("1e+000", true),
        ("1.01e+000", false),
    ] {
        assert_eq!(integral(s), want, "{s}");
    }
    let huge = "9".repeat(100_000);
    for (s, want) in [
        (format!("1e{huge}"), true),
        (format!("1e-{huge}"), false),
        (format!("-0.000e-{huge}"), true),
    ] {
        assert_eq!(integral(&s), want);
    }
    for s in [
        "null",
        "true",
        "[]",
        "{}",
        "\"1\"",
        r#"{"$serde_json::private::Number":"1"}"#,
    ] {
        assert!(!integral(s));
    }
}
