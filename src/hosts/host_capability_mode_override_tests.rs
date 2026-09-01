//! Tests for the structural host capability mode override value object.

use super::*;

#[test]
fn every_source_is_accepted_and_preserved() {
    for source in HostCapabilityModeOverrideSource::ALL {
        let value = HostCapabilityModeOverride::new(source, "reason".to_owned()).unwrap();
        assert_eq!(value.source(), source);
    }
}

#[test]
fn only_an_empty_reason_is_rejected() {
    assert_eq!(
        HostCapabilityModeOverride::new(HostCapabilityModeOverrideSource::User, String::new()),
        Err(HostCapabilityModeOverrideError::EmptyReason)
    );

    for reason in ["x", " ", "   ", "\t", "\n", "ADMIN OVERRIDE", "💾"] {
        let value = HostCapabilityModeOverride::new(
            HostCapabilityModeOverrideSource::Admin,
            reason.to_owned(),
        )
        .unwrap();
        assert_eq!(value.reason(), reason);
    }
}

#[test]
fn reason_is_preserved_byte_for_byte() {
    let reason = "  Réason\t\r\n💾  ".to_owned();
    let expected = reason.clone();
    let value =
        HostCapabilityModeOverride::new(HostCapabilityModeOverrideSource::Debug, reason).unwrap();

    assert_eq!(value.reason().as_bytes(), expected.as_bytes());
}
