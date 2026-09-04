use super::{ActivationIdentityFields, ActivationStateKind};

/// Consistency failure for activation identity fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationIdentityConsistencyError {
    NeverActivatedSubjectPresent,
}

/// Checks that a never-activated identity has no subject, without changing it.
pub fn validate_activation_identity_fields(
    fields: &ActivationIdentityFields,
) -> Result<(), ActivationIdentityConsistencyError> {
    match fields.activation_state() {
        ActivationStateKind::NeverActivated => {
            if fields.subject_id().is_some() {
                Err(ActivationIdentityConsistencyError::NeverActivatedSubjectPresent)
            } else {
                Ok(())
            }
        }
        ActivationStateKind::ActivatedKnown => Ok(()),
        ActivationStateKind::LoggedOut => Ok(()),
    }
}
