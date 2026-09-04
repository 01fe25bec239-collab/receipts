use super::vocabulary::ActivationStateKind;

/// Non-temporal identity fields from activation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationIdentityFields {
    activation_state: ActivationStateKind,
    subject_id: Option<String>,
    last_known_tier_id: Option<String>,
}

impl ActivationIdentityFields {
    pub fn new(
        activation_state: ActivationStateKind,
        subject_id: Option<String>,
        last_known_tier_id: Option<String>,
    ) -> Self {
        Self {
            activation_state,
            subject_id,
            last_known_tier_id,
        }
    }

    pub fn activation_state(&self) -> ActivationStateKind {
        self.activation_state
    }

    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    pub fn last_known_tier_id(&self) -> Option<&str> {
        self.last_known_tier_id.as_deref()
    }
}
