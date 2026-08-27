#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    RateLimited,
    SessionExhausted,
    AuthRequired,
    ProviderDown,
    Timeout,
    SandboxDenied,
    SafetyCheckPending,
    PolicyBlocked,
    RuntimeCrash,
    InvalidOutput,
    UserCancelled,
    Unknown,
}

impl FailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "RATE_LIMITED",
            Self::SessionExhausted => "SESSION_EXHAUSTED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::ProviderDown => "PROVIDER_DOWN",
            Self::Timeout => "TIMEOUT",
            Self::SandboxDenied => "SANDBOX_DENIED",
            Self::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::RuntimeCrash => "RUNTIME_CRASH",
            Self::InvalidOutput => "INVALID_OUTPUT",
            Self::UserCancelled => "USER_CANCELLED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FailureClass;

    const FROZEN: [(FailureClass, &str); 12] = [
        (FailureClass::RateLimited, "RATE_LIMITED"),
        (FailureClass::SessionExhausted, "SESSION_EXHAUSTED"),
        (FailureClass::AuthRequired, "AUTH_REQUIRED"),
        (FailureClass::ProviderDown, "PROVIDER_DOWN"),
        (FailureClass::Timeout, "TIMEOUT"),
        (FailureClass::SandboxDenied, "SANDBOX_DENIED"),
        (FailureClass::SafetyCheckPending, "SAFETY_CHECK_PENDING"),
        (FailureClass::PolicyBlocked, "POLICY_BLOCKED"),
        (FailureClass::RuntimeCrash, "RUNTIME_CRASH"),
        (FailureClass::InvalidOutput, "INVALID_OUTPUT"),
        (FailureClass::UserCancelled, "USER_CANCELLED"),
        (FailureClass::Unknown, "UNKNOWN"),
    ];

    fn assert_failure_class_exhaustive(value: FailureClass) {
        match value {
            FailureClass::RateLimited
            | FailureClass::SessionExhausted
            | FailureClass::AuthRequired
            | FailureClass::ProviderDown
            | FailureClass::Timeout
            | FailureClass::SandboxDenied
            | FailureClass::SafetyCheckPending
            | FailureClass::PolicyBlocked
            | FailureClass::RuntimeCrash
            | FailureClass::InvalidOutput
            | FailureClass::UserCancelled
            | FailureClass::Unknown => {}
        }
    }

    #[test]
    fn frozen_vocabulary_and_canonical_strings_are_exact() {
        assert_eq!(FROZEN.len(), 12);
        for (value, canonical) in FROZEN {
            assert_failure_class_exhaustive(value);
            assert_eq!(value.as_str(), canonical);
        }
    }

    #[test]
    fn values_and_canonical_strings_are_unique() {
        for (index, (value, canonical)) in FROZEN.iter().enumerate() {
            for (other_value, other_canonical) in &FROZEN[index + 1..] {
                assert_ne!(value, other_value);
                assert_ne!(canonical, other_canonical);
            }
        }
    }

    #[test]
    fn unknown_is_preserved() {
        assert_eq!(FailureClass::Unknown.as_str(), "UNKNOWN");
        assert_ne!(FailureClass::Unknown, FailureClass::RateLimited);
        assert_ne!(FailureClass::Unknown, FailureClass::ProviderDown);
        assert_ne!(FailureClass::Unknown, FailureClass::PolicyBlocked);
    }

    #[test]
    fn required_failure_classes_remain_distinct() {
        let classes = [
            FailureClass::SafetyCheckPending,
            FailureClass::PolicyBlocked,
            FailureClass::RateLimited,
            FailureClass::ProviderDown,
        ];

        for (index, class) in classes.iter().enumerate() {
            for other in &classes[index + 1..] {
                assert_ne!(class, other);
            }
        }
        assert_ne!(FailureClass::RateLimited, FailureClass::SessionExhausted);
    }

    #[test]
    fn canonical_representation_is_deterministic() {
        for (value, _) in FROZEN {
            assert_eq!(value.as_str(), value.as_str());
        }
    }
}
