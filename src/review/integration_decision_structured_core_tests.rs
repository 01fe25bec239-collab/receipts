use crate::{
    IntegrationDecisionCheck as Check, IntegrationDecisionCheckResult as CheckResult,
    IntegrationDecisionConstructionError as Error, IntegrationDecisionNonTemporalCore as Decision,
    IntegrationDecisionNullableString as Nullable, IntegrationDecisionOutcome as Outcome,
    IntegrationDecisionProvenance as Provenance,
};
use receipts_workspace_execution::CommitSha;

const START: &str = "0123456789abcdef0123456789abcdef01234567";
const IMPLEMENTATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REVIEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn check() -> Check {
    Check::new("check".into(), CheckResult::Pass, None)
}

fn decision(decision_id: &str, request_id: &str) -> Result<Decision, Error> {
    Decision::new(
        decision_id.into(),
        request_id.into(),
        Outcome::Accept,
        vec![check()],
        None,
        None,
        Nullable::Omitted,
    )
}

fn provenance(
    task_id: &str,
    acceptance: Nullable,
    integration: Nullable,
) -> Result<Provenance, Error> {
    Provenance::new(
        task_id.into(),
        START.into(),
        IMPLEMENTATION.into(),
        REVIEW.into(),
        acceptance,
        integration,
    )
}

#[test]
fn minimum_valid_decision() {
    let value = decision("d", "r").unwrap();
    assert_eq!(value.decision_id(), "d");
    assert_eq!(value.request_id(), "r");
    assert_eq!(value.outcome(), Outcome::Accept);
    assert_eq!(value.checks_evaluated(), &[check()]);
    assert_eq!(value.provenance_chain(), None);
    assert_eq!(value.unmet_conditions(), None);
    assert_eq!(value.integration_sha(), &Nullable::Omitted);
}

#[test]
fn closed_vocabularies_and_no_outcome_inference() {
    assert_eq!(
        Outcome::ALL.map(Outcome::as_str),
        ["ACCEPT", "REPAIR", "BLOCKED", "HUMAN_REQUIRED"]
    );
    assert_eq!(
        CheckResult::ALL.map(CheckResult::as_str),
        ["PASS", "FAIL", "NOT_APPLICABLE"]
    );
    // Every outcome/result pairing is structurally valid, without conditional unmet conditions.
    for outcome in Outcome::ALL {
        for result in CheckResult::ALL {
            let value = Decision::new(
                "d".into(),
                "r".into(),
                outcome,
                vec![Check::new("".into(), result, None)],
                None,
                None,
                Nullable::Omitted,
            )
            .unwrap();
            assert_eq!(value.outcome(), outcome);
            assert_eq!(value.checks_evaluated()[0].result(), result);
            assert_eq!(value.unmet_conditions(), None);
        }
    }
}

#[test]
fn check_strings_and_details_are_preserved() {
    for name in ["", " ", " arbitrary\n检查 "] {
        for detail in [None, Some(""), Some(" detail\n e\u{301} ")] {
            let value = Check::new(
                name.into(),
                CheckResult::NotApplicable,
                detail.map(str::to_owned),
            );
            assert_eq!(value.check(), name);
            assert_eq!(value.detail(), detail);
        }
    }
}

#[test]
fn checks_preserve_order_and_duplicates() {
    let checks = vec![
        Check::new("z".into(), CheckResult::Fail, Some("".into())),
        check(),
        check(),
    ];
    let value = Decision::new(
        "d".into(),
        "r".into(),
        Outcome::Accept,
        checks.clone(),
        None,
        None,
        Nullable::Omitted,
    )
    .unwrap();
    assert_eq!(value.checks_evaluated(), checks);
}

#[test]
fn empty_checks_rejected() {
    assert_eq!(
        Decision::new(
            "d".into(),
            "r".into(),
            Outcome::Accept,
            vec![],
            None,
            None,
            Nullable::Omitted
        ),
        Err(Error::EmptyChecksEvaluated)
    );
}

#[test]
fn identifier_character_boundaries_and_exact_preservation() {
    for character in ["a", "界", "🦀", " "] {
        for length in [0, 1, 200, 201] {
            let id = character.repeat(length);
            let d = decision(&id, "r");
            let r = decision("d", &id);
            let p = provenance(&id, Nullable::Omitted, Nullable::Omitted);
            if (1..=200).contains(&length) {
                assert_eq!(d.unwrap().decision_id(), id);
                assert_eq!(r.unwrap().request_id(), id);
                assert_eq!(p.unwrap().task_id(), id);
            } else {
                assert_eq!(d, Err(Error::InvalidIdentifier("decision_id")));
                assert_eq!(r, Err(Error::InvalidIdentifier("request_id")));
                assert_eq!(p, Err(Error::InvalidIdentifier("task_id")));
            }
        }
    }
    let exact = " Mixed e\u{301}\n界 ";
    let value = decision(exact, exact).unwrap();
    assert_eq!(value.decision_id(), exact);
    assert_eq!(value.request_id(), exact);
    assert_eq!(
        provenance(exact, Nullable::Omitted, Nullable::Omitted)
            .unwrap()
            .task_id(),
        exact
    );
}

#[test]
fn provenance_required_shas_use_canonical_immutable_type() {
    let value = provenance("t", Nullable::Null, Nullable::String("x".into())).unwrap();
    let shas: [&CommitSha; 3] = [
        value.start_sha(),
        value.implementation_sha(),
        value.review_sha(),
    ];
    assert_eq!(shas.map(CommitSha::as_str), [START, IMPLEMENTATION, REVIEW]);
}

#[test]
fn each_required_sha_rejects_complete_invalid_matrix() {
    let invalid = [
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        "abc123".into(),
        "main".into(),
        "HEAD".into(),
        "".into(),
        "é".repeat(20),
    ];
    for (index, error) in [
        Error::MalformedStartSha,
        Error::MalformedImplementationSha,
        Error::MalformedReviewSha,
    ]
    .into_iter()
    .enumerate()
    {
        for raw in &invalid {
            let mut shas = [
                START.to_owned(),
                IMPLEMENTATION.to_owned(),
                REVIEW.to_owned(),
            ];
            shas[index] = raw.clone();
            let [start, implementation, review] = shas;
            assert_eq!(
                Provenance::new(
                    "t".into(),
                    start,
                    implementation,
                    review,
                    Nullable::Omitted,
                    Nullable::Omitted
                ),
                Err(error),
                "field {index}, input {raw:?}"
            );
        }
    }
}

#[test]
fn provenance_arrays_preserve_absence_empty_order_and_duplicates() {
    let first = provenance("z", Nullable::Null, Nullable::String("".into())).unwrap();
    let second = provenance("a", Nullable::String("x".into()), Nullable::Omitted).unwrap();
    let arrays = [
        None,
        Some(vec![]),
        Some(vec![first.clone()]),
        Some(vec![first.clone(), second, first]),
    ];
    let mut decisions = Vec::new();
    for chain in arrays {
        let value = Decision::new(
            "d".into(),
            "r".into(),
            Outcome::Accept,
            vec![check()],
            chain.clone(),
            None,
            Nullable::Omitted,
        )
        .unwrap();
        assert_eq!(value.provenance_chain(), chain.as_deref());
        decisions.push(value);
    }
    assert_ne!(decisions[0], decisions[1]);
}

#[test]
fn all_nullable_states_are_distinct_and_preserved_in_each_field() {
    let states = [
        Nullable::Omitted,
        Nullable::Null,
        Nullable::String("".into()),
        Nullable::String("x".into()),
        Nullable::String("not-a-sha".into()),
        Nullable::String(START.into()),
        Nullable::String(" e\u{301}\n界 ".into()),
    ];
    let mut acceptance_items = Vec::new();
    let mut integration_items = Vec::new();
    let mut decisions = Vec::new();
    for state in &states {
        let acceptance = provenance("t", state.clone(), Nullable::Omitted).unwrap();
        let integration = provenance("t", Nullable::Omitted, state.clone()).unwrap();
        let value = Decision::new(
            "d".into(),
            "r".into(),
            Outcome::Accept,
            vec![check()],
            None,
            None,
            state.clone(),
        )
        .unwrap();
        assert_eq!(acceptance.acceptance_sha(), state);
        assert_eq!(integration.integration_sha(), state);
        assert_eq!(value.integration_sha(), state);
        acceptance_items.push(acceptance);
        integration_items.push(integration);
        decisions.push(value);
    }
    for i in 0..states.len() {
        for j in 0..states.len() {
            assert_eq!(states[i] == states[j], i == j);
            assert_eq!(acceptance_items[i] == acceptance_items[j], i == j);
            assert_eq!(integration_items[i] == integration_items[j], i == j);
            assert_eq!(decisions[i] == decisions[j], i == j);
        }
    }
}

#[test]
fn unmet_conditions_preserve_absence_empty_strings_order_and_duplicates() {
    let arrays = [
        None,
        Some(vec![]),
        Some(vec!["".into()]),
        Some(vec!["z".into(), "".into(), " a\n界 ".into(), "z".into()]),
    ];
    let mut decisions = Vec::new();
    for conditions in arrays {
        let value = Decision::new(
            "d".into(),
            "r".into(),
            Outcome::Accept,
            vec![check()],
            None,
            conditions.clone(),
            Nullable::Omitted,
        )
        .unwrap();
        assert_eq!(value.unmet_conditions(), conditions.as_deref());
        decisions.push(value);
    }
    assert_ne!(decisions[0], decisions[1]);
}

#[test]
fn full_record_is_deterministic_and_clone_preserves_validity() {
    let make = || {
        Decision::new(
            "decision".into(),
            "request".into(),
            Outcome::HumanRequired,
            vec![check(), check()],
            Some(vec![
                provenance("task", Nullable::Null, Nullable::String("x".into())).unwrap(),
            ]),
            Some(vec!["".into(), "condition".into()]),
            Nullable::String(" opaque ".into()),
        )
        .unwrap()
    };
    let first = make();
    for _ in 0..10 {
        assert_eq!(first, make());
        assert_eq!(first, first.clone());
    }
}
