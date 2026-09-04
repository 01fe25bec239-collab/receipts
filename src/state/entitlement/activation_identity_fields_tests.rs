use super::{ActivationIdentityFields, ActivationStateKind};

#[test]
fn every_activation_state_is_preserved() {
    let never_activated =
        ActivationIdentityFields::new(ActivationStateKind::NeverActivated, None, None);
    let activated_known =
        ActivationIdentityFields::new(ActivationStateKind::ActivatedKnown, None, None);
    let logged_out = ActivationIdentityFields::new(ActivationStateKind::LoggedOut, None, None);

    assert_eq!(
        never_activated.activation_state(),
        ActivationStateKind::NeverActivated
    );
    assert_eq!(
        activated_known.activation_state(),
        ActivationStateKind::ActivatedKnown
    );
    assert_eq!(
        logged_out.activation_state(),
        ActivationStateKind::LoggedOut
    );
}

#[test]
fn subject_id_accepts_and_preserves_every_schema_string_shape() {
    let cases = [
        None,
        Some(String::new()),
        Some(" ".to_owned()),
        Some("用户-Δ-🙂".to_owned()),
        Some("x".repeat(201)),
    ];

    for subject_id in cases {
        let expected = subject_id.clone();
        let fields =
            ActivationIdentityFields::new(ActivationStateKind::ActivatedKnown, subject_id, None);
        assert_eq!(fields.subject_id(), expected.as_deref());
    }
}

#[test]
fn last_known_tier_accepts_and_preserves_every_schema_string_shape() {
    for last_known_tier_id in [
        None,
        Some(String::new()),
        Some(" ".to_owned()),
        Some("enterprise.future".to_owned()),
        Some(" FUTURE/tier:v999 ".to_owned()),
    ] {
        let expected = last_known_tier_id.clone();
        let fields = ActivationIdentityFields::new(
            ActivationStateKind::ActivatedKnown,
            None,
            last_known_tier_id,
        );
        assert_eq!(fields.last_known_tier_id(), expected.as_deref());
    }
}

#[test]
fn empty_subject_and_tier_are_accepted_together() {
    let fields = ActivationIdentityFields::new(
        ActivationStateKind::ActivatedKnown,
        Some(String::new()),
        Some(String::new()),
    );

    assert_eq!(fields.subject_id(), Some(""));
    assert_eq!(fields.last_known_tier_id(), Some(""));
}

#[test]
fn unusual_cross_field_combination_is_stored_without_resolution() {
    let fields = ActivationIdentityFields::new(
        ActivationStateKind::NeverActivated,
        Some("subject".to_owned()),
        Some("future".to_owned()),
    );

    assert_eq!(
        fields.activation_state(),
        ActivationStateKind::NeverActivated
    );
    assert_eq!(fields.subject_id(), Some("subject"));
    assert_eq!(fields.last_known_tier_id(), Some("future"));
}
