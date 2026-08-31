#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAuthStatus {
    Connected,
    AuthRequired,
    Expired,
    Unknown,
}

impl RuntimeAuthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "CONNECTED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::Expired => "EXPIRED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeAuthStatus;

    const FROZEN: [(RuntimeAuthStatus, &str); 4] = [
        (RuntimeAuthStatus::Connected, "CONNECTED"),
        (RuntimeAuthStatus::AuthRequired, "AUTH_REQUIRED"),
        (RuntimeAuthStatus::Expired, "EXPIRED"),
        (RuntimeAuthStatus::Unknown, "UNKNOWN"),
    ];

    fn assert_runtime_auth_status_exhaustive(value: RuntimeAuthStatus) {
        match value {
            RuntimeAuthStatus::Connected
            | RuntimeAuthStatus::AuthRequired
            | RuntimeAuthStatus::Expired
            | RuntimeAuthStatus::Unknown => {}
        }
    }

    #[test]
    fn frozen_vocabulary_and_canonical_strings_are_exact() {
        assert_eq!(FROZEN.len(), 4);
        for (value, canonical) in FROZEN {
            assert_runtime_auth_status_exhaustive(value);
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
    fn unknown_is_first_class() {
        assert_eq!(RuntimeAuthStatus::Unknown.as_str(), "UNKNOWN");
        assert_ne!(RuntimeAuthStatus::Unknown, RuntimeAuthStatus::Connected);
        assert_ne!(RuntimeAuthStatus::Unknown, RuntimeAuthStatus::AuthRequired);
        assert_ne!(RuntimeAuthStatus::Unknown, RuntimeAuthStatus::Expired);
    }
}
