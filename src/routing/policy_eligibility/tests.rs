use super::*;

fn timestamp(value: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(value.into()).unwrap()
}

#[test]
fn timestamp_accepts_exact_v1_language_and_preserves_every_byte() {
    for value in [
        "1985-04-12T23:20:50.52Z",
        "1996-12-19T16:39:57-08:00",
        "1937-01-01T12:00:27.87+00:20",
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00+00:00",
        "2024-02-29T23:59:59Z",
        "0000-01-01T00:00:00Z",
        "0000-02-29T00:00:00Z",
        "2000-02-29T00:00:00Z",
        "1990-12-31T23:59:60Z",
        "2026-09-08T12:34:60Z", // Lexical 60, no occurrence database.
        "9999-12-31T23:59:60-23:59",
        "2026-09-08T00:00:00.1+23:59",
        "2026-09-08t00:00:00.123456789123456789000z",
    ] {
        assert_eq!(timestamp(value).as_str(), value);
    }
    let long_fraction = format!("2026-09-08T00:00:00.{}Z", "0".repeat(10_000));
    assert_eq!(timestamp(&long_fraction).as_str(), long_fraction);

    let spellings = [
        "2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00.0Z",
        "2026-09-08T00:00:00.00Z",
    ];
    let values: std::collections::HashSet<_> = spellings.map(timestamp).into_iter().collect();
    assert_eq!(values.len(), spellings.len());
}

#[test]
fn timestamp_rejects_invalid_dates_syntax_ranges_and_whitespace() {
    for value in [
        "",
        " 2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z ",
        "2026-09-08T00:00:00Z\n",
        "2026-09-08 00:00:00Z",
        "2026-09-08\t00:00:00Z",
        "2026-09-08\n00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:60:00Z",
        "2026-09-08T00:00:61Z",
        "2026-13-08T00:00:00Z",
        "2026-00-08T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-32T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2100-02-29T00:00:00Z",
        "2024-02-30T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-09-08T00:00:00.Z",
        "2026-09-08T00:00:00,1Z",
        "2026-09-08T00:00:00.1aZ",
        "2026-09-08T00:00:00+0530",
        "2026-09-08T00:00:00+24:00",
        "2026-09-08T00:00:00-00:60",
        "26-09-08T00:00:00Z",
        "026-09-08T00:00:00Z",
        "02026-09-08T00:00:00Z",
        "+2026-09-08T00:00:00Z",
        "-2026-09-08T00:00:00Z",
        "２０２６-09-08T00:00:00Z",
        "2026-09-08T００:00:00Z",
        "2026-09-08T00:00:00.١Z",
        "2026-09-08T00:00:00+０0:00",
        "2026-9-08T00:00:00Z",
        "2026/09/08T00:00:00Z",
        "2026-09-08T0:00:00Z",
        "2026-09-08T00:00:00",
        "2026-09-08T00:00:00.123",
        "2026-09-08T00:00:00ZZ",
        "2026-09-08T00:00:00Z\0",
    ] {
        assert_eq!(
            ModelRoutingDateTimeV1::try_new(value.into()),
            Err(ModelRoutingDateTimeV1Error),
            "accepted {value:?}"
        );
    }
}

#[test]
fn exact_closed_vocabularies_and_default_policy_gate() {
    assert_eq!(
        TechnicalStatus::ALL.map(TechnicalStatus::as_str),
        [
            "CONNECTED",
            "AUTH_REQUIRED",
            "EXPIRED",
            "NOT_CONFIGURED",
            "UNKNOWN",
        ]
    );
    assert_eq!(
        PolicyStatus::ALL.map(PolicyStatus::as_str),
        [
            "VERIFIED_ALLOWED",
            "VERIFIED_DISALLOWED",
            "NEEDS_REVIEW",
            "UNKNOWN",
        ]
    );
    assert_eq!(
        PolicyEvidenceLabel::ALL.map(PolicyEvidenceLabel::as_str),
        [
            "VERIFIED_CURRENT_SELF_FETCHED",
            "REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE",
            "VERIFIED_HISTORICAL",
            "INDEPENDENT_VERIFIED",
            "USER_DECLARED",
            "DESIGN_DECISION",
            "ASSUMPTION",
            "UNVERIFIED",
            "POLICY_NEEDS_REVIEW",
        ]
    );
    assert_eq!(
        PolicyStatus::ALL.map(PolicyStatus::passes_policy_gate_by_default),
        [true, false, false, false,]
    );
}

fn identity_record(
    provider: &str,
    runtime: &str,
    credential: &str,
    contexts: Option<Vec<String>>,
) -> Result<ProviderPolicyEligibility, ProviderPolicyEligibilityError> {
    ProviderPolicyEligibility::try_new(
        provider.into(),
        runtime.into(),
        credential.into(),
        TechnicalStatus::Connected,
        PolicyStatus::Unknown,
        contexts,
        timestamp("2026-09-08t00:00:00.000z"),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        None,
        None,
    )
}

#[test]
fn identities_and_ordered_contexts_are_open_and_exact() {
    for value in ["future/Identity:α-界", " \t\n", " Mixed.Case "] {
        for contexts in [
            None,
            Some(vec![]),
            Some(vec!["future/context".into()]),
            Some(vec![
                "Z-context".into(),
                "α/界".into(),
                " \t\n".into(),
                "Z-context".into(),
            ]),
        ] {
            let record = identity_record(value, value, value, contexts.clone()).unwrap();
            assert_eq!(record.provider_id(), value);
            assert_eq!(record.runtime_id(), value);
            assert_eq!(record.credential_mode(), value);
            assert_eq!(record.allowed_execution_contexts(), contexts.as_deref());
        }
    }
    use ProviderPolicyEligibilityError::*;
    for (provider, runtime, credential, error) in [
        ("", "r", "c", EmptyProviderId),
        ("p", "", "c", EmptyRuntimeId),
        ("p", "r", "", EmptyCredentialMode),
    ] {
        assert_eq!(
            identity_record(provider, runtime, credential, None),
            Err(error)
        );
    }
    for index in 0..3 {
        let mut contexts = vec!["future/context".into(); 3];
        contexts[index] = String::new();
        assert_eq!(
            identity_record("p", "r", "c", Some(contexts)),
            Err(EmptyExecutionContext { index })
        );
    }
}

#[test]
fn technical_policy_and_evidence_axes_are_independent() {
    for technical in TechnicalStatus::ALL {
        for policy in PolicyStatus::ALL {
            for label in PolicyEvidenceLabel::ALL {
                let record = ProviderPolicyEligibility::try_new(
                    "future/p".into(),
                    "future/r".into(),
                    "future/c".into(),
                    technical,
                    policy,
                    None,
                    timestamp("2026-09-08T00:00:00Z"),
                    None,
                    label,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(record.technical_status(), technical);
                assert_eq!(record.policy_status(), policy);
                assert_eq!(record.evidence_label(), label);
                assert_eq!(
                    record.policy_status().passes_policy_gate_by_default(),
                    policy == PolicyStatus::VerifiedAllowed
                );
            }
        }
    }
    // Q-V13-04's unresolved evidence cannot promote the default policy gate.
    let unresolved = identity_record("future/p", "future/r", "future/c", None).unwrap();
    assert_eq!(
        unresolved.evidence_label(),
        PolicyEvidenceLabel::PolicyNeedsReview
    );
    assert!(!unresolved.policy_status().passes_policy_gate_by_default());
}

#[test]
fn optional_nullable_fields_and_timestamps_preserve_independent_states() {
    let strings = [
        None,
        Some(None),
        Some(Some("")),
        Some(Some(" future/claim:α \n")),
    ];
    let verified = timestamp("2026-09-08t00:00:00.000z");
    // Past, equal and future deadlines are all physical values, without expiry evaluation.
    let deadlines = [
        None,
        Some(None),
        Some(Some(timestamp("0000-01-01T00:00:00-00:00"))),
        Some(Some(verified.clone())),
        Some(Some(timestamp("9999-12-31T23:59:60+23:59"))),
    ];
    for terms in strings {
        for claim in strings {
            for evidence in [None, Some(""), Some(" opaque evidence:α \n")] {
                for reason in [None, Some(""), Some(" opaque reason:β \t")] {
                    for deadline in &deadlines {
                        let record = ProviderPolicyEligibility::try_new(
                            "p".into(),
                            "r".into(),
                            "c".into(),
                            TechnicalStatus::AuthRequired,
                            PolicyStatus::VerifiedAllowed,
                            None,
                            verified.clone(),
                            evidence.map(String::from),
                            PolicyEvidenceLabel::Unverified,
                            terms.map(|v| v.map(String::from)),
                            reason.map(String::from),
                            deadline.clone(),
                            claim.map(|v| v.map(String::from)),
                        )
                        .unwrap();
                        assert_eq!(record.terms_version_or_digest(), terms);
                        assert_eq!(record.source_claim_id(), claim);
                        assert_eq!(record.evidence_source(), evidence);
                        assert_eq!(record.reason(), reason);
                        assert_eq!(record.verified_at().as_str(), verified.as_str());
                        assert_eq!(
                            record.reverification_deadline(),
                            deadline.as_ref().map(Option::as_ref)
                        );
                        assert_eq!(record.policy_status(), PolicyStatus::VerifiedAllowed);
                        assert_eq!(record.technical_status(), TechnicalStatus::AuthRequired);
                    }
                }
            }
        }
    }
}
