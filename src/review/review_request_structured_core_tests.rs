use super::*;
use receipts_workspace_execution::{CommitSha, WorkspaceCheckpointRef, WorkspaceCheckpointRefType};

const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const IMPLEMENTATION: &str = "89abcdef0123456789abcdef0123456789abcdef";
const HUGE: &str = "34028236692093846346337460743176821145600000000000000000000000000000000001";

fn criterion() -> ReviewCapsuleCriterion {
    ReviewCapsuleCriterion::new(
        "c".into(),
        "description".into(),
        ReviewCapsuleCriterionKind::Deterministic,
        None,
        None,
    )
    .unwrap()
}

// Raw test inputs only; production constructors own every validation boundary.
struct Input {
    request_id: String,
    task_id: String,
    attempt_id: String,
    implementation_sha: String,
    baseline_sha: String,
    objective: String,
    acceptance_criteria: Vec<ReviewCapsuleCriterion>,
    allowed_write_paths: Vec<String>,
    assurance_profile: AssuranceProfile,
    review_scope: ReviewCapsuleReviewScope,
    context_epoch: ReviewRequestNonNegativeInteger,
    workstream_id: Option<String>,
    branch: Option<String>,
    non_goals: Option<Vec<String>>,
    architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
    contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
    a3_handoff_ref: Option<WorkspaceCheckpointRef>,
    review_policy: Option<ReviewRequestPolicy>,
    prior_review_ids: Option<Vec<String>>,
    attempt_number: Option<ReviewRequestAttemptNumber>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            request_id: "r".into(),
            task_id: "t".into(),
            attempt_id: "a".into(),
            implementation_sha: IMPLEMENTATION.into(),
            baseline_sha: BASE.into(),
            objective: "o".into(),
            acceptance_criteria: vec![criterion()],
            allowed_write_paths: vec!["".into()],
            assurance_profile: AssuranceProfile::Light,
            review_scope: ReviewCapsuleReviewScope::Full,
            context_epoch: ReviewRequestNonNegativeInteger::from_decimal("0").unwrap(),
            workstream_id: None,
            branch: None,
            non_goals: None,
            architecture_refs: None,
            contract_refs: None,
            a3_handoff_ref: None,
            review_policy: None,
            prior_review_ids: None,
            attempt_number: None,
        }
    }
}

impl Input {
    fn build(self) -> Result<ReviewRequestNonTemporalCore, ReviewRequestConstructionError> {
        ReviewRequestNonTemporalCore::new(
            self.request_id,
            self.task_id,
            self.attempt_id,
            self.implementation_sha,
            self.baseline_sha,
            self.objective,
            self.acceptance_criteria,
            self.allowed_write_paths,
            self.assurance_profile,
            self.review_scope,
            self.context_epoch,
            self.workstream_id,
            self.branch,
            self.non_goals,
            self.architecture_refs,
            self.contract_refs,
            self.a3_handoff_ref,
            self.review_policy,
            self.prior_review_ids,
            self.attempt_number,
        )
    }
}

#[test]
fn minimum_required_core_and_every_absent_optional() {
    let v = Input::default().build().unwrap();
    assert_eq!(v.request_id(), "r");
    assert_eq!(v.task_id(), "t");
    assert_eq!(v.attempt_id(), "a");
    assert_eq!(v.implementation_sha().as_str(), IMPLEMENTATION);
    assert_eq!(v.baseline_sha().as_str(), BASE);
    assert_eq!(v.objective(), "o");
    assert_eq!(v.acceptance_criteria(), [criterion()]);
    assert_eq!(v.allowed_write_paths(), [""]);
    assert_eq!(v.assurance_profile(), AssuranceProfile::Light);
    assert_eq!(v.review_scope(), ReviewCapsuleReviewScope::Full);
    assert_eq!(v.context_epoch().decimal_digits(), "0");
    assert_eq!(v.workstream_id(), None);
    assert_eq!(v.branch(), None);
    assert_eq!(v.non_goals(), None);
    assert_eq!(v.architecture_refs(), None);
    assert_eq!(v.contract_refs(), None);
    assert_eq!(v.a3_handoff_ref(), None);
    assert_eq!(v.review_policy(), None);
    assert_eq!(v.prior_review_ids(), None);
    assert_eq!(v.attempt_number(), None);
    assert_eq!(v.clone(), v);
}

#[test]
fn every_supported_optional_present_and_array_order_duplicates_preserved() {
    let strings: Vec<String> = ["", " x ", "", "界"].map(String::from).into();
    let ids: Vec<String> = ["b", "a", "b"].map(String::from).into();
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::Url,
        " opaque ",
        Some("".into()),
        Some("".into()),
    )
    .unwrap();
    let other =
        WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::RepoPath, "other", None, None)
            .unwrap();
    let refs = vec![reference.clone(), other, reference.clone()];
    let policy = ReviewRequestPolicy::new(
        Some(false),
        Some(DistinctProviderPolicy::Off),
        Some(ReviewRequestReviewerFloor::Economy),
        Some(strings.clone()),
    );
    let criteria = vec![
        criterion(),
        ReviewCapsuleCriterion::new(
            "other".into(),
            " ".into(),
            ReviewCapsuleCriterionKind::Semantic,
            Some(strings.clone()),
            Some("".into()),
        )
        .unwrap(),
        criterion(),
    ];
    let v = Input {
        workstream_id: Some(" w ".into()),
        branch: Some("".into()),
        non_goals: Some(strings.clone()),
        architecture_refs: Some(refs.clone()),
        contract_refs: Some(refs.clone()),
        a3_handoff_ref: Some(reference.clone()),
        review_policy: Some(policy.clone()),
        prior_review_ids: Some(ids.clone()),
        attempt_number: Some(ReviewRequestAttemptNumber::from_decimal("1").unwrap()),
        acceptance_criteria: criteria.clone(),
        allowed_write_paths: strings.clone(),
        ..Input::default()
    }
    .build()
    .unwrap();
    assert_eq!(v.workstream_id(), Some(" w "));
    assert_eq!(v.branch(), Some(""));
    assert_eq!(v.non_goals(), Some(strings.as_slice()));
    assert_eq!(v.allowed_write_paths(), strings);
    assert_eq!(v.acceptance_criteria(), criteria);
    assert_eq!(
        v.acceptance_criteria()[1].check_command(),
        Some(strings.as_slice())
    );
    assert_eq!(v.acceptance_criteria()[1].rationale(), Some(""));
    assert_eq!(v.architecture_refs(), Some(refs.as_slice()));
    assert_eq!(v.contract_refs(), Some(refs.as_slice()));
    assert_eq!(v.a3_handoff_ref(), Some(&reference));
    assert_eq!(v.a3_handoff_ref().unwrap().digest(), Some(""));
    assert_eq!(v.a3_handoff_ref().unwrap().section(), Some(""));
    assert_eq!(v.review_policy(), Some(&policy));
    assert_eq!(
        v.review_policy().unwrap().excluded_session_refs(),
        Some(strings.as_slice())
    );
    assert_eq!(v.prior_review_ids(), Some(ids.as_slice()));
    assert_eq!(v.attempt_number().unwrap().decimal_digits(), "1");
}

#[test]
fn all_optional_arrays_may_be_present_empty() {
    let v = Input {
        non_goals: Some(vec![]),
        architecture_refs: Some(vec![]),
        contract_refs: Some(vec![]),
        prior_review_ids: Some(vec![]),
        review_policy: Some(ReviewRequestPolicy::new(None, None, None, Some(vec![]))),
        ..Input::default()
    }
    .build()
    .unwrap();
    assert_eq!(v.non_goals(), Some([].as_slice()));
    assert_eq!(v.architecture_refs(), Some([].as_slice()));
    assert_eq!(v.contract_refs(), Some([].as_slice()));
    assert_eq!(v.prior_review_ids(), Some([].as_slice()));
    assert_eq!(
        v.review_policy().unwrap().excluded_session_refs(),
        Some([].as_slice())
    );
}

macro_rules! id_test {
    ($name:ident, $field:ident) => {
        #[test]
        fn $name() {
            for accepted in ["界".into(), "界".repeat(200), " \tE\u{301}\n".into()] {
                let v = Input {
                    $field: accepted.clone(),
                    ..Input::default()
                }
                .build()
                .unwrap();
                assert_eq!(v.$field(), accepted);
            }
            for invalid in [String::new(), "界".repeat(201)] {
                assert_eq!(
                    Input {
                        $field: invalid,
                        ..Input::default()
                    }
                    .build(),
                    Err(ReviewRequestConstructionError::InvalidIdentifier(
                        stringify!($field)
                    ))
                );
            }
        }
    };
}
id_test!(request_id_boundaries, request_id);
id_test!(task_id_boundaries, task_id);
id_test!(attempt_id_boundaries, attempt_id);

#[test]
fn workstream_id_boundaries() {
    for accepted in ["界".into(), "界".repeat(200), " \tE\u{301}\n".into()] {
        let v = Input {
            workstream_id: Some(accepted.clone()),
            ..Input::default()
        }
        .build()
        .unwrap();
        assert_eq!(v.workstream_id(), Some(accepted.as_str()));
    }
    for invalid in [String::new(), "界".repeat(201)] {
        assert_eq!(
            Input {
                workstream_id: Some(invalid),
                ..Input::default()
            }
            .build(),
            Err(ReviewRequestConstructionError::InvalidIdentifier(
                "workstream_id"
            ))
        );
    }
}

#[test]
fn prior_review_ids_validate_every_item_without_lookup_or_uniqueness() {
    for accepted in ["界".into(), "界".repeat(200), " \tE\u{301}\n".into()] {
        let ids = vec![accepted.clone(), "middle".into(), accepted];
        let v = Input {
            prior_review_ids: Some(ids.clone()),
            review_scope: ReviewCapsuleReviewScope::RepairVerification,
            ..Input::default()
        }
        .build()
        .unwrap();
        assert_eq!(v.prior_review_ids(), Some(ids.as_slice()));
    }
    for invalid in [String::new(), "界".repeat(201)] {
        for position in 0..3 {
            let mut ids = vec!["a".into(), "b".into(), "a".into()];
            ids[position] = invalid.clone();
            assert_eq!(
                Input {
                    prior_review_ids: Some(ids),
                    ..Input::default()
                }
                .build(),
                Err(ReviewRequestConstructionError::InvalidIdentifier(
                    "prior_review_ids item"
                ))
            );
        }
    }
}

#[test]
fn both_sha_fields_reject_the_complete_malformed_matrix() {
    let invalid = [
        String::new(),
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        "HEAD".into(),
        "main".into(),
        "abcdef0".into(),
        format!(" {BASE}"),
        format!("{BASE}\n"),
        "界".repeat(40),
    ];
    for sha in invalid {
        assert_eq!(
            Input {
                baseline_sha: sha.clone(),
                ..Input::default()
            }
            .build(),
            Err(ReviewRequestConstructionError::MalformedBaselineSha)
        );
        assert_eq!(
            Input {
                implementation_sha: sha,
                ..Input::default()
            }
            .build(),
            Err(ReviewRequestConstructionError::MalformedImplementationSha)
        );
    }
}

#[test]
fn objective_is_nonempty_without_whitespace_or_length_restrictions() {
    assert_eq!(
        Input {
            objective: String::new(),
            ..Input::default()
        }
        .build(),
        Err(ReviewRequestConstructionError::EmptyObjective)
    );
    for objective in [" \t\n".into(), "é e\u{301}".into(), "x".repeat(100_000)] {
        assert_eq!(
            Input {
                objective: objective.clone(),
                ..Input::default()
            }
            .build()
            .unwrap()
            .objective(),
            objective
        );
    }
}

#[test]
fn required_arrays_reject_empty_collections_only() {
    assert_eq!(
        Input {
            acceptance_criteria: vec![],
            ..Input::default()
        }
        .build(),
        Err(ReviewRequestConstructionError::EmptyAcceptanceCriteria)
    );
    assert_eq!(
        Input {
            allowed_write_paths: vec![],
            ..Input::default()
        }
        .build(),
        Err(ReviewRequestConstructionError::EmptyAllowedWritePaths)
    );
    assert_eq!(
        Input::default().build().unwrap().allowed_write_paths(),
        [""]
    );
}

#[test]
fn reused_criterion_shape_and_both_kinds_are_exact() {
    assert_eq!(
        ReviewCapsuleCriterionKind::ALL.map(|v| v.as_str()),
        ["DETERMINISTIC", "SEMANTIC"]
    );
    for kind in ReviewCapsuleCriterionKind::ALL {
        match kind {
            ReviewCapsuleCriterionKind::Deterministic | ReviewCapsuleCriterionKind::Semantic => {}
        }
        for id in ["界".into(), "界".repeat(200), " \n".into()] {
            for check_command in [
                None,
                Some(vec![]),
                Some(vec!["".into(), "x".into(), "".into()]),
            ] {
                for rationale in [None, Some(String::new()), Some(" r ".into())] {
                    let c = ReviewCapsuleCriterion::new(
                        id.clone(),
                        " ".into(),
                        kind,
                        check_command.clone(),
                        rationale.clone(),
                    )
                    .unwrap();
                    let v = Input {
                        acceptance_criteria: vec![c.clone()],
                        ..Input::default()
                    }
                    .build()
                    .unwrap();
                    assert_eq!(v.acceptance_criteria(), std::slice::from_ref(&c));
                    assert_eq!(c.id(), id);
                    assert_eq!(c.description(), " ");
                    assert_eq!(c.kind(), kind);
                    assert_eq!(c.check_command(), check_command.as_deref());
                    assert_eq!(c.rationale(), rationale.as_deref());
                }
            }
        }
    }
    for (id, description, error) in [
        (
            String::new(),
            "x",
            ReviewCapsuleConstructionError::EmptyCriterionId,
        ),
        (
            "界".repeat(201),
            "x",
            ReviewCapsuleConstructionError::CriterionIdTooLong,
        ),
        (
            "c".into(),
            "",
            ReviewCapsuleConstructionError::EmptyCriterionDescription,
        ),
    ] {
        assert_eq!(
            ReviewCapsuleCriterion::new(
                id,
                description.into(),
                ReviewCapsuleCriterionKind::Semantic,
                None,
                None
            ),
            Err(error)
        );
    }
}

#[test]
fn workspace_references_preserve_all_four_types_and_optional_strings() {
    assert_eq!(
        WorkspaceCheckpointRefType::ALL.map(|v| v.as_str()),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    for kind in WorkspaceCheckpointRefType::ALL {
        assert!(WorkspaceCheckpointRef::new(kind, "", None, None).is_err());
        for digest in [None, Some("".into()), Some(" d ".into())] {
            for section in [None, Some("".into()), Some(" s ".into())] {
                let reference =
                    WorkspaceCheckpointRef::new(kind, " ", digest.clone(), section.clone())
                        .unwrap();
                let v = Input {
                    architecture_refs: Some(vec![reference.clone()]),
                    contract_refs: Some(vec![reference.clone()]),
                    a3_handoff_ref: Some(reference.clone()),
                    ..Input::default()
                }
                .build()
                .unwrap();
                for stored in [
                    &v.architecture_refs().unwrap()[0],
                    &v.contract_refs().unwrap()[0],
                    v.a3_handoff_ref().unwrap(),
                ] {
                    assert_eq!(stored, &reference);
                    assert_eq!(stored.ref_type(), kind);
                    assert_eq!(stored.target(), " ");
                    assert_eq!(stored.digest(), digest.as_deref());
                    assert_eq!(stored.section(), section.as_deref());
                }
            }
        }
    }
}

#[test]
fn every_scope_and_assurance_profile_are_stored_without_policy_inference() {
    assert_eq!(
        ReviewCapsuleReviewScope::ALL.map(|v| v.as_str()),
        ["FULL", "SECURITY", "REGRESSION", "REPAIR_VERIFICATION"]
    );
    assert_eq!(
        AssuranceProfile::ALL.map(|v| v.as_str()),
        ["LIGHT", "STANDARD", "HIGH_ASSURANCE"]
    );
    for scope in ReviewCapsuleReviewScope::ALL {
        match scope {
            ReviewCapsuleReviewScope::Full
            | ReviewCapsuleReviewScope::Security
            | ReviewCapsuleReviewScope::Regression
            | ReviewCapsuleReviewScope::RepairVerification => {}
        }
        for profile in AssuranceProfile::ALL {
            let v = Input {
                review_scope: scope,
                assurance_profile: profile,
                ..Input::default()
            }
            .build()
            .unwrap();
            assert_eq!(v.review_scope(), scope);
            assert_eq!(v.assurance_profile(), profile);
            assert_eq!(v.review_policy(), None);
            assert_eq!(v.prior_review_ids(), None);
        }
    }
}

#[test]
fn policy_empty_object_and_required_tristate_are_preserved() {
    for required in [None, Some(false), Some(true)] {
        let v = Input {
            review_policy: Some(ReviewRequestPolicy::new(required, None, None, None)),
            ..Input::default()
        }
        .build()
        .unwrap();
        let p = v.review_policy().unwrap();
        assert_eq!(p.required(), required);
        assert_eq!(p.distinct_provider(), None);
        assert_eq!(p.reviewer_floor(), None);
        assert_eq!(p.excluded_session_refs(), None);
    }
}

#[test]
fn all_provider_and_reviewer_floor_values_are_independent_passive_data() {
    assert_eq!(
        ReviewRequestReviewerFloor::ALL.map(|v| v.as_str()),
        ["FRONTIER", "BALANCED", "ECONOMY"]
    );
    let providers = [
        DistinctProviderPolicy::Off,
        DistinctProviderPolicy::Preferred,
        DistinctProviderPolicy::Required,
    ];
    for (provider, spelling) in providers.into_iter().zip(["OFF", "PREFERRED", "REQUIRED"]) {
        assert_eq!(
            match provider {
                DistinctProviderPolicy::Off => "OFF",
                DistinctProviderPolicy::Preferred => "PREFERRED",
                DistinctProviderPolicy::Required => "REQUIRED",
            },
            spelling
        );
        for floor in ReviewRequestReviewerFloor::ALL {
            let expected = match floor {
                ReviewRequestReviewerFloor::Frontier => "FRONTIER",
                ReviewRequestReviewerFloor::Balanced => "BALANCED",
                ReviewRequestReviewerFloor::Economy => "ECONOMY",
            };
            assert_eq!(floor.as_str(), expected);
            let v = Input {
                assurance_profile: AssuranceProfile::HighAssurance,
                review_policy: Some(ReviewRequestPolicy::new(
                    Some(false),
                    Some(provider),
                    Some(floor),
                    None,
                )),
                ..Input::default()
            }
            .build()
            .unwrap();
            let p = v.review_policy().unwrap();
            assert_eq!(p.required(), Some(false));
            assert_eq!(p.distinct_provider(), Some(provider));
            assert_eq!(p.reviewer_floor(), Some(floor));
        }
    }
}

#[test]
fn branch_is_opaque_and_may_be_empty() {
    for branch in ["", " not/a/git/ref .. @{ ", "main"] {
        let v = Input {
            branch: Some(branch.into()),
            ..Input::default()
        }
        .build()
        .unwrap();
        assert_eq!(v.branch(), Some(branch));
        assert_eq!(v.implementation_sha().as_str(), IMPLEMENTATION);
    }
}

#[test]
fn context_epoch_zero_one_and_arbitrary_magnitude() {
    for digits in ["0".into(), "1".into(), HUGE.into(), "9".repeat(10_000)] {
        let value = ReviewRequestNonNegativeInteger::from_decimal(&digits).unwrap();
        let v = Input {
            context_epoch: value.clone(),
            ..Input::default()
        }
        .build()
        .unwrap();
        assert_eq!(v.context_epoch(), &value);
        assert_eq!(v.context_epoch().decimal_digits(), digits);
    }
    assert!(HUGE.parse::<u128>().is_err());
}

#[test]
fn attempt_number_one_two_and_arbitrary_magnitude() {
    for digits in ["1".into(), "2".into(), HUGE.into(), "9".repeat(10_000)] {
        let value = ReviewRequestAttemptNumber::from_decimal(&digits).unwrap();
        let v = Input {
            attempt_number: Some(value.clone()),
            ..Input::default()
        }
        .build()
        .unwrap();
        assert_eq!(v.attempt_number(), Some(&value));
        assert_eq!(v.attempt_number().unwrap().decimal_digits(), digits);
    }
    assert!(HUGE.parse::<u128>().is_err());
}

#[test]
fn integer_value_equality_ignores_sign_of_zero_and_leading_zeroes() {
    let zero = ReviewRequestNonNegativeInteger::from_decimal("0").unwrap();
    for spelling in ["0", "000", "+0", "-0", "-000"] {
        assert_eq!(
            ReviewRequestNonNegativeInteger::from_decimal(spelling).unwrap(),
            zero
        );
        assert_eq!(
            ReviewRequestAttemptNumber::from_decimal(spelling),
            Err(ReviewRequestConstructionError::ZeroAttemptNumber)
        );
    }
    for digits in ["1", "2", HUGE] {
        for spelling in [
            digits.to_string(),
            format!("000{digits}"),
            format!("+000{digits}"),
        ] {
            assert_eq!(
                ReviewRequestNonNegativeInteger::from_decimal(&spelling).unwrap(),
                ReviewRequestNonNegativeInteger::from_decimal(digits).unwrap()
            );
            assert_eq!(
                ReviewRequestAttemptNumber::from_decimal(&spelling).unwrap(),
                ReviewRequestAttemptNumber::from_decimal(digits).unwrap()
            );
        }
    }
}

#[test]
fn integer_carriers_reject_negative_values_and_non_integral_input() {
    for negative in [
        "-1",
        "-0001",
        "-999999999999999999999999999999999999999999999999999999999999999",
    ] {
        assert_eq!(
            ReviewRequestNonNegativeInteger::from_decimal(negative),
            Err(ReviewRequestConstructionError::NegativeInteger)
        );
        assert_eq!(
            ReviewRequestAttemptNumber::from_decimal(negative),
            Err(ReviewRequestConstructionError::NegativeInteger)
        );
    }
    // This API consumes decimal-domain input, not JSON numbers (even integral 1.0).
    for invalid in [
        "", "+", "-", "1.5", "1.0", "1e3", "1e-1", " 1", "1\n", "NaN", "∞", "１", "--1", "+-1",
        "0x10",
    ] {
        assert_eq!(
            ReviewRequestNonNegativeInteger::from_decimal(invalid),
            Err(ReviewRequestConstructionError::InvalidDecimalInteger)
        );
        assert_eq!(
            ReviewRequestAttemptNumber::from_decimal(invalid),
            Err(ReviewRequestConstructionError::InvalidDecimalInteger)
        );
    }
}

#[test]
fn canonical_type_reuse_is_part_of_the_public_signature() {
    let _: fn(&ReviewRequestNonTemporalCore) -> &CommitSha =
        ReviewRequestNonTemporalCore::baseline_sha;
    let _: fn(&ReviewRequestNonTemporalCore) -> &CommitSha =
        ReviewRequestNonTemporalCore::implementation_sha;
    let _: fn(&ReviewRequestNonTemporalCore) -> &[ReviewCapsuleCriterion] =
        ReviewRequestNonTemporalCore::acceptance_criteria;
    let _: fn(&ReviewRequestNonTemporalCore) -> AssuranceProfile =
        ReviewRequestNonTemporalCore::assurance_profile;
    let _: fn(&ReviewRequestNonTemporalCore) -> ReviewCapsuleReviewScope =
        ReviewRequestNonTemporalCore::review_scope;
    let _: fn(&ReviewRequestPolicy) -> Option<DistinctProviderPolicy> =
        ReviewRequestPolicy::distinct_provider;
    let _: fn(&ReviewRequestNonTemporalCore) -> Option<&[WorkspaceCheckpointRef]> =
        ReviewRequestNonTemporalCore::architecture_refs;
    let _: fn(&ReviewRequestNonTemporalCore) -> Option<&[WorkspaceCheckpointRef]> =
        ReviewRequestNonTemporalCore::contract_refs;
    let _: fn(&ReviewRequestNonTemporalCore) -> Option<&WorkspaceCheckpointRef> =
        ReviewRequestNonTemporalCore::a3_handoff_ref;
}

#[test]
fn boundary_has_exact_fields_no_mutable_api_or_deferred_runtime_wire_support() {
    let source = include_str!("review_request_structured_core.rs");
    let body = source
        .split("pub struct ReviewRequestNonTemporalCore {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let fields: Vec<_> = body
        .lines()
        .filter_map(|line| line.trim().split_once(':').map(|(name, _)| name))
        .collect();
    assert_eq!(
        fields,
        [
            "request_id",
            "task_id",
            "attempt_id",
            "implementation_sha",
            "baseline_sha",
            "objective",
            "acceptance_criteria",
            "allowed_write_paths",
            "assurance_profile",
            "review_scope",
            "context_epoch",
            "workstream_id",
            "branch",
            "non_goals",
            "architecture_refs",
            "contract_refs",
            "a3_handoff_ref",
            "review_policy",
            "prior_review_ids",
            "attempt_number"
        ]
    );
    let policy = source
        .split("pub struct ReviewRequestPolicy {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let fields: Vec<_> = policy
        .lines()
        .filter_map(|line| line.trim().split_once(':').map(|(name, _)| name))
        .collect();
    assert_eq!(
        fields,
        [
            "required",
            "distinct_provider",
            "reviewer_floor",
            "excluded_session_refs"
        ]
    );
    let code = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "requested_at",
        "serde",
        "orchestration",
        "receipts_state",
        "pub(crate)",
        "pub(super)",
        "impl Default",
        "enum AssuranceProfile",
        "struct AssuranceProfile",
        "std::process",
        "std::fs",
        "SystemTime",
    ] {
        assert!(
            !code.contains(forbidden),
            "unexpected implementation: {forbidden}"
        );
    }
    // Inspect public inherent signatures, not standard trait implementations:
    // Display requires &mut Formatter but never exposes validated contract state.
    // External compile_fail doctests additionally check private fields and the
    // actual immutable accessor types, including nested values.
    for declaration in code
        .split("pub ")
        .filter(|part| part.starts_with("fn ") || part.starts_with("const fn "))
    {
        let signature = declaration.split('{').next().unwrap();
        assert!(
            !signature.contains("&mut"),
            "public mutable contract API: {signature}"
        );
    }
    let cargo = include_str!("../../crates/receipts-review-integration/Cargo.toml");
    let deps = cargo.split("[dependencies]").nth(1).unwrap().trim();
    assert_eq!(
        deps,
        "receipts-workspace-execution = { path = \"../receipts-workspace-execution\" }"
    );
}
