use crate::{
    AvailabilitySignalSource, AvailabilityStateCoreError, AvailabilityStateKind,
    AvailabilityStateNonTemporalCore,
};

#[test]
fn empty_identifiers_have_field_specific_errors() {
    for (provider, model, runtime, expected) in [
        ("", None, None, AvailabilityStateCoreError::EmptyProviderId),
        (
            "provider-α",
            Some(""),
            None,
            AvailabilityStateCoreError::EmptyModelId,
        ),
        (
            "provider-α",
            None,
            Some(""),
            AvailabilityStateCoreError::EmptyRuntimeId,
        ),
    ] {
        assert_eq!(
            AvailabilityStateNonTemporalCore::try_new(
                provider.into(),
                model.map(String::from),
                runtime.map(String::from),
                AvailabilityStateKind::Available,
                None,
                None,
                None,
            ),
            Err(expected)
        );
    }
}

#[test]
fn opaque_identifiers_are_preserved_and_optional_ids_are_independent() {
    for provider in [" Opaque-ID/7 ", " \t\n", "provider-α/界/e\u{301}"] {
        for model in [None, Some(" \t\n"), Some(" model-β/界/e\u{301} ")] {
            for runtime in [None, Some(" \t\n"), Some(" runtime-γ/界/e\u{301} ")] {
                let core = AvailabilityStateNonTemporalCore::try_new(
                    provider.into(),
                    model.map(String::from),
                    runtime.map(String::from),
                    AvailabilityStateKind::Available,
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(core.provider_id(), provider);
                assert_eq!(core.model_id(), model);
                assert_eq!(core.runtime_id(), runtime);
            }
        }
    }
}

#[test]
fn all_states_sources_retry_values_and_notes_are_independent_storage() {
    let states = [
        AvailabilityStateKind::Available,
        AvailabilityStateKind::Degraded,
        AvailabilityStateKind::RateLimited,
        AvailabilityStateKind::SessionExhausted,
        AvailabilityStateKind::AuthRequired,
        AvailabilityStateKind::ProviderDown,
        AvailabilityStateKind::SafetyCheckPending,
        AvailabilityStateKind::PolicyBlocked,
        AvailabilityStateKind::Unknown,
    ];
    let sources = [
        None,
        Some(AvailabilitySignalSource::RateLimitHeader),
        Some(AvailabilitySignalSource::ExitCode),
        Some(AvailabilitySignalSource::StderrClassification),
        Some(AvailabilitySignalSource::LocalUsageView),
        Some(AvailabilitySignalSource::LatencyTrend),
        Some(AvailabilitySignalSource::Probe),
        Some(AvailabilitySignalSource::Unknown),
    ];
    for state in states {
        for source in sources {
            for retry in [None, Some(0), Some(73), Some(u64::MAX)] {
                for note in [None, Some(""), Some(" \t\n"), Some(" note-δ/界/e\u{301} ")] {
                    let core = AvailabilityStateNonTemporalCore::try_new(
                        "provider-α".into(),
                        None,
                        None,
                        state,
                        retry,
                        source,
                        note.map(String::from),
                    )
                    .unwrap();
                    assert_eq!(core.state(), state);
                    assert_eq!(core.retry_after_seconds(), retry);
                    assert_eq!(core.signal_source(), source);
                    assert_eq!(core.note(), note);
                }
            }
        }
    }
}
