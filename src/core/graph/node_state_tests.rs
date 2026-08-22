//! Deterministic coverage for the frozen node-state vocabulary slice:
//! exact vocabulary size, canonical wire strings, strict round-trip parsing,
//! explicit rejection of malformed and legacy inputs, and parse determinism.
//!
//! No test here asserts or implies transition legality: this slice defines
//! vocabulary and string conversion only.

use std::collections::HashSet;

use crate::node_state::GraphNodeState;

/// The frozen vocabulary, paired with its byte-for-byte canonical wire
/// spelling, in `ALL` listing order.
const CANONICAL: [(GraphNodeState, &str); 15] = [
    (GraphNodeState::Planned, "PLANNED"),
    (GraphNodeState::Ready, "READY"),
    (GraphNodeState::Admitted, "ADMITTED"),
    (GraphNodeState::Dispatched, "DISPATCHED"),
    (GraphNodeState::Running, "RUNNING"),
    (GraphNodeState::AwaitingReview, "AWAITING_REVIEW"),
    (GraphNodeState::Passed, "PASSED"),
    (GraphNodeState::Rejected, "REJECTED"),
    (GraphNodeState::Repairing, "REPAIRING"),
    (GraphNodeState::Accepted, "ACCEPTED"),
    (GraphNodeState::Integrated, "INTEGRATED"),
    (GraphNodeState::Blocked, "BLOCKED"),
    (GraphNodeState::LockedRequiresPro, "LOCKED_REQUIRES_PRO"),
    (GraphNodeState::Cancelled, "CANCELLED"),
    (GraphNodeState::HumanRequired, "HUMAN_REQUIRED"),
];

fn assert_rejected(input: &str) {
    let error = GraphNodeState::parse(input)
        .expect_err("input outside the frozen vocabulary must be rejected");
    // The offending input is preserved verbatim: never trimmed or normalized.
    assert_eq!(error.input(), input);
}

#[test]
fn exactly_fifteen_canonical_states_exist() {
    assert_eq!(GraphNodeState::ALL.len(), 15);
    assert_eq!(CANONICAL.len(), 15);

    // `ALL` covers exactly the frozen variants, each exactly once.
    for (listed, expected) in GraphNodeState::ALL.iter().zip(CANONICAL.iter()) {
        assert_eq!(listed, &expected.0);
    }

    // All fifteen canonical spellings are pairwise distinct, so no two
    // vocabulary entries share a wire string.
    let spellings: HashSet<&str> = CANONICAL.iter().map(|(_, text)| *text).collect();
    assert_eq!(spellings.len(), 15);
}

#[test]
fn every_state_emits_its_exact_frozen_wire_string() {
    for (state, expected) in CANONICAL.iter() {
        assert_eq!(state.as_str(), *expected);
    }
}

#[test]
fn every_state_round_trips_through_strict_parsing() {
    for (state, _) in CANONICAL.iter() {
        let parsed = GraphNodeState::parse(state.as_str()).expect("canonical form must parse");
        assert_eq!(parsed, *state);
        // And the round-trip is stable across generations.
        assert_eq!(parsed.as_str(), state.as_str());
    }
}

#[test]
fn lowercase_input_is_rejected() {
    assert_rejected("planned");
}

#[test]
fn mixed_or_lower_case_forms_are_rejected_for_every_state() {
    for (state, canonical) in CANONICAL.iter() {
        let _ = state;
        assert_rejected(&canonical.to_lowercase());
        // Deliberately mangled mixed casing per state: leading and trailing
        // letters lowered (`PLANNED` -> `pLanneD`) are both non-canonical.
        let mut leading_lower = (*canonical).to_string();
        if let Some(first) = leading_lower.get_mut(..1) {
            first.make_ascii_lowercase();
        }
        assert_rejected(&leading_lower);

        let mut trailing_lower = (*canonical).to_string();
        let last_index = trailing_lower.len() - 1;
        if let Some(last) = trailing_lower.get_mut(last_index..) {
            last.make_ascii_lowercase();
        }
        assert_rejected(&trailing_lower);
    }
}

#[test]
fn whitespace_prefixed_and_suffixed_inputs_are_rejected() {
    assert_rejected(" PLANNED");
    assert_rejected("PLANNED ");
    assert_rejected("  READY\t");
    assert_rejected("\nREADY\n");
    assert_rejected(" LOCKED_REQUIRES_PRO ");
}

#[test]
fn empty_string_is_rejected() {
    assert_rejected("");
}

#[test]
fn arbitrary_unknown_states_are_rejected() {
    assert_rejected("TELEPORTED");
    assert_rejected("PLANNED_PLANNED");
    assert_rejected("PLANN ED");
    assert_rejected("READY?");
}

#[test]
fn legacy_in_progress_is_rejected() {
    assert_rejected("IN_PROGRESS");
}

#[test]
fn legacy_review_passed_is_rejected() {
    assert_rejected("REVIEW_PASSED");
}

#[test]
fn legacy_review_rejected_is_rejected() {
    assert_rejected("REVIEW_REJECTED");
}

#[test]
fn repeated_parsing_of_identical_input_is_deterministic() {
    for (state, canonical) in CANONICAL.iter() {
        let first = GraphNodeState::parse(canonical);
        let second = GraphNodeState::parse(canonical);
        assert_eq!(first, second);
        assert_eq!(first, Ok(*state));
    }

    let rejects = [
        "",
        "planned",
        " Planned ",
        "IN_PROGRESS",
        "REVIEW_PASSED",
        "REVIEW_REJECTED",
        "TELEPORTED",
    ];
    for reject in rejects {
        let first = GraphNodeState::parse(reject);
        let second = GraphNodeState::parse(reject);
        assert_eq!(first, second);
        assert!(first.is_err());
    }
}
