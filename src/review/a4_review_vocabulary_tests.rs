//! Behavioral contract tests; exhaustive matches also reject unlisted variants.

use super::*;
use std::fmt::Debug;

fn assert_closed<T: Copy + Debug + Eq, const N: usize>(
    all: [T; N],
    expected: [T; N],
    strings: [&str; N],
    as_str: impl Fn(T) -> &'static str,
    schema_index: impl Fn(T) -> usize,
) {
    assert_eq!(all.len(), N);
    assert_eq!(all, expected);
    assert_eq!(all.map(&as_str), strings);
    for (index, value) in all.into_iter().enumerate() {
        assert_eq!(schema_index(value), index);
        for other in &all[index + 1..] {
            assert_ne!(value, *other);
            assert_ne!(as_str(value), as_str(*other));
        }
    }
}

#[test]
fn a4_review_verdict_matches_frozen_schema() {
    use A4ReviewVerdict::*;

    assert_closed(
        A4ReviewVerdict::ALL,
        [Pass, PassWithNonblockingFindings, Reject],
        ["PASS", "PASS_WITH_NONBLOCKING_FINDINGS", "REJECT"],
        A4ReviewVerdict::as_str,
        |value| match value {
            Pass => 0,
            PassWithNonblockingFindings => 1,
            Reject => 2,
        },
    );
}

#[test]
fn a4_review_finding_severity_matches_frozen_schema() {
    use A4ReviewFindingSeverity::*;

    assert_closed(
        A4ReviewFindingSeverity::ALL,
        [Info, Low, Medium, High, Critical],
        ["INFO", "LOW", "MEDIUM", "HIGH", "CRITICAL"],
        A4ReviewFindingSeverity::as_str,
        |value| match value {
            Info => 0,
            Low => 1,
            Medium => 2,
            High => 3,
            Critical => 4,
        },
    );
}

#[test]
fn a4_review_finding_category_matches_frozen_schema() {
    use A4ReviewFindingCategory::*;

    assert_closed(
        A4ReviewFindingCategory::ALL,
        [
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
        ],
        [
            "ARCHITECTURE_VIOLATION",
            "CONTRACT_VIOLATION",
            "SECURITY_BOUNDARY_VIOLATION",
            "WRITE_SCOPE_VIOLATION",
            "UNDISCLOSED_CHANGE",
            "MISSING_REQUIRED_NEGATIVE_TEST",
            "UNREPRODUCIBLE_EVIDENCE",
            "OVERSTATED_LABEL",
            "TEST_WEAKENED_OR_DELETED",
            "ACCEPTANCE_CRITERION_UNMET",
            "CORRECTNESS",
            "ERROR_HANDLING",
            "REGRESSION_RISK",
            "STYLE",
            "NAMING",
            "MINOR_PERFORMANCE",
            "DOC_GAP",
        ],
        A4ReviewFindingCategory::as_str,
        |value| match value {
            ArchitectureViolation => 0,
            ContractViolation => 1,
            SecurityBoundaryViolation => 2,
            WriteScopeViolation => 3,
            UndisclosedChange => 4,
            MissingRequiredNegativeTest => 5,
            UnreproducibleEvidence => 6,
            OverstatedLabel => 7,
            TestWeakenedOrDeleted => 8,
            AcceptanceCriterionUnmet => 9,
            Correctness => 10,
            ErrorHandling => 11,
            RegressionRisk => 12,
            Style => 13,
            Naming => 14,
            MinorPerformance => 15,
            DocGap => 16,
        },
    );
}

#[test]
fn a4_review_finding_confidence_matches_frozen_schema() {
    use A4ReviewFindingConfidence::*;

    assert_closed(
        A4ReviewFindingConfidence::ALL,
        [High, Medium, Low],
        ["HIGH", "MEDIUM", "LOW"],
        A4ReviewFindingConfidence::as_str,
        |value| match value {
            High => 0,
            Medium => 1,
            Low => 2,
        },
    );
}

#[test]
fn a4_review_finding_source_matches_frozen_schema() {
    use A4ReviewFindingSource::*;

    assert_closed(
        A4ReviewFindingSource::ALL,
        [LlmReview, StaticAnalysis, DependencyScan, Test, ConfigCheck],
        [
            "LLM_REVIEW",
            "STATIC_ANALYSIS",
            "DEPENDENCY_SCAN",
            "TEST",
            "CONFIG_CHECK",
        ],
        A4ReviewFindingSource::as_str,
        |value| match value {
            LlmReview => 0,
            StaticAnalysis => 1,
            DependencyScan => 2,
            Test => 3,
            ConfigCheck => 4,
        },
    );
}

#[test]
fn a4_review_dimension_matches_frozen_schema() {
    use A4ReviewDimension::*;

    assert_closed(
        A4ReviewDimension::ALL,
        [
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
        ],
        [
            "SCOPE_COMPLIANCE",
            "CONTRACT_COMPLIANCE",
            "ARCHITECTURE_COMPLIANCE",
            "CORRECTNESS",
            "ERROR_HANDLING",
            "SECURITY_TRUST_BOUNDARIES",
            "TEST_ADEQUACY",
            "NEGATIVE_TESTS",
            "REGRESSION_RISK",
            "WRITE_SCOPE",
            "UNDISCLOSED_CHANGES",
            "EVIDENCE_ACCURACY",
        ],
        A4ReviewDimension::as_str,
        |value| match value {
            ScopeCompliance => 0,
            ContractCompliance => 1,
            ArchitectureCompliance => 2,
            Correctness => 3,
            ErrorHandling => 4,
            SecurityTrustBoundaries => 5,
            TestAdequacy => 6,
            NegativeTests => 7,
            RegressionRisk => 8,
            WriteScope => 9,
            UndisclosedChanges => 10,
            EvidenceAccuracy => 11,
        },
    );
}

#[test]
fn a4_review_dimension_assessment_matches_frozen_schema() {
    use A4ReviewDimensionAssessment::*;

    assert_closed(
        A4ReviewDimensionAssessment::ALL,
        [Satisfied, NotSatisfied, NotApplicable],
        ["SATISFIED", "NOT_SATISFIED", "NOT_APPLICABLE"],
        A4ReviewDimensionAssessment::as_str,
        |value| match value {
            Satisfied => 0,
            NotSatisfied => 1,
            NotApplicable => 2,
        },
    );
}

#[test]
fn a4_review_recommended_action_matches_frozen_schema() {
    use A4ReviewRecommendedAction::*;

    assert_closed(
        A4ReviewRecommendedAction::ALL,
        [
            Accept,
            AcceptWithFindingsLogged,
            RepairRequired,
            EscalateToA1,
        ],
        [
            "ACCEPT",
            "ACCEPT_WITH_FINDINGS_LOGGED",
            "REPAIR_REQUIRED",
            "ESCALATE_TO_A1",
        ],
        A4ReviewRecommendedAction::as_str,
        |value| match value {
            Accept => 0,
            AcceptWithFindingsLogged => 1,
            RepairRequired => 2,
            EscalateToA1 => 3,
        },
    );
}

#[test]
fn combined_closed_value_count_is_52() {
    const COMBINED_CLOSED_VALUE_COUNT: usize = A4ReviewVerdict::ALL.len()
        + A4ReviewFindingSeverity::ALL.len()
        + A4ReviewFindingCategory::ALL.len()
        + A4ReviewFindingConfidence::ALL.len()
        + A4ReviewFindingSource::ALL.len()
        + A4ReviewDimension::ALL.len()
        + A4ReviewDimensionAssessment::ALL.len()
        + A4ReviewRecommendedAction::ALL.len();
    assert_eq!(COMBINED_CLOSED_VALUE_COUNT, 52);
}
