//! Capsule-owned inline snapshots under BUILD-A1-ADR-REPAIRCAPSULE-INLINE-PHYSICAL-SNAPSHOT-V1-001.
//! Lexical integers have no magnitude limit, conversion, ordering, or arithmetic.

use super::CapsuleError;

fn positive(value: &str) -> bool {
    matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|b| b.is_ascii_digit())
}

/// Positive canonical ASCII decimal finding line.
///
/// Raw lexical storage cannot bypass validation:
/// ```compile_fail
/// use receipts_orchestration::capsules::RepairFindingLineV1;
/// let invalid = RepairFindingLineV1("0".into());
/// ```
/// Lexical access is immutable:
/// ```compile_fail
/// use receipts_orchestration::capsules::RepairFindingLineV1;
/// fn invalidate(line: &mut RepairFindingLineV1) { line.as_str().clear(); }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepairFindingLineV1(String);

impl RepairFindingLineV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, CapsuleError> {
        let value = value.into();
        if !positive(&value) {
            return Err(CapsuleError::InvalidRepairInteger {
                field: "finding.line",
            });
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Non-negative canonical ASCII decimal snapshot; no State semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepairContextEpochV1(String);

impl RepairContextEpochV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, CapsuleError> {
        let value = value.into();
        if !(value == "0" || positive(&value)) {
            return Err(CapsuleError::InvalidRepairInteger {
                field: "context_epoch",
            });
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical ASCII decimal attempt >= 2, explicitly supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepairAttemptNumberV1(String);

impl RepairAttemptNumberV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, CapsuleError> {
        let value = value.into();
        if !(value != "1" && positive(&value)) {
            return Err(CapsuleError::InvalidRepairInteger {
                field: "attempt_number",
            });
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical signed ASCII decimal historical exit code; negative zero is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepairExitCodeV1(String);

impl RepairExitCodeV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, CapsuleError> {
        let value = value.into();
        if !(value == "0" || positive(value.strip_prefix('-').unwrap_or(&value))) {
            return Err(CapsuleError::InvalidRepairInteger { field: "exit_code" });
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairFindingSeverityV1 {
    Info,
    Low,
    Medium,
    High,
    Critical,
}
impl RepairFindingSeverityV1 {
    pub const ALL: [Self; 5] = [
        Self::Info,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Critical,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairFindingCategoryV1 {
    ArchitectureViolation,
    ContractViolation,
    SecurityBoundaryViolation,
    WriteScopeViolation,
    UndisclosedChange,
    MissingRequiredNegativeTest,
    UnreproducibleEvidence,
    OverstatedLabel,
    TestWeakenedOrDeleted,
    AcceptanceCriterionUnmet,
    Correctness,
    ErrorHandling,
    RegressionRisk,
    Style,
    Naming,
    MinorPerformance,
    DocGap,
}
impl RepairFindingCategoryV1 {
    pub const ALL: [Self; 17] = [
        Self::ArchitectureViolation,
        Self::ContractViolation,
        Self::SecurityBoundaryViolation,
        Self::WriteScopeViolation,
        Self::UndisclosedChange,
        Self::MissingRequiredNegativeTest,
        Self::UnreproducibleEvidence,
        Self::OverstatedLabel,
        Self::TestWeakenedOrDeleted,
        Self::AcceptanceCriterionUnmet,
        Self::Correctness,
        Self::ErrorHandling,
        Self::RegressionRisk,
        Self::Style,
        Self::Naming,
        Self::MinorPerformance,
        Self::DocGap,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ArchitectureViolation => "ARCHITECTURE_VIOLATION",
            Self::ContractViolation => "CONTRACT_VIOLATION",
            Self::SecurityBoundaryViolation => "SECURITY_BOUNDARY_VIOLATION",
            Self::WriteScopeViolation => "WRITE_SCOPE_VIOLATION",
            Self::UndisclosedChange => "UNDISCLOSED_CHANGE",
            Self::MissingRequiredNegativeTest => "MISSING_REQUIRED_NEGATIVE_TEST",
            Self::UnreproducibleEvidence => "UNREPRODUCIBLE_EVIDENCE",
            Self::OverstatedLabel => "OVERSTATED_LABEL",
            Self::TestWeakenedOrDeleted => "TEST_WEAKENED_OR_DELETED",
            Self::AcceptanceCriterionUnmet => "ACCEPTANCE_CRITERION_UNMET",
            Self::Correctness => "CORRECTNESS",
            Self::ErrorHandling => "ERROR_HANDLING",
            Self::RegressionRisk => "REGRESSION_RISK",
            Self::Style => "STYLE",
            Self::Naming => "NAMING",
            Self::MinorPerformance => "MINOR_PERFORMANCE",
            Self::DocGap => "DOC_GAP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairFindingConfidenceV1 {
    High,
    Medium,
    Low,
}
impl RepairFindingConfidenceV1 {
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairFindingSourceV1 {
    LlmReview,
    StaticAnalysis,
    DependencyScan,
    Test,
    ConfigCheck,
}
impl RepairFindingSourceV1 {
    pub const ALL: [Self; 5] = [
        Self::LlmReview,
        Self::StaticAnalysis,
        Self::DependencyScan,
        Self::Test,
        Self::ConfigCheck,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LlmReview => "LLM_REVIEW",
            Self::StaticAnalysis => "STATIC_ANALYSIS",
            Self::DependencyScan => "DEPENDENCY_SCAN",
            Self::Test => "TEST",
            Self::ConfigCheck => "CONFIG_CHECK",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairCheckSourceV1 {
    WorkerExecution,
    BrokerExecution,
    ReviewExecution,
    GitProvenance,
}
impl RepairCheckSourceV1 {
    pub const ALL: [Self; 4] = [
        Self::WorkerExecution,
        Self::BrokerExecution,
        Self::ReviewExecution,
        Self::GitProvenance,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::WorkerExecution => "WORKER_EXECUTION",
            Self::BrokerExecution => "BROKER_EXECUTION",
            Self::ReviewExecution => "REVIEW_EXECUTION",
            Self::GitProvenance => "GIT_PROVENANCE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairCheckResultV1 {
    Pass,
    Fail,
    Error,
    Skipped,
    Unknown,
}
impl RepairCheckResultV1 {
    pub const ALL: [Self; 5] = [
        Self::Pass,
        Self::Fail,
        Self::Error,
        Self::Skipped,
        Self::Unknown,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Skipped => "SKIPPED",
            Self::Unknown => "UNKNOWN",
        }
    }
}
