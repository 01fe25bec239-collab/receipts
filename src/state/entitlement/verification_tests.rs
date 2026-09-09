use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use super::{
    EntitlementCacheDecision as Decision, EntitlementVerificationError as Error,
    EntitlementVerifier, ProductEntitlementKeyId, ProductEntitlementSubjectId,
    verification::signed_message, verification_vectors::*, wire,
};

fn subject(value: &str) -> ProductEntitlementSubjectId {
    ProductEntitlementSubjectId::new(value.into()).unwrap()
}
fn verifier() -> EntitlementVerifier {
    trust(&[("k", PUBLIC_KEY), ("next", PUBLIC_KEY)])
}
fn trust(keys: &[(&str, &str)]) -> EntitlementVerifier {
    EntitlementVerifier::new(
        keys.iter()
            .map(|(id, key)| (ProductEntitlementKeyId::new((*id).into()).unwrap(), *key)),
    )
    .unwrap()
}
fn verify(raw: &str) -> Result<super::VerifiedProductEntitlement, Error> {
    verifier().reverify_cached(raw.as_bytes(), &subject("é"))
}
fn encoded(bytes: &[u8]) -> String {
    let mut buffer = vec![0; bytes.len().div_ceil(3) * 4];
    let length = URL_SAFE_NO_PAD.encode_slice(bytes, &mut buffer).unwrap();
    String::from_utf8(buffer[..length].to_vec()).unwrap()
}
fn with_signature(signature: &str) -> String {
    let original = wire::parse(VALID.as_bytes()).unwrap();
    VALID.replace(original.signature().as_str(), signature)
}
fn append(raw: &str, member: &str) -> String {
    format!("{},{}{}", &raw[..raw.len() - 1], member, "}")
}
fn parsed_error(raw: &str, expected: Error) {
    assert_eq!(wire::parse(raw.as_bytes()).unwrap_err(), expected, "{raw}");
}

#[test]
fn valid_signature_and_exact_immutable_cache_artifact() {
    let proof = verify(VALID).unwrap();
    assert_eq!(proof.raw_document(), VALID.as_bytes());
    assert_eq!(proof.trusted_key_id().as_str(), "k");
    assert_eq!(proof.entitlement().tier_id().as_str(), "future-tier");
    assert_eq!(proof.entitlement().subject_id().as_str(), "é");
    assert_eq!(proof.entitlement().device_binding(), None);
    assert_eq!(
        proof
            .entitlement()
            .capabilities()
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
        ["b.c", "a.b", "b.c"]
    );
    assert!(matches!(
        verifier().ingest(VALID.as_bytes(), &subject("é"), None),
        Ok(Decision::Replace(_))
    ));
    assert!(verify(MAX_VERSION).is_ok());
}

#[test]
fn literal_independent_binary_frame_oracle() {
    // Byte lengths (including multibyte subject), order, count, version and option markers
    // are literal protocol bytes, independently specified rather than produced by a helper.
    let expected = b"receipts.product-entitlement.v1\0\
        \0\0\0\0\0\0\0\x02\xc3\xa9\
        \0\0\0\0\0\0\0\x0bfuture-tier\
        \0\0\0\0\0\0\0\x03\
        \0\0\0\0\0\0\0\x03b.c\
        \0\0\0\0\0\0\0\x03a.b\
        \0\0\0\0\0\0\0\x03b.c\
        \0\0\0\0\0\0\0\x1e2026-01-01T00:00:00.000000000Z\
        \0\0\0\0\0\0\0\x1e2027-01-01T00:00:00.000000000Z\
        \0\0\0\0\0\0\0\x02\
        \0\0\0\0\0\0\0\x01k\
        \x01\0\0\0\0\0\0\0\x1e2027-02-01T00:00:00.000000000Z\
        \0";
    assert_eq!(
        signed_message(&wire::parse(VALID.as_bytes()).unwrap()),
        expected
    );
}

#[test]
fn all_ten_decoded_members_reject_duplicates_including_null_optionals() {
    for (field, value) in [
        ("subject_id", "\"é\""),
        ("tier_id", "\"pro\""),
        ("capabilities", "[]"),
        ("issued_at", "\"x\""),
        ("expires_at", "\"x\""),
        ("entitlement_version", "2"),
        ("key_id", "\"k\""),
        ("signature", "\"x\""),
        ("offline_grace_until", "null"),
        ("device_binding", "null"),
    ] {
        let base = if field == "device_binding" {
            append(VALID, "\"device_binding\":null")
        } else {
            VALID.into()
        };
        parsed_error(
            &append(&base, &format!("\"{field}\":{value}")),
            Error::DuplicateField(field),
        );
    }
    parsed_error(
        &append(VALID, r#""subject\u005fid":"é""#),
        Error::DuplicateField("subject_id"),
    );
    let escaped = VALID.replace("subject_id", "subject\\u005fid");
    assert_eq!(
        verify(&escaped).unwrap().entitlement(),
        verify(VALID).unwrap().entitlement()
    );
    parsed_error(
        &append(&escaped, r#""subject_id":"é""#),
        Error::DuplicateField("subject_id"),
    );
}

#[test]
fn unknown_members_rejected_before_between_and_after_known_members() {
    for name in [
        "unknown",
        "subјect_id",
        "subject_id.extra",
        "Subject_id",
        "subject\\u0000_id",
    ] {
        let member = format!("\"{name}\":{{\"nested\":true}}");
        parsed_error(&format!("{{{member},{}", &VALID[1..]), Error::UnknownField);
        parsed_error(&append(VALID, &member), Error::UnknownField);
        parsed_error(
            &VALID.replacen(",\"tier_id\"", &format!(",{member},\"tier_id\""), 1),
            Error::UnknownField,
        );
    }
    // Immediate rejection must not try to deserialize an unknown value.
    parsed_error("{\"unknown\":", Error::UnknownField);
}

#[test]
fn exactly_one_object_and_complete_input() {
    for suffix in [
        "{}",
        " true",
        " 123",
        " []",
        " malformed",
        " null",
        " /",
        "\0",
    ] {
        parsed_error(&format!("{VALID}{suffix}"), Error::MalformedJson);
    }
    for raw in ["[]", "null", "true", "123", "\"object\"", "", "{", "}"] {
        parsed_error(raw, Error::MalformedJson);
    }
    for raw in [
        format!(" \t\r\n{VALID}"),
        format!("{VALID} \t\r\n"),
        format!(" \n{VALID}\t "),
    ] {
        assert_eq!(verify(&raw).unwrap().raw_document(), raw.as_bytes());
    }
}

#[test]
fn all_required_members_must_exist() {
    // The fixture has no escaped quotes or commas inside these scalar members.
    for (field, value) in [
        ("subject_id", "\"é\""),
        ("tier_id", "\"future-tier\""),
        ("capabilities", "[\"b.c\",\"a.b\",\"b.c\"]"),
        ("issued_at", "\"2026-01-01T00:00:00.000000000Z\""),
        ("expires_at", "\"2027-01-01T00:00:00.000000000Z\""),
        ("entitlement_version", "2"),
        ("key_id", "\"k\""),
    ] {
        let raw = VALID.replace(&format!("\"{field}\":{value},"), "");
        assert_ne!(raw, VALID);
        parsed_error(&raw, Error::MissingRequiredField(field));
    }
    let start = VALID.find(",\"signature\"").unwrap();
    parsed_error(
        &format!("{}}}", &VALID[..start]),
        Error::MissingRequiredField("signature"),
    );
}

#[test]
fn wire_size_is_checked_before_utf8_and_parsing() {
    let mut raw = VALID.as_bytes().to_vec();
    raw.resize(65_536, b' ');
    assert_eq!(
        verifier()
            .reverify_cached(&raw, &subject("é"))
            .unwrap()
            .raw_document(),
        raw
    );
    raw.push(b' ');
    assert_eq!(
        verifier().reverify_cached(&raw, &subject("é")),
        Err(Error::DocumentTooLarge)
    );
    assert_eq!(
        wire::parse(&vec![255; 65_537]),
        Err(Error::DocumentTooLarge)
    );
    assert_eq!(wire::parse(&[255]), Err(Error::InvalidUtf8));
    assert_eq!(wire::parse(&vec![b' '; 65_536]), Err(Error::MalformedJson));
}

#[test]
fn truncated_and_malformed_documents_never_panic() {
    for end in 0..VALID.len() {
        assert!(
            verifier()
                .reverify_cached(&VALID.as_bytes()[..end], &subject("é"))
                .is_err()
        );
    }
    for raw in [
        r#"{"subject_id":"\uD800"}"#,
        r#"{"subject_id":"\x"}"#,
        r#"{"subject_id":]}"#,
        r#"{"subject_id":{},}"#,
    ] {
        assert!(
            verifier()
                .reverify_cached(raw.as_bytes(), &subject("é"))
                .is_err()
        );
    }
    let nested = format!(
        "{{\"capabilities\":{}0{}}}",
        "[".repeat(200),
        "]".repeat(200)
    );
    assert!(wire::parse(nested.as_bytes()).is_err());
}

#[test]
fn existing_physical_validation_and_strict_timestamps_are_used() {
    for (from, to, field) in [
        ("\"subject_id\":\"é\"", "\"subject_id\":\"\"", "subject_id"),
        ("\"tier_id\":\"future-tier\"", "\"tier_id\":\"\"", "tier_id"),
        ("\"key_id\":\"k\"", "\"key_id\":\"\"", "key_id"),
        ("\"b.c\"", "\"Invalid\"", "capabilities"),
        ("\"subject_id\":\"é\"", "\"subject_id\":true", "subject_id"),
    ] {
        parsed_error(&VALID.replace(from, to), Error::InvalidPhysicalField(field));
    }
    parsed_error(
        &with_signature(""),
        Error::InvalidPhysicalField("signature"),
    );
    parsed_error(
        &VALID.replace("\"é\"", &format!("\"{}\"", "é".repeat(201))),
        Error::InvalidPhysicalField("subject_id"),
    );
    for (field, original) in [
        ("issued_at", "2026-01-01T00:00:00.000000000Z"),
        ("expires_at", "2027-01-01T00:00:00.000000000Z"),
        ("offline_grace_until", "2027-02-01T00:00:00.000000000Z"),
    ] {
        for invalid in [
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00.00000000Z",
            "2026-01-01T00:00:00.0000000000Z",
            "2026-01-01T00:00:00.000000000+00:00",
            "2026-02-30T00:00:00.000000000Z",
            "2026-01-01T00:00:00.000000000z",
        ] {
            parsed_error(
                &VALID.replace(original, invalid),
                Error::UnsupportedTimestamp(field),
            );
        }
    }
}

#[test]
fn version_accepts_only_positive_signed_integer_range_without_coercion() {
    for value in [
        "0",
        "-1",
        "9223372036854775808",
        "18446744073709551616",
        "999999999999999999999999999999999999",
        "1.0",
        "1e0",
        "1e9999",
        "null",
        "true",
        "\"2\"",
    ] {
        parsed_error(
            &VALID.replace(
                "\"entitlement_version\":2",
                &format!("\"entitlement_version\":{value}"),
            ),
            Error::UnsupportedVersion,
        );
    }
    assert_eq!(
        wire::parse(MAX_VERSION.as_bytes())
            .unwrap()
            .entitlement_version()
            .get(),
        i64::MAX
    );
}

#[test]
fn signature_encoding_is_strict_url_safe_unpadded_and_exact_length() {
    let signature = wire::parse(VALID.as_bytes())
        .unwrap()
        .signature()
        .as_str()
        .to_owned();
    for invalid in [
        "%%%".to_owned(),
        format!("{signature}="),
        "+".repeat(86),
        "/".repeat(86),
        encoded(&[1; 63]),
        encoded(&[1; 65]),
        format!(" {signature}"),
        format!("{signature}\n"),
    ] {
        // Newline is escaped to remain a valid JSON string.
        assert_eq!(
            verify(&with_signature(&invalid.replace('\n', "\\n"))),
            Err(Error::InvalidSignatureEncoding)
        );
    }
    let mut bytes = [0; 64];
    URL_SAFE_NO_PAD
        .decode_slice(&signature, &mut bytes)
        .unwrap();
    bytes[0] ^= 1;
    assert_eq!(
        signed_message(&wire::parse(VALID.as_bytes()).unwrap()),
        signed_message(&wire::parse(with_signature(&encoded(&bytes)).as_bytes()).unwrap()),
    );
    assert_eq!(
        verify(&with_signature(&encoded(&bytes))),
        Err(Error::SignatureVerificationFailed)
    );
    // Nonzero unused trailing bits are not an alternate representation.
    let mut noncanonical = signature.into_bytes();
    let last = noncanonical.len() - 1;
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let position = alphabet
        .iter()
        .position(|b| *b == noncanonical[last])
        .unwrap();
    noncanonical[last] = alphabet[position + 1];
    assert_eq!(
        verify(&with_signature(std::str::from_utf8(&noncanonical).unwrap())),
        Err(Error::InvalidSignatureEncoding)
    );
}

#[test]
fn trust_set_rejects_invalid_keys_and_duplicate_ids() {
    for encoded in [
        "bad!".to_owned(),
        format!("{PUBLIC_KEY}="),
        encoded(&[1; 31]),
        encoded(&[1; 33]),
        encoded(&[0; 32]),
        encoded(&[2; 32]),
        "+".repeat(43),
    ] {
        assert!(matches!(
            EntitlementVerifier::new([(
                ProductEntitlementKeyId::new("k".into()).unwrap(),
                encoded.as_str()
            )]),
            Err(Error::InvalidPublicKey)
        ));
    }
    for second in [PUBLIC_KEY, WRONG_KEY] {
        assert!(matches!(
            EntitlementVerifier::new(
                [("k", PUBLIC_KEY), ("k", second)]
                    .map(|(id, key)| (ProductEntitlementKeyId::new(id.into()).unwrap(), key))
            ),
            Err(Error::DuplicateTrustedKeyId)
        ));
    }
    assert_eq!(
        trust(&[("k", WRONG_KEY)]).reverify_cached(VALID.as_bytes(), &subject("é")),
        Err(Error::SignatureVerificationFailed)
    );
    assert_eq!(
        trust(&[("next", PUBLIC_KEY)]).reverify_cached(VALID.as_bytes(), &subject("é")),
        Err(Error::UnknownKeyId)
    );
    assert_eq!(
        verify(&VALID.replace("\"key_id\":\"k\"", "\"key_id\":\"unknown\"")),
        Err(Error::UnknownKeyId)
    );
    assert!(verify(OTHER_KEY_ID).is_ok());
    assert!(matches!(
        EntitlementVerifier::new([])
            .unwrap()
            .reverify_cached(VALID.as_bytes(), &subject("é")),
        Err(Error::UnknownKeyId)
    ));
}

#[test]
fn signed_field_changes_and_capability_order_or_dedup_invalidate_signature() {
    for raw in [
        VALID.replace("future-tier", "modified-tier"),
        VALID.replace("[\"b.c\",\"a.b\",\"b.c\"]", "[\"a.b\",\"b.c\",\"b.c\"]"),
        VALID.replace("[\"b.c\",\"a.b\",\"b.c\"]", "[\"b.c\",\"a.b\"]"),
        VALID.replace("2027-02-01", "2027-02-02"),
        VALID.replace("\"entitlement_version\":2", "\"entitlement_version\":3"),
    ] {
        assert_eq!(verify(&raw), Err(Error::SignatureVerificationFailed));
    }
}

#[test]
fn subject_exactness_including_unicode_normalization_case_and_spaces() {
    for expected in ["e\u{301}", "É", " é", "é ", "other"] {
        assert_eq!(
            verifier().reverify_cached(VALID.as_bytes(), &subject(expected)),
            Err(Error::SubjectMismatch)
        );
    }
    assert_eq!(verify(DECOMPOSED), Err(Error::SubjectMismatch));
    assert!(
        verifier()
            .reverify_cached(DECOMPOSED.as_bytes(), &subject("e\u{301}"))
            .is_ok()
    );
    assert_eq!(
        verify(&VALID.replace('é', "e\u{301}")),
        Err(Error::SignatureVerificationFailed)
    );
    assert_ne!(
        signed_message(&wire::parse(VALID.as_bytes()).unwrap()),
        signed_message(&wire::parse(DECOMPOSED.as_bytes()).unwrap())
    );
}

#[test]
fn omitted_and_present_optionals_preserve_binary_framing_and_v1_policy() {
    let absent = NO_OPTIONALS.replace(",\"offline_grace_until\":null", "");
    let proof = verify(&absent).unwrap();
    assert_eq!(proof.entitlement().offline_grace_until(), None);
    assert_eq!(proof.entitlement().device_binding(), None);
    let absent_message = signed_message(proof.entitlement());
    let present = verify(VALID).unwrap();
    assert_eq!(
        present
            .entitlement()
            .offline_grace_until()
            .unwrap()
            .as_str(),
        "2027-02-01T00:00:00.000000000Z"
    );
    let unbound = signed_message(present.entitlement());
    let optional_start = absent_message.len() - 2;
    assert_eq!(&absent_message[optional_start..], &[0, 0]);
    assert_eq!(
        &unbound[..optional_start],
        &absent_message[..optional_start]
    );
    assert_eq!(
        &unbound[optional_start..],
        b"\x01\0\0\0\0\0\0\0\x1e2027-02-01T00:00:00.000000000Z\x00"
    );
    let empty = signed_message(&wire::parse(BOUND_EMPTY.as_bytes()).unwrap());
    assert_eq!(&empty[..unbound.len() - 1], &unbound[..unbound.len() - 1]);
    assert_eq!(&empty[unbound.len() - 1..], &[1, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_ne!(unbound, absent_message);
    assert_eq!(verify(BOUND_EMPTY), Err(Error::UnsupportedDeviceBindingV1));
    assert_eq!(verify(BOUND), Err(Error::UnsupportedDeviceBindingV1));
    assert_eq!(
        verify(&append(VALID, "\"device_binding\":\"\"")),
        Err(Error::SignatureVerificationFailed)
    );
}

#[test]
fn explicit_null_never_reaches_signing_proof_or_cache_authority() {
    let absent = NO_OPTIONALS.replace(",\"offline_grace_until\":null", "");
    let prior = verify(&absent).unwrap();
    let snapshot = prior.clone();
    for field in ["offline_grace_until", "device_binding"] {
        for key in [field.to_owned(), field.replacen('_', "\\u005f", 1)] {
            for token in ["null", " \t\r\nnull \t\r\n"] {
                // Retain the valid absent-state signature: accepting null would verify.
                let raw = append(&absent, &format!("\"{key}\":{token}"));
                let error = Error::InvalidPhysicalField(field);
                assert_eq!(
                    wire::parse(raw.as_bytes()).map(|p| signed_message(&p)),
                    Err(error)
                );
                assert_eq!(verify(&raw), Err(error));
                assert_eq!(
                    verifier().ingest(raw.as_bytes(), &subject("é"), None),
                    Err(error)
                );
                assert_eq!(
                    verifier().ingest(raw.as_bytes(), &subject("é"), Some(&prior)),
                    Err(error)
                );
                assert_eq!(prior, snapshot);
            }
        }
    }
    assert!(verify(NO_OPTIONALS).is_err());
    assert!(verify(std::str::from_utf8(prior.raw_document()).unwrap()).is_ok());
}

#[test]
fn optional_null_duplicates_win_in_both_orders_and_key_spellings() {
    let absent = NO_OPTIONALS.replace(",\"offline_grace_until\":null", "");
    for (field, value) in [
        ("offline_grace_until", "\"2026-09-09T00:00:00.000000000Z\""),
        ("device_binding", "\"\""),
    ] {
        let escaped = field.replacen('_', "\\u005f", 1);
        for (first_key, second_key) in [(field, field), (&escaped, field), (field, &escaped)] {
            for (first_value, second_value) in [("null", value), (value, "null"), ("null", "null")]
            {
                let raw = append(
                    &absent,
                    &format!("\"{first_key}\":{first_value},\"{second_key}\":{second_value}"),
                );
                parsed_error(&raw, Error::DuplicateField(field));
            }
        }
    }
}

#[test]
fn optional_validation_does_not_confuse_empty_wrong_types_or_nested_null() {
    parsed_error(
        &VALID.replace("2027-02-01T00:00:00.000000000Z", ""),
        Error::UnsupportedTimestamp("offline_grace_until"),
    );
    let absent = NO_OPTIONALS.replace(",\"offline_grace_until\":null", "");
    for field in ["offline_grace_until", "device_binding"] {
        for value in ["true", "42", "[null]", "{\"nested\":null}"] {
            parsed_error(
                &append(&absent, &format!("\"{field}\":{value}")),
                Error::InvalidPhysicalField(field),
            );
        }
    }
    for value in [
        "null",
        "{\"device_binding\":null,\"offline_grace_until\":null}",
    ] {
        parsed_error(
            &append(&absent, &format!("\"unknown\":{value}")),
            Error::UnknownField,
        );
    }
    parsed_error(
        &absent.replace("[\"b.c\",\"a.b\",\"b.c\"]", "[null]"),
        Error::InvalidPhysicalField("capabilities"),
    );
}

#[test]
fn whitespace_device_binding_is_present_and_unsupported_when_authentic() {
    use ed25519_dalek::{Signer, SigningKey};

    let raw = append(VALID, "\"device_binding\":\" \"");
    let physical = wire::parse(raw.as_bytes()).unwrap();
    assert_eq!(physical.device_binding(), Some(" "));
    let message = signed_message(&physical);
    assert!(message.ends_with(&[1, 0, 0, 0, 0, 0, 0, 0, 1, b' ']));
    let key = SigningKey::from_bytes(&[7; 32]);
    let signature = encoded(&key.sign(&message).to_bytes());
    let raw = raw.replace(physical.signature().as_str(), &signature);
    assert_eq!(
        trust(&[("k", &encoded(&key.verifying_key().to_bytes()))])
            .reverify_cached(raw.as_bytes(), &subject("é")),
        Err(Error::UnsupportedDeviceBindingV1)
    );
}

#[test]
fn json_order_is_irrelevant_but_json_and_wrong_binary_order_are_not_signing_authority() {
    let reordered = VALID.replacen(
        "\"subject_id\":\"é\",\"tier_id\":\"future-tier\"",
        "\"tier_id\":\"future-tier\",\"subject_id\":\"é\"",
        1,
    );
    assert_ne!(reordered, VALID);
    assert_eq!(
        signed_message(&wire::parse(reordered.as_bytes()).unwrap()),
        signed_message(&wire::parse(VALID.as_bytes()).unwrap())
    );
    assert!(verify(&reordered).is_ok());
    assert_eq!(verify(WRONG_ORDER), Err(Error::SignatureVerificationFailed));
    assert_eq!(verify(SIGNED_JSON), Err(Error::SignatureVerificationFailed));
    let physical = wire::parse(VALID.as_bytes()).unwrap();
    let mut key = [0; 32];
    let mut signature = [0; 64];
    URL_SAFE_NO_PAD.decode_slice(PUBLIC_KEY, &mut key).unwrap();
    URL_SAFE_NO_PAD
        .decode_slice(physical.signature().as_str(), &mut signature)
        .unwrap();
    let key = ed25519_dalek::VerifyingKey::from_bytes(&key).unwrap();
    let signature = ed25519_dalek::Signature::from_bytes(&signature);
    assert!(key.verify_strict(VALID.as_bytes(), &signature).is_err());
    // Canonicalizer is exercised only as an attack oracle, never production authority.
    let canonical = serde_json_canonicalizer::pipe(VALID).unwrap();
    assert!(key.verify_strict(canonical.as_bytes(), &signature).is_err());
}

#[test]
fn replay_and_cache_replacement_preserve_verified_prior() {
    let verifier = verifier();
    let expected = subject("é");
    let prior = verify(VALID).unwrap();
    let snapshot = prior.clone();
    assert_eq!(
        verifier.ingest(LOWER.as_bytes(), &expected, Some(&prior)),
        Err(Error::VersionRollback)
    );
    assert_eq!(
        verifier.ingest(VALID.as_bytes(), &expected, Some(&prior)),
        Ok(Decision::KeepExisting)
    );
    assert_eq!(
        verifier.ingest(format!(" {VALID} ").as_bytes(), &expected, Some(&prior)),
        Ok(Decision::KeepExisting)
    );
    assert_eq!(
        verifier.ingest(DIVERGENT.as_bytes(), &expected, Some(&prior)),
        Err(Error::EqualVersionDivergence)
    );
    let Decision::Replace(next) = verifier
        .ingest(HIGHER.as_bytes(), &expected, Some(&prior))
        .unwrap()
    else {
        panic!("higher version must replace")
    };
    assert_eq!(next.entitlement().entitlement_version().get(), 3);
    for raw in [
        "{",
        LOWER,
        DIVERGENT,
        BOUND,
        BOUND_EMPTY,
        DECOMPOSED,
        SIGNED_JSON,
    ] {
        assert!(
            verifier
                .ingest(raw.as_bytes(), &expected, Some(&prior))
                .is_err()
        );
        assert_eq!(prior, snapshot);
    }
    for raw in [
        with_signature("%%%"),
        with_signature(&encoded(&[0; 64])),
        VALID.replace("\"key_id\":\"k\"", "\"key_id\":\"removed\""),
    ] {
        assert!(
            verifier
                .ingest(raw.as_bytes(), &expected, Some(&prior))
                .is_err()
        );
        assert_eq!(prior, snapshot);
    }
}

#[test]
fn cached_bytes_and_artifacts_reverify_under_current_trust_and_subject() {
    let prior = verify(VALID).unwrap();
    assert!(
        verifier()
            .reverify_cached(prior.raw_document(), &subject("é"))
            .is_ok()
    );
    assert_eq!(
        verify(&VALID.replace("future-tier", "corrupt-tier")),
        Err(Error::SignatureVerificationFailed)
    );
    assert_eq!(
        trust(&[("next", PUBLIC_KEY)]).ingest(OTHER_KEY_ID.as_bytes(), &subject("é"), Some(&prior)),
        Err(Error::UnknownKeyId)
    );
    assert_eq!(
        trust(&[("k", WRONG_KEY)]).ingest(VALID.as_bytes(), &subject("é"), Some(&prior)),
        Err(Error::SignatureVerificationFailed)
    );
    assert_eq!(
        verifier().ingest(DECOMPOSED.as_bytes(), &subject("e\u{301}"), Some(&prior)),
        Err(Error::SubjectMismatch)
    );
}

#[test]
fn verifier_failure_remains_unknown_for_activated_user() {
    use super::*;
    let activation =
        ActivationIdentityFields::new(ActivationStateKind::ActivatedKnown, Some("é".into()), None);
    assert!(verify(SIGNED_JSON).is_err());
    assert_eq!(
        resolve_product_entitlement_state(
            &activation,
            ObservedEntitlementEvidence::Corrupt,
            LicensingServiceAvailability::Unavailable,
            LocalClockEvidence::NoRollbackDetected
        ),
        ProductEntitlementState::EntitlementUnknown
    );
}

#[test]
fn errors_and_proof_debug_do_not_expose_wire_or_crypto_buffers() {
    let proof = verify(VALID).unwrap();
    assert_eq!(format!("{proof:?}"), "VerifiedProductEntitlement { .. }");
    for error in [
        Error::UnknownField,
        Error::InvalidSignatureEncoding,
        Error::InvalidPublicKey,
        Error::SignatureVerificationFailed,
    ] {
        let text = format!("{error} {error:?}");
        assert!(!text.contains(VALID));
        assert!(!text.contains(PUBLIC_KEY));
        assert!(!text.contains(proof.entitlement().signature().as_str()));
    }
    parsed_error("{\"secret-untrusted-field\":true}", Error::UnknownField);
}
