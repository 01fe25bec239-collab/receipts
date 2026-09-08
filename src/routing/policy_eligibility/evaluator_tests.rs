use super::*;

fn record(
    provider: &str,
    policy: PolicyStatus,
    technical: TechnicalStatus,
    contexts: Option<&[&str]>,
    deadline: Option<Option<&str>>,
) -> ProviderPolicyEligibility {
    ProviderPolicyEligibility::try_new(
        provider.into(),
        "future/runtime".into(),
        "future/credential".into(),
        technical,
        policy,
        contexts.map(|values| values.iter().map(|value| String::from(*value)).collect()),
        ModelRoutingDateTimeV1::try_new("2026-09-08T00:00:00Z".into()).unwrap(),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        deadline
            .map(|value| value.map(|value| ModelRoutingDateTimeV1::try_new(value.into()).unwrap())),
        None,
    )
    .unwrap()
}

#[test]
fn policy_status_cannot_be_bypassed_by_technical_status_or_provider_identity() {
    // The three-input API has no user pin, preference or score override.
    let evaluate: fn(&ProviderPolicyEligibility, Option<&str>, Option<bool>) -> bool =
        PolicyEligibilityEvaluator::evaluate;
    for provider in [
        "future/provider",
        "another/provider:α",
        " Arbitrary.Provider ",
    ] {
        for policy in PolicyStatus::ALL {
            for technical in TechnicalStatus::ALL {
                let record = record(
                    provider,
                    policy,
                    technical,
                    Some(&["worker"]),
                    Some(Some("2026-09-09T00:00:00Z")),
                );
                let original = record.clone();
                for evidence in [None, Some(true), Some(false)] {
                    assert_eq!(
                        evaluate(&record, Some("worker"), evidence),
                        policy == PolicyStatus::VerifiedAllowed && evidence == Some(false),
                        "{provider:?}, {policy:?}, {technical:?}, {evidence:?}"
                    );
                    assert_eq!(record, original);
                }
            }
        }
    }
    // In particular, generic NeedsReview evidence keeps Q-V13-04 disabled.
}

#[test]
fn execution_context_membership_is_open_exact_and_required() {
    for (allowed, requested, expected) in [
        (Some(vec!["worker"]), None, false),
        (None, Some("worker"), false),
        (Some(vec![]), Some("worker"), false),
        (Some(vec!["worker"]), Some("worker"), true),
        (Some(vec!["worker"]), Some("Worker"), false),
        (Some(vec!["worker"]), Some(" worker"), false),
        (Some(vec!["worker"]), Some("worker "), false),
        (Some(vec![" worker "]), Some("worker"), false),
        (Some(vec![" worker "]), Some(" worker "), true),
        (Some(vec!["worker"]), Some(""), false),
        (Some(vec!["worker"]), Some("FUTURE_CONTEXT_α"), false),
        (
            Some(vec!["FUTURE_CONTEXT_α"]),
            Some("FUTURE_CONTEXT_α"),
            true,
        ),
        (
            Some(vec!["FUTURE_CONTEXT_α"]),
            Some("future_context_α"),
            false,
        ),
        (Some(vec!["α/界"]), Some("α/界"), true),
        (Some(vec!["é"]), Some("e\u{301}"), false),
        (Some(vec![" \t\n"]), Some(" \t\n"), true),
        (Some(vec![" \t\n"]), Some(" "), false),
        (
            Some(vec!["worker", "other", "worker"]),
            Some("worker"),
            true,
        ),
        (
            Some(vec!["other", "worker", "worker"]),
            Some("worker"),
            true,
        ),
        (
            Some(vec!["worker", "other", "worker"]),
            Some("missing"),
            false,
        ),
        (
            Some(vec!["other", "worker", "worker"]),
            Some("missing"),
            false,
        ),
    ] {
        let record = record(
            "future/provider",
            PolicyStatus::VerifiedAllowed,
            TechnicalStatus::AuthRequired,
            allowed.as_deref(),
            Some(Some("2026-09-09T00:00:00Z")),
        );
        let original = record.clone();
        assert_eq!(
            PolicyEligibilityEvaluator::evaluate(&record, requested, Some(false)),
            expected,
            "{allowed:?}, {requested:?}"
        );
        assert_eq!(record, original);
    }
}

#[test]
fn deadline_evidence_is_required_only_for_actual_deadlines() {
    for deadline in [
        None,
        Some(None),
        Some(Some("0000-01-01T00:00:00-00:00")),
        Some(Some("2026-09-08T00:00:00Z")),
        Some(Some("9999-12-31T23:59:60+23:59")),
    ] {
        for policy in PolicyStatus::ALL {
            let record = record(
                "future/provider",
                policy,
                TechnicalStatus::Connected,
                Some(&["worker"]),
                deadline,
            );
            let original = record.clone();
            for evidence in [None, Some(true), Some(false)] {
                for requested in [None, Some("missing"), Some("worker")] {
                    assert_eq!(
                        PolicyEligibilityEvaluator::evaluate(&record, requested, evidence),
                        policy == PolicyStatus::VerifiedAllowed
                            && requested == Some("worker")
                            && (deadline.flatten().is_none() || evidence == Some(false)),
                        "{policy:?}, {deadline:?}, {evidence:?}, {requested:?}"
                    );
                    assert_eq!(record, original);
                }
            }
        }
    }
}
