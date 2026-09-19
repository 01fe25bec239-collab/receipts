use crate::policy_eligibility::{ModelRoutingDateTimeV1, ModelRoutingDateTimeV1Error};
use crate::registry::{EmptyIdentifier, ProviderId};
use crate::{QuotaScope, QuotaStateRequiredCore};

fn core(provider: &str, scope: QuotaScope, observed: &str) -> QuotaStateRequiredCore {
    QuotaStateRequiredCore::new(
        ProviderId::try_new(provider.into()).unwrap(),
        scope,
        ModelRoutingDateTimeV1::try_new(observed.into()).unwrap(),
    )
}

#[test]
fn all_required_values_are_stored_unchanged() {
    let provider = ProviderId::try_new("provider-a".into()).unwrap();
    let observed = ModelRoutingDateTimeV1::try_new("2026-09-19T12:34:56Z".into()).unwrap();
    let value =
        QuotaStateRequiredCore::new(provider.clone(), QuotaScope::Session, observed.clone());
    assert_eq!(value.provider_id(), &provider);
    assert_eq!(value.quota_scope(), QuotaScope::Session);
    assert_eq!(value.observed_at(), &observed);
}

#[test]
fn all_scopes_have_exact_strings_and_survive_without_model_identity() {
    let scopes = [
        QuotaScope::Provider,
        QuotaScope::Model,
        QuotaScope::Session,
        QuotaScope::Unknown,
    ];
    assert_eq!(QuotaScope::ALL, scopes);
    for (scope, text) in scopes
        .into_iter()
        .zip(["PROVIDER", "MODEL", "SESSION", "UNKNOWN"])
    {
        assert_eq!(scope.as_str(), text);
        assert_eq!(
            core("provider-a", scope, "2026-09-19T12:34:56Z").quota_scope(),
            scope
        );
    }
}

#[test]
fn provider_validation_is_canonical_and_composition_is_exact() {
    assert_eq!(ProviderId::try_new("".into()), Err(EmptyIdentifier));
    for provider in ["provider-a", "Provider-A", " provider ", "提供者-α", " "] {
        assert_eq!(
            core(provider, QuotaScope::Unknown, "2026-09-19T12:34:56Z")
                .provider_id()
                .as_str(),
            provider
        );
    }
}

#[test]
fn observed_at_preserves_accepted_lexical_bytes() {
    let long_fraction = format!("2026-09-19T12:34:56.{}Z", "1234567890".repeat(1000));
    for observed in [
        "2026-09-19T12:34:56Z",
        "2026-09-19t12:34:56z",
        "2026-09-19T12:34:56+05:30",
        "2026-09-19T12:34:56-07:00",
        "2026-09-19T12:34:56-00:00",
        "2026-09-19T12:34:56.123Z",
        "2026-09-19T12:34:60Z",
        &long_fraction,
    ] {
        assert_eq!(
            core("provider-a", QuotaScope::Unknown, observed)
                .observed_at()
                .as_str()
                .as_bytes(),
            observed.as_bytes()
        );
    }
}

#[test]
fn invalid_datetimes_are_rejected_at_the_canonical_boundary() {
    for invalid in [
        "2026-13-19T12:34:56Z",
        "2026-02-30T12:34:56Z",
        "2026-09-19T24:34:56Z",
        "2026-09-19T12:60:56Z",
        "2026-09-19T12:34:61Z",
        "2026-09-19T12:34:56",
        "2026-09-19T12:34:56.Z",
        "2026-09-19T12:34:56.1aZ",
        "2026-09-19T12:34:56+0530",
        "2026-09-19T12:34:56+24:00",
        "2026-09-19T12:34:56-00:60",
        " 2026-09-19T12:34:56Z",
        "2026-09-19T12:34:56Z ",
        "2026/09-19T12:34:56Z",
        "2026-09-19 12:34:56Z",
        "2026-09-19T12-34:56Z",
    ] {
        assert_eq!(
            ModelRoutingDateTimeV1::try_new(invalid.into()),
            Err(ModelRoutingDateTimeV1Error),
            "{invalid}"
        );
    }
}

#[test]
fn clone_and_equality_are_structural_and_lexical() {
    let original = core("provider-a", QuotaScope::Unknown, "2026-09-19T12:34:56Z");
    assert_eq!(original.clone(), original);
    assert_eq!(
        core("provider-a", QuotaScope::Unknown, "2026-09-19T12:34:56Z"),
        original
    );
    for different in [
        core("Provider-A", QuotaScope::Unknown, "2026-09-19T12:34:56Z"),
        core("provider-a", QuotaScope::Provider, "2026-09-19T12:34:56Z"),
        core("provider-a", QuotaScope::Unknown, "2026-09-19t12:34:56z"),
        core(
            "provider-a",
            QuotaScope::Unknown,
            "2026-09-19T12:34:56+00:00",
        ),
    ] {
        assert_ne!(original, different);
    }
}
