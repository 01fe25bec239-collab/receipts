use crate::policy_eligibility::{ModelRoutingDateTimeV1, ModelRoutingDateTimeV1Error};
use crate::{
    AvailabilitySignalSource, AvailabilityState, AvailabilityStateKind,
    AvailabilityStateNonTemporalCore,
};

fn core(state: AvailabilityStateKind) -> AvailabilityStateNonTemporalCore {
    AvailabilityStateNonTemporalCore::try_new(
        "provider-α".into(),
        None,
        None,
        state,
        None,
        None,
        None,
    )
    .unwrap()
}

fn timestamp(value: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(value.into()).unwrap()
}

#[test]
fn complete_composition_preserves_both_supplied_values() {
    let core = core(AvailabilityStateKind::Available);
    let observed_at = timestamp("2026-09-19T12:34:56Z");
    let state = AvailabilityState::new(core.clone(), observed_at.clone());
    assert_eq!(state.core(), &core);
    assert_eq!(state.observed_at(), &observed_at);
}

#[test]
fn accepted_timestamp_spellings_are_preserved_exactly() {
    let core = core(AvailabilityStateKind::Unknown);
    for text in [
        "2026-09-19T12:34:56Z",
        "2026-09-19t12:34:56z",
        "2026-09-19T12:34:56+05:30",
        "2026-09-19T12:34:56-07:00",
        "2026-09-19T12:34:56-00:00",
        "2026-09-19T12:34:56.1200Z",
        "2026-09-19t12:34:56.123456789012345678901234567890z",
        "2016-12-31T23:59:60Z",
    ] {
        let state = AvailabilityState::new(core.clone(), timestamp(text));
        assert_eq!(state.observed_at().as_str(), text);
        assert_eq!(state.core(), &core);
        assert_eq!(state.core().state(), AvailabilityStateKind::Unknown);
    }
}

#[test]
fn invalid_raw_timestamps_are_rejected_at_the_canonical_boundary() {
    for text in [
        "2026-13-19T12:34:56Z",
        "2026-02-29T12:34:56Z",
        "2026-09-19T24:34:56Z",
        "2026-09-19T12:60:56Z",
        "2026-09-19T12:34:61Z",
        "2026-09-19T12:34:56",
        "2026-09-19T12:34:56.Z",
        "2026-09-19T12:34:56+0530",
        "2026-09-19T12:34:56+24:00",
        "2026-09-19T12:34:56+05:60",
        " 2026-09-19T12:34:56Z",
        "2026-09-19T12:34:56Z ",
        "2026/09/19T12:34:56Z",
        "2026-09-19 12:34:56Z",
    ] {
        assert_eq!(
            ModelRoutingDateTimeV1::try_new(text.into()),
            Err(ModelRoutingDateTimeV1Error),
            "{text:?}"
        );
    }
}

#[test]
fn every_state_and_signal_source_is_independent_of_observation_time() {
    for kind in AvailabilityStateKind::ALL {
        for source in AvailabilitySignalSource::ALL {
            let core = AvailabilityStateNonTemporalCore::try_new(
                "provider-α".into(),
                None,
                None,
                kind,
                Some(u64::MAX),
                Some(source),
                None,
            )
            .unwrap();
            // Old-looking and new-looking evidence is stored without classification.
            for text in ["0000-01-01T00:00:00-00:00", "9999-12-31T23:59:60+23:59"] {
                let state = AvailabilityState::new(core.clone(), timestamp(text));
                assert_eq!(state.core(), &core);
                assert_eq!(state.core().state(), kind);
                assert_eq!(state.core().signal_source(), Some(source));
                assert_eq!(state.observed_at().as_str(), text);
            }
        }
    }
}

#[test]
fn optional_core_fields_and_opaque_identifiers_survive_composition() {
    for provider in [
        "Provider-A",
        "provider-a",
        " \t\n",
        " Provider-α/界/e\u{301} ",
    ] {
        for model in [None, Some(" \t\n"), Some(" Model-β/界/e\u{301} ")] {
            for runtime in [None, Some(" \t\n"), Some(" Runtime-γ/界/é ")] {
                for retry in [None, Some(0), Some(73), Some(u64::MAX)] {
                    for source in [None, Some(AvailabilitySignalSource::Unknown)] {
                        for note in [None, Some(""), Some(" \t\n"), Some(" Note-δ/界/e\u{301} ")]
                        {
                            let core = AvailabilityStateNonTemporalCore::try_new(
                                provider.into(),
                                model.map(String::from),
                                runtime.map(String::from),
                                AvailabilityStateKind::Unknown,
                                retry,
                                source,
                                note.map(String::from),
                            )
                            .unwrap();
                            let state = AvailabilityState::new(
                                core.clone(),
                                timestamp("2026-09-19T12:34:56Z"),
                            );
                            assert_eq!(state.core(), &core);
                            assert_eq!(state.core().provider_id(), provider);
                            assert_eq!(state.core().model_id(), model);
                            assert_eq!(state.core().runtime_id(), runtime);
                            assert_eq!(state.core().retry_after_seconds(), retry);
                            assert_eq!(state.core().signal_source(), source);
                            assert_eq!(state.core().note(), note);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn clone_and_equality_are_structural_and_lexical() {
    let core = core(AvailabilityStateKind::Available);
    let state = AvailabilityState::new(core.clone(), timestamp("2026-09-19T12:34:56Z"));
    assert_eq!(state, state.clone());
    assert_eq!(
        state,
        AvailabilityState::new(core.clone(), timestamp("2026-09-19T12:34:56Z"))
    );
    for text in [
        "2026-09-19t12:34:56z",
        "2026-09-19T12:34:56+00:00",
        "0000-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
    ] {
        let other = AvailabilityState::new(core.clone(), timestamp(text));
        assert_eq!(other.core(), state.core());
        assert_eq!(other.observed_at().as_str(), text);
        assert_ne!(state, other);
    }
    assert_ne!(
        state,
        AvailabilityState::new(
            self::core(AvailabilityStateKind::Degraded),
            state.observed_at().clone()
        )
    );
}
