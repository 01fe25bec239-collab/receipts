//! Typed failures at capsule construction boundaries.

use super::TaskType;
use crate::graph::GraphError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleError {
    EmptyIdentifier {
        field: &'static str,
    },
    IdentifierTooLong {
        field: &'static str,
        length: usize,
        max: usize,
    },
    RepairParentMissing,
    EmptyField {
        field: &'static str,
    },
    IntegerBelowMinimum {
        field: &'static str,
        minimum: i64,
        value: i64,
    },
    InvalidSha {
        field: &'static str,
    },
    VerificationArgvEmpty,
    VerificationArgv0Empty,
    VerificationPlanMissing {
        task_type: TaskType,
    },
    InvalidBranch,
    NonFiniteMaxCost,
}

// Reuse the integrated scalar-count bound, retaining capsule field attribution.
pub(super) fn validate_identifier(field: &'static str, value: &str) -> Result<(), CapsuleError> {
    GraphError::validate_identifier(field, value).map_err(|error| match error {
        GraphError::EmptyIdentifier { field } => CapsuleError::EmptyIdentifier { field },
        GraphError::IdentifierTooLong { field, length, max } => {
            CapsuleError::IdentifierTooLong { field, length, max }
        }
        _ => unreachable!("identifier validation only reports identifier length errors"),
    })
}

impl std::fmt::Display for CapsuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentifier { field } => write!(f, "{field} identifier is empty"),
            Self::IdentifierTooLong { field, length, max } => write!(
                f,
                "{field} has {length} Unicode scalar values; maximum is {max}"
            ),
            Self::RepairParentMissing => f.write_str("REPAIR requires parent_task_id"),
            Self::EmptyField { field } => write!(f, "{field} must not be empty"),
            Self::IntegerBelowMinimum {
                field,
                minimum,
                value,
            } => write!(f, "{field} must be >= {minimum}, got {value}"),
            Self::InvalidSha { field } => write!(
                f,
                "{field} must be exactly 40 lowercase hexadecimal characters"
            ),
            Self::VerificationArgvEmpty => {
                f.write_str("verification command argv must not be empty")
            }
            Self::VerificationArgv0Empty => {
                f.write_str("verification command argv[0] must not be empty")
            }
            Self::VerificationPlanMissing { task_type } => write!(
                f,
                "{} requires a nonempty verification plan",
                task_type.as_str()
            ),
            Self::InvalidBranch => f.write_str("branch must begin with runtime-a3/"),
            Self::NonFiniteMaxCost => f.write_str("max_cost must be finite"),
        }
    }
}

impl std::error::Error for CapsuleError {}
