use std::cmp::Ordering;
use std::fmt;

use crate::error::StateError;

/// Canonical, unbounded non-negative State epoch value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateEpochValueV1(String);

impl StateEpochValueV1 {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn try_to_i64(&self) -> Result<i64, StateError> {
        self.0
            .parse()
            .map_err(|_| StateError::StateEpochValueOutOfI64Range {
                value: self.0.clone(),
            })
    }

    pub fn successor(&self) -> Result<Self, StateError> {
        let bytes = self.0.as_bytes();
        let mut output = Vec::new();
        let capacity = bytes.len().checked_add(1).ok_or_else(|| {
            StateError::StateEpochValueAllocationFailed {
                detail: "successor length exceeds addressable memory".to_string(),
            }
        })?;
        output.try_reserve_exact(capacity).map_err(|error| {
            StateError::StateEpochValueAllocationFailed {
                detail: error.to_string(),
            }
        })?;
        output.extend_from_slice(bytes);
        for index in (0..output.len()).rev() {
            if output[index] != b'9' {
                output[index] += 1;
                return Ok(Self(
                    String::from_utf8(output).expect("ASCII digits are UTF-8"),
                ));
            }
            output[index] = b'0';
        }
        output.insert(0, b'1');
        Ok(Self(
            String::from_utf8(output).expect("ASCII digits are UTF-8"),
        ))
    }

    fn validate(value: &str) -> Result<(), StateError> {
        let canonical = value == "0"
            || (value
                .as_bytes()
                .first()
                .is_some_and(|byte| (b'1'..=b'9').contains(byte))
                && value.as_bytes()[1..].iter().all(u8::is_ascii_digit));
        if !canonical {
            return Err(StateError::InvalidStateEpochValue {
                value: value.to_string(),
            });
        }
        Ok(())
    }

    fn parse(value: &str) -> Result<Self, StateError> {
        Self::validate(value)?;
        let mut owned = String::new();
        owned.try_reserve_exact(value.len()).map_err(|error| {
            StateError::StateEpochValueAllocationFailed {
                detail: error.to_string(),
            }
        })?;
        owned.push_str(value);
        Ok(Self(owned))
    }
}

impl TryFrom<&str> for StateEpochValueV1 {
    type Error = StateError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for StateEpochValueV1 {
    type Error = StateError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<i64> for StateEpochValueV1 {
    type Error = StateError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value < 0 {
            return Err(StateError::InvalidStateEpochValue {
                value: value.to_string(),
            });
        }
        Ok(Self(value.to_string()))
    }
}

impl Ord for StateEpochValueV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for StateEpochValueV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<i64> for StateEpochValueV1 {
    fn eq(&self, other: &i64) -> bool {
        self.try_to_i64().is_ok_and(|value| value == *other)
    }
}

impl PartialEq<StateEpochValueV1> for i64 {
    fn eq(&self, other: &StateEpochValueV1) -> bool {
        other == self
    }
}

impl fmt::Display for StateEpochValueV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
