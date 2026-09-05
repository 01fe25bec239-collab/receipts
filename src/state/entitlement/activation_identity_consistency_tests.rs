use super::{
    ActivationIdentityConsistencyError, ActivationIdentityFields, ActivationStateKind,
    validate_activation_identity_fields,
};

const TIERS: [Option<&str>; 7] = [
    None,
    Some(""),
    Some(" "),
    Some("free"),
    Some("enterprise.future"),
    Some(" FUTURE/tier:v999 "),
    Some("未来"),
];

#[test]
fn never_activated_without_subject_is_valid_regardless_of_tier() {
    for tier in TIERS {
        let fields = ActivationIdentityFields::new(
            ActivationStateKind::NeverActivated,
            None,
            tier.map(str::to_owned),
        );
        assert_eq!(validate_activation_identity_fields(&fields), Ok(()));
    }
}

#[test]
fn never_activated_rejects_every_present_subject_regardless_of_tier() {
    for subject in [
        String::new(),
        " ".to_owned(),
        "arbitrary".to_owned(),
        "subject".to_owned(),
        "用户-Δ-🙂".to_owned(),
        "x".repeat(1000),
    ] {
        for tier in TIERS {
            let fields = ActivationIdentityFields::new(
                ActivationStateKind::NeverActivated,
                Some(subject.clone()),
                tier.map(str::to_owned),
            );
            assert_eq!(
                validate_activation_identity_fields(&fields),
                Err(ActivationIdentityConsistencyError::NeverActivatedSubjectPresent)
            );
        }
    }
}

#[test]
fn other_states_accept_absent_and_present_subjects_regardless_of_tier() {
    for state in [
        ActivationStateKind::ActivatedKnown,
        ActivationStateKind::LoggedOut,
    ] {
        for subject in [
            None,
            Some(String::new()),
            Some(" ".to_owned()),
            Some("arbitrary".to_owned()),
            Some("subject".to_owned()),
            Some("用户".to_owned()),
            Some("x".repeat(1000)),
        ] {
            for tier in TIERS {
                let fields =
                    ActivationIdentityFields::new(state, subject.clone(), tier.map(str::to_owned));
                assert_eq!(validate_activation_identity_fields(&fields), Ok(()));
            }
        }
    }
}

#[test]
fn raw_inconsistent_construction_is_preserved_after_validation() {
    let fields = ActivationIdentityFields::new(
        ActivationStateKind::NeverActivated,
        Some("subject".to_owned()),
        Some("future".to_owned()),
    );
    let original = fields.clone();
    assert_eq!(fields.subject_id(), Some("subject"));
    assert_eq!(fields.last_known_tier_id(), Some("future"));
    let error = validate_activation_identity_fields(&fields).unwrap_err();
    assert_eq!(
        error,
        ActivationIdentityConsistencyError::NeverActivatedSubjectPresent
    );
    assert_eq!(format!("{error:?}"), "NeverActivatedSubjectPresent");
    assert_eq!(fields, original);
}
