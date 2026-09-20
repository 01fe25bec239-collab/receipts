use crate::{
    IntegrationDecision, IntegrationDecisionCheck as Check,
    IntegrationDecisionCheckResult as CheckResult, IntegrationDecisionNonTemporalCore as Core,
    IntegrationDecisionNullableString as Nullable, IntegrationDecisionOutcome as Outcome,
    IntegrationDecisionProvenance as Provenance, ReviewDateTimeV1,
};

fn core(outcome: Outcome) -> Core {
    let check = Check::new(
        "caller check".into(),
        CheckResult::Fail,
        Some(" evidence ".into()),
    );
    let provenance = Provenance::new(
        "task".into(),
        "a".repeat(40),
        "b".repeat(40),
        "c".repeat(40),
        Nullable::Null,
        Nullable::String(" opaque provenance ".into()),
    )
    .unwrap();
    Core::new(
        " decision ".into(),
        " request ".into(),
        outcome,
        vec![check.clone(), check],
        Some(vec![provenance.clone(), provenance]),
        Some(vec!["unmet".into(), "".into(), "unmet".into()]),
        Nullable::String(" opaque integration ".into()),
    )
    .unwrap()
}

#[test]
fn required_caller_timestamp_is_stored_directly_and_preserves_exact_spelling() {
    let long = format!("2026-09-08T00:00:00.{}Z", "1234567890".repeat(1_000));
    for input in [
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-08:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.123456789123456789Z",
        &long,
    ] {
        let timestamp = ReviewDateTimeV1::try_new(input).unwrap();
        let original_storage = timestamp.as_str().as_ptr();
        let value = IntegrationDecision::new(core(Outcome::Accept), timestamp);
        let _: &ReviewDateTimeV1 = value.decided_at();
        assert_eq!(value.decided_at().as_str().as_ptr(), original_storage);
        assert_eq!(value.decided_at().as_str(), input);
        assert_eq!(value.clone().decided_at().as_str(), input);
    }
}

#[test]
fn invalid_strings_fail_at_the_canonical_datetime_boundary() {
    for input in [
        "",
        "2026-09-08 00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:00:00",
        " 2026-09-08T00:00:00Z",
        "2026-09-08T00:00:00Z ",
    ] {
        assert!(
            ReviewDateTimeV1::try_new(input).is_err(),
            "accepted {input:?}"
        );
    }
}

#[test]
fn standalone_core_and_every_supplied_outcome_are_preserved_without_inference() {
    for outcome in Outcome::ALL {
        let supplied = core(outcome);
        assert_eq!(supplied.decision_id(), " decision ");
        assert_eq!(supplied.request_id(), " request ");
        assert_eq!(supplied.outcome(), outcome);
        assert_eq!(supplied.checks_evaluated()[0].result(), CheckResult::Fail);
        let moved_core = supplied.clone();
        let original_checks = moved_core.checks_evaluated().as_ptr();
        let value = IntegrationDecision::new(
            moved_core,
            ReviewDateTimeV1::try_new("2026-09-08T00:00:00Z").unwrap(),
        );
        let stored: &Core = value.core();
        assert_eq!(stored.checks_evaluated().as_ptr(), original_checks);
        assert_eq!(stored, &supplied);
        assert_eq!(stored.decision_id(), supplied.decision_id());
        assert_eq!(stored.request_id(), supplied.request_id());
        assert_eq!(stored.outcome(), supplied.outcome());
        assert_eq!(stored.checks_evaluated(), supplied.checks_evaluated());
        assert_eq!(stored.provenance_chain(), supplied.provenance_chain());
        assert_eq!(stored.unmet_conditions(), supplied.unmet_conditions());
        assert_eq!(stored.integration_sha(), supplied.integration_sha());
        assert_eq!(value.clone().core(), &supplied);
    }
}
