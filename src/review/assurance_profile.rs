//! Review-local, in-process policy from `ASSURANCE_PROFILES.md` at architecture
//! f49d621ee510705939394f7df4996223a73fdcb7. Requirements describe obligations;
//! they do not execute checks, dispatch reviews, route providers, or bind SHAs.

use crate::A4ReviewFindingCategory;

/// Declaration order is the frozen monotonic assurance order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssuranceProfile {
    Light,
    #[default]
    Standard,
    HighAssurance,
}

impl AssuranceProfile {
    pub const ALL: [Self; 3] = [Self::Light, Self::Standard, Self::HighAssurance];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "LIGHT",
            Self::Standard => "STANDARD",
            Self::HighAssurance => "HIGH_ASSURANCE",
        }
    }

    pub fn meets_floor(self, floor: Self) -> bool {
        self >= floor
    }

    /// Raises assurance as needed; never lowers the requested profile.
    pub fn raise_to_at_least(self, floor: Self) -> Self {
        self.max(floor)
    }

    pub const fn requirements(self) -> AssuranceProfileRequirements {
        let (independent_a4, broker_rerun, quality, distinct_provider, security) = match self {
            Self::Light => (
                false,
                false,
                AssuranceReviewerQualityFloor::Balanced,
                DistinctProviderPolicy::Off,
                SecurityPipelinePolicy::NoAutomaticRequirement,
            ),
            Self::Standard => (
                true,
                false,
                AssuranceReviewerQualityFloor::Frontier,
                DistinctProviderPolicy::Preferred,
                SecurityPipelinePolicy::OnSecurityTasks,
            ),
            Self::HighAssurance => (
                true,
                true,
                AssuranceReviewerQualityFloor::Frontier,
                DistinctProviderPolicy::Required,
                SecurityPipelinePolicy::Always,
            ),
        };
        AssuranceProfileRequirements {
            worker_implementation_required: true,
            worker_checks_required: true,
            captured_worker_evidence_required: true,
            independent_a4_required: independent_a4,
            a4_reproduction_required: independent_a4,
            exact_code_state_binding_required: true,
            broker_deterministic_rerun_required: broker_rerun,
            reviewer_quality_floor: quality,
            distinct_provider,
            security_pipeline: security,
            write_scope_verification_required: true,
        }
    }
}

/// Profile requirements only: `false` means not required by this profile,
/// never prohibited. Captured worker evidence is required for every profile
/// and suffices for LIGHT's worker-evidence layer; it does not waive exact
/// code-state binding, write-scope verification, or any blocking findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssuranceProfileRequirements {
    pub worker_implementation_required: bool,
    pub worker_checks_required: bool,
    pub captured_worker_evidence_required: bool,
    pub independent_a4_required: bool,
    pub a4_reproduction_required: bool,
    pub exact_code_state_binding_required: bool,
    pub broker_deterministic_rerun_required: bool,
    pub reviewer_quality_floor: AssuranceReviewerQualityFloor,
    pub distinct_provider: DistinctProviderPolicy,
    pub security_pipeline: SecurityPipelinePolicy,
    pub write_scope_verification_required: bool,
}

/// Review-internal quality policy, not an assurance profile or routing contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceReviewerQualityFloor {
    /// BALANCED
    Balanced,
    /// FRONTIER
    Frontier,
}

/// Review-internal provider-diversity requirement; performs no routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinctProviderPolicy {
    /// OFF
    Off,
    /// PREFERRED
    Preferred,
    /// REQUIRED
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityPipelinePolicy {
    /// NO_AUTOMATIC_REQUIREMENT from this profile. Other rules may require security.
    NoAutomaticRequirement,
    /// ON_SECURITY_TASKS
    OnSecurityTasks,
    /// ALWAYS
    Always,
}

/// Internal selection categories, not the TaskCapsule task-category wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceTaskCategory {
    ProductionCode,
    SecuritySensitiveCode,
    ArchitectureInterface,
    Migration,
    RoutineRefactor,
    Tests,
    DocumentationNonArchitectural,
    StatusMetadata,
}

pub const fn default_for_task_category(category: AssuranceTaskCategory) -> AssuranceProfile {
    match category {
        AssuranceTaskCategory::ProductionCode
        | AssuranceTaskCategory::RoutineRefactor
        | AssuranceTaskCategory::Tests => AssuranceProfile::Standard,
        AssuranceTaskCategory::SecuritySensitiveCode
        | AssuranceTaskCategory::ArchitectureInterface
        | AssuranceTaskCategory::Migration => AssuranceProfile::HighAssurance,
        AssuranceTaskCategory::DocumentationNonArchitectural
        | AssuranceTaskCategory::StatusMetadata => AssuranceProfile::Light,
    }
}

/// Only the eight-category assurance-profile floor, NOT the global A4 blocking
/// policy. In particular, MissingRequiredNegativeTest remains globally blocking
/// under RUNTIME_A4_LIFECYCLE even though it is outside this narrower floor.
pub const fn is_assurance_profile_blocking_floor(category: A4ReviewFindingCategory) -> bool {
    use A4ReviewFindingCategory::*;
    match category {
        ArchitectureViolation
        | ContractViolation
        | SecurityBoundaryViolation
        | WriteScopeViolation
        | UndisclosedChange
        | UnreproducibleEvidence
        | OverstatedLabel
        | TestWeakenedOrDeleted => true,
        MissingRequiredNegativeTest
        | AcceptanceCriterionUnmet
        | Correctness
        | ErrorHandling
        | RegressionRisk
        | Style
        | Naming
        | MinorPerformance
        | DocGap => false,
    }
}
