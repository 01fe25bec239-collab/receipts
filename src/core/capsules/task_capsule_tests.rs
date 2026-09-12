//! Behavioral tests use only the public capsule API, including from this sibling module.
use crate::capsules::*;
use crate::graph::{CapabilityName, GraphError, GraphNode, GraphNodeKind, GraphNodeState};

const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const START: &str = "abcdef0123456789abcdef0123456789abcdef01";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_owned()).collect()
}
fn criterion() -> Criterion {
    Criterion::try_new(
        "criterion".into(),
        "  Verify ☃  ".into(),
        CriterionKind::Deterministic,
        Some(strings(&["cargo", "test"])),
        Some("reason".into()),
    )
    .unwrap()
}
fn reference() -> Ref {
    Ref::try_new(
        RefType::RepoPath,
        "  文/e\u{301}  ".into(),
        Some("".into()),
        Some("section".into()),
    )
    .unwrap()
}
fn plan() -> VerificationPlanEntryV1 {
    VerificationPlanEntryV1::try_new(strings(&["cargo", "test"]), 0).unwrap()
}

// Test-only input harness: all production construction goes through try_new.
struct Input {
    task_id: String,
    workstream_id: String,
    parent_task_id: Option<String>,
    attempt_number: i64,
    task_type: TaskType,
    objective: String,
    acceptance_criteria: Vec<Criterion>,
    non_goals: Option<Vec<String>>,
    baseline_sha: String,
    start_sha: String,
    allowed_write_paths: Vec<String>,
    forbidden_write_paths: Vec<String>,
    relevant_context_refs: Option<Vec<Ref>>,
    architecture_refs: Option<Vec<Ref>>,
    contract_refs: Option<Vec<Ref>>,
    dependencies: Option<Vec<String>>,
    quality_floor: QualityFloor,
    required_capabilities: Vec<CapabilityName>,
    preferred_capabilities: Option<Vec<CapabilityName>>,
    verification_plan: Vec<VerificationPlanEntryV1>,
    review_policy: Option<ReviewPolicy>,
    assurance_profile: AssuranceProfileSelector,
    cost_policy: Option<CostPolicy>,
    time_budget_seconds: Option<i64>,
    turn_budget: Option<i64>,
    branch: String,
    worktree: String,
    remote_publish_policy: Option<RemotePublishPolicy>,
    stop_conditions: Vec<String>,
    handoff_schema: Option<Ref>,
    context_epoch: i64,
}
impl Input {
    fn valid() -> Self {
        Self {
            task_id: "task".into(),
            workstream_id: "stream".into(),
            parent_task_id: None,
            attempt_number: 1,
            task_type: TaskType::Implementation,
            objective: "  Build ☃  ".into(),
            acceptance_criteria: vec![criterion()],
            non_goals: None,
            baseline_sha: BASE.into(),
            start_sha: START.into(),
            allowed_write_paths: strings(&["src/**"]),
            forbidden_write_paths: vec![],
            relevant_context_refs: None,
            architecture_refs: None,
            contract_refs: None,
            dependencies: None,
            quality_floor: QualityFloor::Frontier,
            required_capabilities: vec![],
            preferred_capabilities: None,
            verification_plan: vec![plan()],
            review_policy: None,
            assurance_profile: AssuranceProfileSelector::Standard,
            cost_policy: None,
            time_budget_seconds: None,
            turn_budget: None,
            branch: "runtime-a3/task-001".into(),
            worktree: " /future/☃/../worker ".into(),
            remote_publish_policy: None,
            stop_conditions: strings(&["stop ☃"]),
            handoff_schema: None,
            context_epoch: 0,
        }
    }
    fn build(self) -> Result<TaskCapsule, CapsuleError> {
        TaskCapsule::try_new(
            self.task_id,
            self.workstream_id,
            self.parent_task_id,
            self.attempt_number,
            self.task_type,
            self.objective,
            self.acceptance_criteria,
            self.non_goals,
            self.baseline_sha,
            self.start_sha,
            self.allowed_write_paths,
            self.forbidden_write_paths,
            self.relevant_context_refs,
            self.architecture_refs,
            self.contract_refs,
            self.dependencies,
            self.quality_floor,
            self.required_capabilities,
            self.preferred_capabilities,
            self.verification_plan,
            self.review_policy,
            self.assurance_profile,
            self.cost_policy,
            self.time_budget_seconds,
            self.turn_budget,
            self.branch,
            self.worktree,
            self.remote_publish_policy,
            self.stop_conditions,
            self.handoff_schema,
            self.context_epoch,
        )
    }
}

#[test]
fn all_required_fields_survive_and_all_optional_fields_can_be_absent() {
    let c = Input::valid().build().unwrap();
    assert_eq!(c.task_id(), "task");
    assert_eq!(c.workstream_id(), "stream");
    assert_eq!(c.attempt_number(), 1);
    assert_eq!(c.task_type(), TaskType::Implementation);
    assert_eq!(c.objective(), "  Build ☃  ");
    assert_eq!(c.acceptance_criteria(), &[criterion()]);
    assert_eq!(c.baseline_sha(), BASE);
    assert_eq!(c.start_sha(), START);
    assert_ne!(c.baseline_sha(), c.start_sha());
    assert_eq!(c.allowed_write_paths(), strings(&["src/**"]));
    assert!(c.forbidden_write_paths().is_empty());
    assert_eq!(c.quality_floor(), QualityFloor::Frontier);
    assert!(c.required_capabilities().is_empty());
    assert_eq!(c.verification_plan(), &[plan()]);
    assert_eq!(c.assurance_profile(), AssuranceProfileSelector::Standard);
    assert_eq!(c.branch(), "runtime-a3/task-001");
    assert_eq!(c.worktree(), " /future/☃/../worker ");
    assert_eq!(c.stop_conditions(), strings(&["stop ☃"]));
    assert_eq!(c.context_epoch(), 0);
    assert_eq!(c.parent_task_id(), None);
    assert_eq!(c.non_goals(), None);
    assert_eq!(c.relevant_context_refs(), None);
    assert_eq!(c.architecture_refs(), None);
    assert_eq!(c.contract_refs(), None);
    assert_eq!(c.dependencies(), None);
    assert_eq!(c.preferred_capabilities(), None);
    assert_eq!(c.review_policy(), None);
    assert_eq!(c.cost_policy(), None);
    assert_eq!(c.time_budget_seconds(), None);
    assert_eq!(c.turn_budget(), None);
    assert_eq!(c.remote_publish_policy(), None);
    assert_eq!(c.handoff_schema(), None);
}

#[test]
fn all_optional_fields_preserve_present_values() {
    let mut i = Input::valid();
    i.parent_task_id = Some(" parent ☃ ".into());
    i.non_goals = Some(strings(&["", " no change ☃ ", ""]));
    i.relevant_context_refs = Some(vec![reference()]);
    i.architecture_refs = Some(vec![reference(), reference()]);
    i.contract_refs = Some(vec![reference()]);
    i.dependencies = Some(strings(&[" z ", "☃", " z "]));
    let cap = CapabilityName::new("graph.core").unwrap();
    i.preferred_capabilities = Some(vec![cap.clone()]);
    let review = ReviewPolicy::try_new(
        Some(false),
        Some(DistinctProvider::Required),
        Some(QualityFloor::Economy),
    )
    .unwrap();
    i.review_policy = Some(review.clone());
    let cost = CostPolicy::try_new(Some(Some(-2.5)), Some(CostPriority::Low)).unwrap();
    i.cost_policy = Some(cost.clone());
    i.time_budget_seconds = Some(12);
    i.turn_budget = Some(3);
    i.remote_publish_policy = Some(RemotePublishPolicy::PushOnAccept);
    i.handoff_schema = Some(reference());
    let c = i.build().unwrap();
    assert_eq!(c.parent_task_id(), Some(" parent ☃ "));
    assert_eq!(
        c.non_goals(),
        Some(strings(&["", " no change ☃ ", ""]).as_slice())
    );
    assert_eq!(c.relevant_context_refs(), Some([reference()].as_slice()));
    assert_eq!(
        c.architecture_refs(),
        Some([reference(), reference()].as_slice())
    );
    assert_eq!(c.contract_refs(), Some([reference()].as_slice()));
    assert_eq!(
        c.dependencies(),
        Some(strings(&[" z ", "☃", " z "]).as_slice())
    );
    assert_eq!(c.preferred_capabilities(), Some([cap].as_slice()));
    assert_eq!(c.review_policy(), Some(&review));
    assert_eq!(c.cost_policy(), Some(&cost));
    assert_eq!(c.time_budget_seconds(), Some(12));
    assert_eq!(c.turn_budget(), Some(3));
    assert_eq!(
        c.remote_publish_policy(),
        Some(RemotePublishPolicy::PushOnAccept)
    );
    assert_eq!(c.handoff_schema(), Some(&reference()));
}

#[test]
fn optional_arrays_preserve_present_empty_distinct_from_absence() {
    let mut i = Input::valid();
    i.non_goals = Some(vec![]);
    i.relevant_context_refs = Some(vec![]);
    i.architecture_refs = Some(vec![]);
    i.contract_refs = Some(vec![]);
    i.dependencies = Some(vec![]);
    i.preferred_capabilities = Some(vec![]);
    let c = i.build().unwrap();
    assert_eq!(c.non_goals(), Some([].as_slice()));
    assert_eq!(c.relevant_context_refs(), Some([].as_slice()));
    assert_eq!(c.architecture_refs(), Some([].as_slice()));
    assert_eq!(c.contract_refs(), Some([].as_slice()));
    assert_eq!(c.dependencies(), Some([].as_slice()));
    assert_eq!(c.preferred_capabilities(), Some([].as_slice()));
}

#[test]
fn every_identifier_family_counts_unicode_scalars_and_preserves_exact_text() {
    for field in [
        "task_id",
        "workstream_id",
        "parent_task_id",
        "acceptance_criteria.id",
        "dependencies",
    ] {
        for value in [
            String::new(),
            "☃".into(),
            "🦀".repeat(200),
            "🦀".repeat(201),
            "  e\u{301} É  ".into(),
        ] {
            let mut i = Input::valid();
            let result = if field == "acceptance_criteria.id" {
                Criterion::try_new(
                    value.clone(),
                    "description".into(),
                    CriterionKind::Semantic,
                    None,
                    None,
                )
                .map(|c| c.id().to_owned())
            } else {
                match field {
                    "task_id" => i.task_id = value.clone(),
                    "workstream_id" => i.workstream_id = value.clone(),
                    "parent_task_id" => i.parent_task_id = Some(value.clone()),
                    "dependencies" => i.dependencies = Some(vec![value.clone()]),
                    _ => unreachable!(),
                }
                i.build().map(|c| match field {
                    "task_id" => c.task_id().to_owned(),
                    "workstream_id" => c.workstream_id().to_owned(),
                    "parent_task_id" => c.parent_task_id().unwrap().to_owned(),
                    "dependencies" => c.dependencies().unwrap()[0].clone(),
                    _ => unreachable!(),
                })
            };
            match value.chars().count() {
                0 => assert_eq!(result, Err(CapsuleError::EmptyIdentifier { field })),
                201 => assert_eq!(
                    result,
                    Err(CapsuleError::IdentifierTooLong {
                        field,
                        length: 201,
                        max: 200
                    })
                ),
                _ => assert_eq!(result.unwrap(), value),
            }
        }
    }
}

#[test]
fn sha_validation_is_exact_for_both_fields() {
    for field in ["baseline_sha", "start_sha"] {
        for value in [
            "a".repeat(39),
            "a".repeat(41),
            "A".repeat(40),
            "g".repeat(40),
            "abc1234".into(),
            "runtime-a3/task".into(),
            "main".into(),
            "v1.0".into(),
            format!(" {BASE}"),
            format!("{BASE}\n"),
            "☃".repeat(40),
            String::new(),
        ] {
            let mut i = Input::valid();
            if field == "baseline_sha" {
                i.baseline_sha = value;
            } else {
                i.start_sha = value;
            }
            assert_eq!(i.build(), Err(CapsuleError::InvalidSha { field }));
        }
    }
    Input::valid().build().unwrap();
}

#[test]
fn argv_is_preserved_with_empty_later_arguments_and_signed_expectations() {
    assert_eq!(
        VerificationPlanEntryV1::try_new(vec![], 0),
        Err(CapsuleError::VerificationArgvEmpty)
    );
    assert_eq!(
        VerificationPlanEntryV1::try_new(strings(&[""]), 0),
        Err(CapsuleError::VerificationArgv0Empty)
    );
    for argv in [
        strings(&["cargo", "test"]),
        strings(&["cargo", ""]),
        strings(&["cargo", " ", "--test", "☃"]),
        strings(&[" ", "$(untouched)", "\0"]),
    ] {
        for code in [0, 1, -1, i64::MIN, i64::MAX] {
            let entry = VerificationPlanEntryV1::try_new(argv.clone(), code).unwrap();
            assert_eq!(entry.command(), argv);
            assert_eq!(entry.expected_exit_code(), code);
        }
    }
}

#[test]
fn verification_cardinality_matrix_and_repair_relationship() {
    for task_type in TaskType::ALL {
        let mut i = Input::valid();
        i.task_type = task_type;
        i.parent_task_id = Some("parent".into());
        i.verification_plan = vec![];
        let result = i.build();
        if matches!(task_type, TaskType::Docs | TaskType::Investigation) {
            assert!(result.unwrap().verification_plan().is_empty());
        } else {
            assert_eq!(
                result,
                Err(CapsuleError::VerificationPlanMissing { task_type })
            );
        }
        for parent in [None, Some("parent".to_owned())] {
            let mut i = Input::valid();
            i.task_type = task_type;
            i.parent_task_id = parent.clone();
            let result = i.build();
            if task_type == TaskType::Repair && parent.is_none() {
                assert_eq!(result, Err(CapsuleError::RepairParentMissing));
            } else {
                let c = result.unwrap();
                assert_eq!(c.verification_plan(), &[plan()]);
                assert_eq!(c.parent_task_id(), parent.as_deref());
                assert_eq!(c.task_type(), task_type);
            }
        }
    }
}

#[test]
fn canonical_graph_capabilities_transfer_verbatim_without_domain_weakening() {
    let caps = vec![
        CapabilityName::new("future_engine.some_capability").unwrap(),
        CapabilityName::new("graph.core").unwrap(),
        CapabilityName::new("future_engine.some_capability").unwrap(),
    ];
    let node = GraphNode::new(
        "node",
        GraphNodeKind::TASK,
        GraphNodeState::Planned,
        caps.clone(),
    )
    .unwrap();
    let mut i = Input::valid();
    i.required_capabilities = node.required_capabilities().to_vec();
    i.preferred_capabilities = Some(node.required_capabilities().to_vec());
    let c = i.build().unwrap();
    let required: &[CapabilityName] = c.required_capabilities();
    let preferred: Option<&[CapabilityName]> = c.preferred_capabilities();
    assert_eq!(required, node.required_capabilities());
    assert_eq!(preferred, Some(caps.as_slice()));
    assert_eq!(
        required
            .iter()
            .map(CapabilityName::as_str)
            .collect::<Vec<_>>(),
        [
            "future_engine.some_capability",
            "graph.core",
            "future_engine.some_capability"
        ]
    );
    for invalid in ["graph", "Graph.core", "coding", "structured_output"] {
        // Both constructor parameters require canonical values; syntax fails before capsule construction.
        let result = CapabilityName::new(invalid).map(|cap| {
            let mut i = Input::valid();
            i.required_capabilities = vec![cap.clone()];
            i.preferred_capabilities = Some(vec![cap]);
            i.build()
        });
        assert_eq!(
            result,
            Err(GraphError::InvalidCapabilitySyntax {
                value: invalid.into()
            })
        );
    }
}

#[test]
fn paths_and_open_strings_are_declarative_and_preserved() {
    let mut i = Input::valid();
    i.allowed_write_paths.clear();
    assert_eq!(
        i.build(),
        Err(CapsuleError::EmptyField {
            field: "allowed_write_paths"
        })
    );
    for paths in [
        strings(&[""]),
        strings(&[" "]),
        strings(&["☃/**", "../outside", "~/x", " a/../b ", "☃/**"]),
    ] {
        let mut i = Input::valid();
        i.allowed_write_paths = paths.clone();
        i.forbidden_write_paths = paths.clone();
        i.stop_conditions = paths.clone();
        i.objective = " ".into();
        i.worktree = " ".into();
        let c = i.build().unwrap();
        assert_eq!(c.allowed_write_paths(), paths);
        assert_eq!(c.forbidden_write_paths(), paths);
        assert_eq!(c.stop_conditions(), paths);
        assert_eq!(c.objective(), " ");
        assert_eq!(c.worktree(), " ");
    }
}

#[test]
fn references_preserve_every_type_and_optional_open_string_state() {
    for ref_type in RefType::ALL {
        assert_eq!(
            Ref::try_new(ref_type, "".into(), None, None),
            Err(CapsuleError::EmptyField {
                field: "reference.target"
            })
        );
        for digest in [None, Some(""), Some(" arbitrary ☃ ")] {
            for section in [None, Some(""), Some(" e\u{301} section ")] {
                for target in ["☃/e\u{301}", " "] {
                    let r = Ref::try_new(
                        ref_type,
                        target.into(),
                        digest.map(str::to_owned),
                        section.map(str::to_owned),
                    )
                    .unwrap();
                    assert_eq!(r.ref_type(), ref_type);
                    assert_eq!(r.target(), target);
                    assert_eq!(r.digest(), digest);
                    assert_eq!(r.section(), section);
                }
            }
        }
    }
}

#[test]
fn criterion_retains_historical_check_command_without_argv_rules() {
    assert_eq!(
        Criterion::try_new("id".into(), "".into(), CriterionKind::Semantic, None, None),
        Err(CapsuleError::EmptyField {
            field: "acceptance_criteria.description"
        })
    );
    for kind in CriterionKind::ALL {
        for command in [None, Some(vec![]), Some(strings(&["", " ", "☃"]))] {
            for rationale in [None, Some(""), Some("  reason ☃  ")] {
                let c = Criterion::try_new(
                    " id ".into(),
                    " ".into(),
                    kind,
                    command.clone(),
                    rationale.map(str::to_owned),
                )
                .unwrap();
                assert_eq!(c.id(), " id ");
                assert_eq!(c.description(), " ");
                assert_eq!(c.kind(), kind);
                assert_eq!(c.check_command(), command.as_deref());
                assert_eq!(c.rationale(), rationale);
            }
        }
    }
}

#[test]
fn cost_policy_preserves_absent_null_and_finite_states() {
    assert!(Input::valid().build().unwrap().cost_policy().is_none());
    for max_cost in [
        None,
        Some(None),
        Some(Some(0.0)),
        Some(Some(12.5)),
        Some(Some(-2.5)),
        Some(Some(f64::MAX)),
        Some(Some(-f64::MAX)),
    ] {
        for priority in [
            None,
            Some(CostPriority::Highest),
            Some(CostPriority::High),
            Some(CostPriority::Secondary),
            Some(CostPriority::Low),
        ] {
            let mut i = Input::valid();
            i.cost_policy = Some(CostPolicy::try_new(max_cost, priority).unwrap());
            let c = i.build().unwrap();
            let policy = c.cost_policy().unwrap();
            assert_eq!(policy.max_cost(), max_cost);
            assert_eq!(policy.priority(), priority);
        }
    }
    assert_ne!(
        CostPolicy::try_new(None, None).unwrap(),
        CostPolicy::try_new(Some(None), None).unwrap()
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            CostPolicy::try_new(Some(Some(value)), None),
            Err(CapsuleError::NonFiniteMaxCost)
        );
    }
}

#[test]
fn review_selectors_preserve_independent_optional_values() {
    for required in [None, Some(false), Some(true)] {
        for distinct in [
            None,
            Some(DistinctProvider::Off),
            Some(DistinctProvider::Preferred),
            Some(DistinctProvider::Required),
        ] {
            for floor in [
                None,
                Some(QualityFloor::Frontier),
                Some(QualityFloor::Balanced),
                Some(QualityFloor::Economy),
            ] {
                let r = ReviewPolicy::try_new(required, distinct, floor).unwrap();
                assert_eq!(r.required(), required);
                assert_eq!(r.distinct_provider(), distinct);
                assert_eq!(r.reviewer_floor(), floor);
            }
        }
    }
}

#[test]
fn required_empty_fields_fail_explicitly() {
    for field in [
        "objective",
        "acceptance_criteria",
        "worktree",
        "stop_conditions",
    ] {
        let mut i = Input::valid();
        match field {
            "objective" => i.objective.clear(),
            "acceptance_criteria" => i.acceptance_criteria.clear(),
            "worktree" => i.worktree.clear(),
            "stop_conditions" => i.stop_conditions.clear(),
            _ => unreachable!(),
        }
        assert_eq!(i.build(), Err(CapsuleError::EmptyField { field }));
    }
}

#[test]
fn numeric_minima_and_full_physical_integer_range() {
    for field in [
        "attempt_number",
        "time_budget_seconds",
        "turn_budget",
        "context_epoch",
    ] {
        for value in [i64::MIN, -1, 0, 1, i64::MAX] {
            let mut i = Input::valid();
            match field {
                "attempt_number" => i.attempt_number = value,
                "time_budget_seconds" => i.time_budget_seconds = Some(value),
                "turn_budget" => i.turn_budget = Some(value),
                "context_epoch" => i.context_epoch = value,
                _ => unreachable!(),
            }
            let minimum = if field == "context_epoch" { 0 } else { 1 };
            let result = i.build();
            if value < minimum {
                assert_eq!(
                    result,
                    Err(CapsuleError::IntegerBelowMinimum {
                        field,
                        minimum,
                        value
                    })
                );
            } else {
                let c = result.unwrap();
                let actual = match field {
                    "attempt_number" => c.attempt_number(),
                    "time_budget_seconds" => c.time_budget_seconds().unwrap(),
                    "turn_budget" => c.turn_budget().unwrap(),
                    "context_epoch" => c.context_epoch(),
                    _ => unreachable!(),
                };
                assert_eq!(actual, value);
            }
        }
    }
}

#[test]
fn branch_validation_is_only_the_frozen_prefix() {
    for branch in [
        "",
        "runtime-a3",
        " Runtime-a3/task",
        "Runtime-a3/task",
        "xruntime-a3/task",
        "build/orchestration-a3-016-task-capsule-physical-contract-v2",
    ] {
        let mut i = Input::valid();
        i.branch = branch.into();
        assert_eq!(i.build(), Err(CapsuleError::InvalidBranch));
    }
    for branch in ["runtime-a3/", "runtime-a3/task-001", "runtime-a3/ ☃/../\n"] {
        let mut i = Input::valid();
        i.branch = branch.into();
        assert_eq!(i.build().unwrap().branch(), branch);
    }
}

#[test]
fn exact_closed_vocabularies_and_selector_storage() {
    assert_eq!(
        TaskType::ALL.map(|v| v.as_str()),
        [
            "IMPLEMENTATION",
            "REPAIR",
            "TEST",
            "REFACTOR",
            "MIGRATION",
            "DOCS",
            "INVESTIGATION",
            "SECURITY_FIX"
        ]
    );
    assert_eq!(
        CriterionKind::ALL.map(|v| v.as_str()),
        ["DETERMINISTIC", "SEMANTIC"]
    );
    assert_eq!(
        RefType::ALL.map(|v| v.as_str()),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    assert_eq!(
        QualityFloor::ALL.map(|v| v.as_str()),
        ["FRONTIER", "BALANCED", "ECONOMY"]
    );
    assert_eq!(
        DistinctProvider::ALL.map(|v| v.as_str()),
        ["OFF", "PREFERRED", "REQUIRED"]
    );
    assert_eq!(
        AssuranceProfileSelector::ALL.map(|v| v.as_str()),
        ["LIGHT", "STANDARD", "HIGH_ASSURANCE"]
    );
    assert_eq!(
        CostPriority::ALL.map(|v| v.as_str()),
        ["HIGHEST", "HIGH", "SECONDARY", "LOW"]
    );
    assert_eq!(
        RemotePublishPolicy::ALL.map(|v| v.as_str()),
        ["LOCAL_ONLY", "PUSH_ON_ACCEPT", "PUSH_ALWAYS"]
    );
    for quality in QualityFloor::ALL {
        for assurance in AssuranceProfileSelector::ALL {
            for remote in RemotePublishPolicy::ALL {
                let mut i = Input::valid();
                i.quality_floor = quality;
                i.assurance_profile = assurance;
                i.remote_publish_policy = Some(remote);
                let c = i.build().unwrap();
                assert_eq!(c.quality_floor(), quality);
                assert_eq!(c.assurance_profile(), assurance);
                assert_eq!(c.remote_publish_policy(), Some(remote));
            }
        }
    }
}
