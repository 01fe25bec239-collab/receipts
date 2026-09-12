use super::*;
use AssuranceProfile::{HighAssurance, Light, Standard};

#[test]
fn assurance_profile_vocabulary_and_default_are_exact() {
    assert_eq!(AssuranceProfile::ALL, [Light, Standard, HighAssurance]);
    assert_eq!(AssuranceProfile::ALL.len(), 3);
    for profile in AssuranceProfile::ALL {
        // Exhaustiveness makes an added variant require an explicit policy decision.
        let expected = match profile {
            Light => "LIGHT",
            Standard => "STANDARD",
            HighAssurance => "HIGH_ASSURANCE",
        };
        assert_eq!(profile.as_str(), expected);
    }
    assert_eq!(AssuranceProfile::default(), Standard);
}

#[test]
fn assurance_profile_ordering_covers_every_pair() {
    let ordered = [Light, Standard, HighAssurance];
    for (i, requested) in ordered.into_iter().enumerate() {
        for (j, floor) in ordered.into_iter().enumerate() {
            assert_eq!(requested.cmp(&floor), i.cmp(&j));
            assert_eq!(requested.partial_cmp(&floor), Some(i.cmp(&j)));
            assert_eq!(requested < floor, i < j);
            assert_eq!(requested > floor, i > j);
            assert_eq!(requested <= floor, i <= j);
            assert_eq!(requested >= floor, i >= j);
            assert_eq!(requested == floor, i == j);
        }
    }
}

#[test]
fn assurance_profile_floor_helpers_cover_all_nine_combinations() {
    let cases = [
        (Light, Light, true, Light),
        (Light, Standard, false, Standard),
        (Light, HighAssurance, false, HighAssurance),
        (Standard, Light, true, Standard),
        (Standard, Standard, true, Standard),
        (Standard, HighAssurance, false, HighAssurance),
        (HighAssurance, Light, true, HighAssurance),
        (HighAssurance, Standard, true, HighAssurance),
        (HighAssurance, HighAssurance, true, HighAssurance),
    ];
    for (requested, floor, meets, raised) in cases {
        assert_eq!(requested.meets_floor(floor), meets);
        assert_eq!(requested.raise_to_at_least(floor), raised);
        assert!(raised >= requested);
        assert!(raised >= floor);
        if meets {
            assert_eq!(raised, requested);
        }
    }
}

#[test]
fn assurance_profile_light_full_matrix() {
    assert_eq!(
        Light.requirements(),
        AssuranceProfileRequirements {
            worker_implementation_required: true,
            worker_checks_required: true,
            captured_worker_evidence_required: true,
            independent_a4_required: false,
            a4_reproduction_required: false,
            exact_code_state_binding_required: true,
            broker_deterministic_rerun_required: false,
            reviewer_quality_floor: AssuranceReviewerQualityFloor::Balanced,
            distinct_provider: DistinctProviderPolicy::Off,
            security_pipeline: SecurityPipelinePolicy::NoAutomaticRequirement,
            write_scope_verification_required: true,
        }
    );
    // Captured worker evidence suffices for LIGHT's worker-evidence layer.
    // Optional A4 and no automatic security requirement do not prohibit either.
}

#[test]
fn assurance_profile_standard_full_matrix() {
    assert_eq!(
        Standard.requirements(),
        AssuranceProfileRequirements {
            worker_implementation_required: true,
            worker_checks_required: true,
            captured_worker_evidence_required: true,
            independent_a4_required: true,
            a4_reproduction_required: true,
            exact_code_state_binding_required: true,
            broker_deterministic_rerun_required: false,
            reviewer_quality_floor: AssuranceReviewerQualityFloor::Frontier,
            distinct_provider: DistinctProviderPolicy::Preferred,
            security_pipeline: SecurityPipelinePolicy::OnSecurityTasks,
            write_scope_verification_required: true,
        }
    );
}

#[test]
fn assurance_profile_high_assurance_full_matrix() {
    assert_eq!(
        HighAssurance.requirements(),
        AssuranceProfileRequirements {
            worker_implementation_required: true,
            worker_checks_required: true,
            captured_worker_evidence_required: true,
            independent_a4_required: true,
            a4_reproduction_required: true,
            exact_code_state_binding_required: true,
            broker_deterministic_rerun_required: true,
            reviewer_quality_floor: AssuranceReviewerQualityFloor::Frontier,
            distinct_provider: DistinctProviderPolicy::Required,
            security_pipeline: SecurityPipelinePolicy::Always,
            write_scope_verification_required: true,
        }
    );
}

#[test]
fn assurance_profile_cross_profile_invariants() {
    for profile in AssuranceProfile::ALL {
        let requirements = profile.requirements();
        assert!(requirements.worker_implementation_required);
        assert!(requirements.worker_checks_required);
        assert!(requirements.captured_worker_evidence_required);
        assert!(requirements.exact_code_state_binding_required);
        assert!(requirements.write_scope_verification_required);
        assert_eq!(requirements.independent_a4_required, profile != Light);
        assert_eq!(requirements.a4_reproduction_required, profile != Light);
        assert_eq!(
            requirements.broker_deterministic_rerun_required,
            profile == HighAssurance
        );
    }
}

#[test]
fn assurance_profile_internal_policy_vocabularies_are_closed() {
    for profile in AssuranceProfile::ALL {
        let requirements = profile.requirements();
        let quality = match requirements.reviewer_quality_floor {
            AssuranceReviewerQualityFloor::Balanced => "BALANCED",
            AssuranceReviewerQualityFloor::Frontier => "FRONTIER",
        };
        let diversity = match requirements.distinct_provider {
            DistinctProviderPolicy::Off => "OFF",
            DistinctProviderPolicy::Preferred => "PREFERRED",
            DistinctProviderPolicy::Required => "REQUIRED",
        };
        let security = match requirements.security_pipeline {
            SecurityPipelinePolicy::NoAutomaticRequirement => "NO_AUTOMATIC_REQUIREMENT",
            SecurityPipelinePolicy::OnSecurityTasks => "ON_SECURITY_TASKS",
            SecurityPipelinePolicy::Always => "ALWAYS",
        };
        let expected = match profile {
            Light => ("BALANCED", "OFF", "NO_AUTOMATIC_REQUIREMENT"),
            Standard => ("FRONTIER", "PREFERRED", "ON_SECURITY_TASKS"),
            HighAssurance => ("FRONTIER", "REQUIRED", "ALWAYS"),
        };
        assert_eq!((quality, diversity, security), expected);
    }
}

const TASK_DEFAULTS: [(AssuranceTaskCategory, AssuranceProfile); 8] = [
    (AssuranceTaskCategory::ProductionCode, Standard),
    (AssuranceTaskCategory::SecuritySensitiveCode, HighAssurance),
    (AssuranceTaskCategory::ArchitectureInterface, HighAssurance),
    (AssuranceTaskCategory::Migration, HighAssurance),
    (AssuranceTaskCategory::RoutineRefactor, Standard),
    (AssuranceTaskCategory::Tests, Standard),
    (AssuranceTaskCategory::DocumentationNonArchitectural, Light),
    (AssuranceTaskCategory::StatusMetadata, Light),
];

#[test]
fn assurance_profile_all_eight_task_defaults() {
    assert_eq!(TASK_DEFAULTS.len(), 8);
    for (index, (category, expected)) in TASK_DEFAULTS.into_iter().enumerate() {
        let category_index = match category {
            AssuranceTaskCategory::ProductionCode => 0,
            AssuranceTaskCategory::SecuritySensitiveCode => 1,
            AssuranceTaskCategory::ArchitectureInterface => 2,
            AssuranceTaskCategory::Migration => 3,
            AssuranceTaskCategory::RoutineRefactor => 4,
            AssuranceTaskCategory::Tests => 5,
            AssuranceTaskCategory::DocumentationNonArchitectural => 6,
            AssuranceTaskCategory::StatusMetadata => 7,
        };
        assert_eq!(category_index, index);
        assert_eq!(default_for_task_category(category), expected);
    }
}

#[test]
fn assurance_profile_blocking_floor_is_exactly_eight_categories() {
    use A4ReviewFindingCategory::*;
    let floor = [
        ArchitectureViolation,
        ContractViolation,
        SecurityBoundaryViolation,
        WriteScopeViolation,
        UndisclosedChange,
        UnreproducibleEvidence,
        OverstatedLabel,
        TestWeakenedOrDeleted,
    ];
    for category in A4ReviewFindingCategory::ALL {
        assert_eq!(
            is_assurance_profile_blocking_floor(category),
            floor.contains(&category),
            "{category:?}"
        );
    }
    assert_eq!(
        A4ReviewFindingCategory::ALL
            .into_iter()
            .filter(|category| is_assurance_profile_blocking_floor(*category))
            .count(),
        8
    );
}

#[test]
fn assurance_profile_floor_does_not_define_global_a4_blocking_policy() {
    // RUNTIME_A4_LIFECYCLE still makes this globally blocking. A false result
    // only excludes it from ASSURANCE_PROFILES' narrower eight-category floor.
    assert!(!is_assurance_profile_blocking_floor(
        A4ReviewFindingCategory::MissingRequiredNegativeTest
    ));
}

#[test]
fn assurance_profile_policy_is_deterministic() {
    for _ in 0..3 {
        for requested in AssuranceProfile::ALL {
            assert_eq!(requested.requirements(), requested.requirements());
            assert_eq!(requested.as_str(), requested.as_str());
            for floor in AssuranceProfile::ALL {
                assert_eq!(requested.meets_floor(floor), requested.meets_floor(floor));
                assert_eq!(
                    requested.raise_to_at_least(floor),
                    requested.raise_to_at_least(floor)
                );
            }
        }
        for (category, expected) in TASK_DEFAULTS {
            assert_eq!(default_for_task_category(category), expected);
        }
        for category in A4ReviewFindingCategory::ALL {
            assert_eq!(
                is_assurance_profile_blocking_floor(category),
                is_assurance_profile_blocking_floor(category)
            );
        }
    }
}
