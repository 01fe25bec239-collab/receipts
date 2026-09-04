//! Closed vocabularies from the frozen `A4Review.schema.json`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewVerdict {
    Pass,
    PassWithNonblockingFindings,
    Reject,
}

impl A4ReviewVerdict {
    pub const ALL: [Self; 3] = [Self::Pass, Self::PassWithNonblockingFindings, Self::Reject];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::PassWithNonblockingFindings => "PASS_WITH_NONBLOCKING_FINDINGS",
            Self::Reject => "REJECT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewFindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl A4ReviewFindingSeverity {
    pub const ALL: [Self; 5] = [
        Self::Info,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Critical,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewFindingCategory {
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

impl A4ReviewFindingCategory {
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

    pub const fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewFindingConfidence {
    High,
    Medium,
    Low,
}

impl A4ReviewFindingConfidence {
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewFindingSource {
    LlmReview,
    StaticAnalysis,
    DependencyScan,
    Test,
    ConfigCheck,
}

impl A4ReviewFindingSource {
    pub const ALL: [Self; 5] = [
        Self::LlmReview,
        Self::StaticAnalysis,
        Self::DependencyScan,
        Self::Test,
        Self::ConfigCheck,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LlmReview => "LLM_REVIEW",
            Self::StaticAnalysis => "STATIC_ANALYSIS",
            Self::DependencyScan => "DEPENDENCY_SCAN",
            Self::Test => "TEST",
            Self::ConfigCheck => "CONFIG_CHECK",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewDimension {
    ScopeCompliance,
    ContractCompliance,
    ArchitectureCompliance,
    Correctness,
    ErrorHandling,
    SecurityTrustBoundaries,
    TestAdequacy,
    NegativeTests,
    RegressionRisk,
    WriteScope,
    UndisclosedChanges,
    EvidenceAccuracy,
}

impl A4ReviewDimension {
    pub const ALL: [Self; 12] = [
        Self::ScopeCompliance,
        Self::ContractCompliance,
        Self::ArchitectureCompliance,
        Self::Correctness,
        Self::ErrorHandling,
        Self::SecurityTrustBoundaries,
        Self::TestAdequacy,
        Self::NegativeTests,
        Self::RegressionRisk,
        Self::WriteScope,
        Self::UndisclosedChanges,
        Self::EvidenceAccuracy,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScopeCompliance => "SCOPE_COMPLIANCE",
            Self::ContractCompliance => "CONTRACT_COMPLIANCE",
            Self::ArchitectureCompliance => "ARCHITECTURE_COMPLIANCE",
            Self::Correctness => "CORRECTNESS",
            Self::ErrorHandling => "ERROR_HANDLING",
            Self::SecurityTrustBoundaries => "SECURITY_TRUST_BOUNDARIES",
            Self::TestAdequacy => "TEST_ADEQUACY",
            Self::NegativeTests => "NEGATIVE_TESTS",
            Self::RegressionRisk => "REGRESSION_RISK",
            Self::WriteScope => "WRITE_SCOPE",
            Self::UndisclosedChanges => "UNDISCLOSED_CHANGES",
            Self::EvidenceAccuracy => "EVIDENCE_ACCURACY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewDimensionAssessment {
    Satisfied,
    NotSatisfied,
    NotApplicable,
}

impl A4ReviewDimensionAssessment {
    pub const ALL: [Self; 3] = [Self::Satisfied, Self::NotSatisfied, Self::NotApplicable];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "SATISFIED",
            Self::NotSatisfied => "NOT_SATISFIED",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewRecommendedAction {
    Accept,
    AcceptWithFindingsLogged,
    RepairRequired,
    EscalateToA1,
}

impl A4ReviewRecommendedAction {
    pub const ALL: [Self; 4] = [
        Self::Accept,
        Self::AcceptWithFindingsLogged,
        Self::RepairRequired,
        Self::EscalateToA1,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::AcceptWithFindingsLogged => "ACCEPT_WITH_FINDINGS_LOGGED",
            Self::RepairRequired => "REPAIR_REQUIRED",
            Self::EscalateToA1 => "ESCALATE_TO_A1",
        }
    }
}
