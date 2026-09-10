//! Tests through the public API; no private invariant access.
use super::*;
use crate::orchestration::OrchestrationDateTimeV1;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OPEN: &str = "  Aa é e\u{301} 文 🦀 /../ ** \n";
const HUGE: &str = "18446744073709551616";
fn strings() -> Vec<String> {
    vec![OPEN.into(), "".into(), "aA".into(), OPEN.into()]
}
fn reference() -> Ref {
    Ref::try_new(
        RefType::RepoPath,
        OPEN.into(),
        Some(OPEN.into()),
        Some(OPEN.into()),
    )
    .unwrap()
}
fn criteria() -> Vec<Criterion> {
    vec![
        Criterion::try_new(
            "second".into(),
            OPEN.into(),
            CriterionKind::Semantic,
            Some(strings()),
            Some(OPEN.into()),
        )
        .unwrap(),
        Criterion::try_new(
            "first".into(),
            " Original target ".into(),
            CriterionKind::Deterministic,
            Some(vec!["check".into(), "".into()]),
            None,
        )
        .unwrap(),
        Criterion::try_new(
            "second".into(),
            OPEN.into(),
            CriterionKind::Semantic,
            Some(strings()),
            Some(OPEN.into()),
        )
        .unwrap(),
    ]
}
#[derive(Clone)]
struct FindingInput {
    finding_id: String,
    severity: RepairFindingSeverityV1,
    category: RepairFindingCategoryV1,
    description: String,
    blocking: bool,
    path: Option<String>,
    line: Option<RepairFindingLineV1>,
    evidence_ref: Option<Ref>,
    confidence: Option<RepairFindingConfidenceV1>,
    source: Option<RepairFindingSourceV1>,
}
impl FindingInput {
    fn valid() -> Self {
        Self {
            finding_id: OPEN.into(),
            severity: RepairFindingSeverityV1::Critical,
            category: RepairFindingCategoryV1::Correctness,
            description: OPEN.into(),
            blocking: false,
            path: Some(OPEN.into()),
            line: Some(RepairFindingLineV1::try_new(HUGE).unwrap()),
            evidence_ref: Some(reference()),
            confidence: Some(RepairFindingConfidenceV1::High),
            source: Some(RepairFindingSourceV1::Test),
        }
    }
    fn build(self) -> Result<RepairCapsuleFindingV1, CapsuleError> {
        RepairCapsuleFindingV1::try_new(
            self.finding_id,
            self.severity,
            self.category,
            self.description,
            self.blocking,
            self.path,
            self.line,
            self.evidence_ref,
            self.confidence,
            self.source,
        )
    }
}
fn finding() -> RepairCapsuleFindingV1 {
    FindingInput::valid().build().unwrap()
}
#[test]
fn finding_every_supplied_field_is_observable_exactly() {
    let input = FindingInput::valid();
    let value = input.clone().build().unwrap();
    assert_eq!(value.finding_id(), input.finding_id);
    assert_eq!(value.severity(), input.severity);
    assert_eq!(value.category(), input.category);
    assert_eq!(value.description(), input.description);
    assert_eq!(value.blocking(), input.blocking);
    assert_eq!(value.path(), input.path.as_deref());
    assert_eq!(value.line(), input.line.as_ref());
    assert_eq!(value.evidence_ref(), input.evidence_ref.as_ref());
    assert_eq!(value.confidence(), input.confidence);
    assert_eq!(value.source(), input.source);
}
#[derive(Clone)]
struct CheckInput {
    source: RepairCheckSourceV1,
    command: Vec<String>,
    exit_code: RepairExitCodeV1,
    code_sha: String,
    timed_out: Option<bool>,
    started_at: Option<OrchestrationDateTimeV1>,
    finished_at: Option<OrchestrationDateTimeV1>,
    output_ref: Option<Ref>,
    result: Option<RepairCheckResultV1>,
}
impl CheckInput {
    fn valid() -> Self {
        Self {
            source: RepairCheckSourceV1::WorkerExecution,
            command: strings(),
            exit_code: RepairExitCodeV1::try_new(format!("-{HUGE}")).unwrap(),
            code_sha: SHA.into(),
            timed_out: Some(false),
            started_at: Some(OrchestrationDateTimeV1::try_new("2026-09-08t00:00:00.100z").unwrap()),
            finished_at: Some(
                OrchestrationDateTimeV1::try_new("2025-01-01T00:00:00+01:00").unwrap(),
            ),
            output_ref: Some(reference()),
            result: Some(RepairCheckResultV1::Fail),
        }
    }
    fn build(self) -> Result<RepairCapsuleFailedCheckV1, CapsuleError> {
        RepairCapsuleFailedCheckV1::try_new(
            self.source,
            self.command,
            self.exit_code,
            self.code_sha,
            self.timed_out,
            self.started_at,
            self.finished_at,
            self.output_ref,
            self.result,
        )
    }
}
fn check() -> RepairCapsuleFailedCheckV1 {
    CheckInput::valid().build().unwrap()
}
#[test]
fn check_every_supplied_field_is_observable_exactly() {
    let input = CheckInput::valid();
    let value = input.clone().build().unwrap();
    assert_eq!(value.source(), input.source);
    assert_eq!(value.command(), input.command.as_slice());
    assert_eq!(value.exit_code(), &input.exit_code);
    assert_eq!(value.code_sha(), input.code_sha);
    assert_eq!(value.timed_out(), input.timed_out);
    assert_eq!(value.started_at(), input.started_at.as_ref());
    assert_eq!(value.finished_at(), input.finished_at.as_ref());
    assert_eq!(value.output_ref(), input.output_ref.as_ref());
    assert_eq!(value.result(), input.result);
}
#[derive(Clone)]
struct CapsuleInput {
    task_id: String,
    original_task_id: String,
    parent_attempt_id: Option<String>,
    attempt_number: RepairAttemptNumberV1,
    current_sha: String,
    original_objective: String,
    original_acceptance_criteria: Vec<Criterion>,
    a4_findings: Option<Vec<RepairCapsuleFindingV1>>,
    blocking_findings: Vec<RepairCapsuleFindingV1>,
    failed_checks: Option<Vec<RepairCapsuleFailedCheckV1>>,
    repair_scope: String,
    prior_attempt_summary: Option<String>,
    relevant_context_refs: Option<Vec<Ref>>,
    allowed_write_paths: Vec<String>,
    forbidden_write_paths: Option<Vec<String>>,
    required_quality_floor: QualityFloor,
    branch: String,
    worktree: String,
    context_epoch: RepairContextEpochV1,
    parent_quality_floor: QualityFloor,
}
impl CapsuleInput {
    fn valid() -> Self {
        Self {
            task_id: OPEN.into(),
            original_task_id: OPEN.into(),
            parent_attempt_id: Some(OPEN.into()),
            attempt_number: RepairAttemptNumberV1::try_new(HUGE).unwrap(),
            current_sha: SHA.into(),
            original_objective: OPEN.into(),
            original_acceptance_criteria: criteria(),
            a4_findings: Some(vec![finding(), other_finding(), finding()]),
            blocking_findings: vec![other_finding(), finding(), other_finding()],
            failed_checks: Some(vec![check(), check()]),
            repair_scope: OPEN.into(),
            prior_attempt_summary: Some(OPEN.into()),
            relevant_context_refs: Some(vec![
                reference(),
                Ref::try_new(RefType::Url, "Opaque".into(), None, None).unwrap(),
                reference(),
            ]),
            allowed_write_paths: strings(),
            forbidden_write_paths: Some(strings()),
            required_quality_floor: QualityFloor::Frontier,
            branch: "runtime-a3/TASK-004-R1".into(),
            worktree: OPEN.into(),
            context_epoch: RepairContextEpochV1::try_new(HUGE).unwrap(),
            parent_quality_floor: QualityFloor::Balanced,
        }
    }
    fn build(self) -> Result<RepairCapsule, CapsuleError> {
        RepairCapsule::try_new(
            self.task_id,
            self.original_task_id,
            self.parent_attempt_id,
            self.attempt_number,
            self.current_sha,
            self.original_objective,
            self.original_acceptance_criteria,
            self.a4_findings,
            self.blocking_findings,
            self.failed_checks,
            self.repair_scope,
            self.prior_attempt_summary,
            self.relevant_context_refs,
            self.allowed_write_paths,
            self.forbidden_write_paths,
            self.required_quality_floor,
            self.branch,
            self.worktree,
            self.context_epoch,
            self.parent_quality_floor,
        )
    }
}
#[test]
fn capsule_every_supplied_field_is_observable_exactly() {
    let input = CapsuleInput::valid();
    let value = input.clone().build().unwrap();
    assert_eq!(value.task_id(), input.task_id);
    assert_eq!(value.original_task_id(), input.original_task_id);
    assert_eq!(
        value.parent_attempt_id(),
        input.parent_attempt_id.as_deref()
    );
    assert_eq!(value.attempt_number(), &input.attempt_number);
    assert_eq!(value.current_sha(), input.current_sha);
    assert_eq!(value.original_objective(), input.original_objective);
    assert_eq!(
        value.original_acceptance_criteria(),
        input.original_acceptance_criteria.as_slice()
    );
    assert_eq!(value.a4_findings(), input.a4_findings.as_deref());
    assert_eq!(
        value.blocking_findings(),
        input.blocking_findings.as_slice()
    );
    assert_eq!(value.failed_checks(), input.failed_checks.as_deref());
    assert_eq!(value.repair_scope(), input.repair_scope);
    assert_eq!(
        value.prior_attempt_summary(),
        input.prior_attempt_summary.as_deref()
    );
    assert_eq!(
        value.relevant_context_refs(),
        input.relevant_context_refs.as_deref()
    );
    assert_eq!(
        value.allowed_write_paths(),
        input.allowed_write_paths.as_slice()
    );
    assert_eq!(
        value.forbidden_write_paths(),
        input.forbidden_write_paths.as_deref()
    );
    assert_eq!(value.required_quality_floor(), input.required_quality_floor);
    assert_eq!(value.branch(), input.branch);
    assert_eq!(value.worktree(), input.worktree);
    assert_eq!(value.context_epoch(), &input.context_epoch);
}
fn other_finding() -> RepairCapsuleFindingV1 {
    let mut input = FindingInput::valid();
    input.finding_id = "different".into();
    input.blocking = true;
    input.build().unwrap()
}

#[test]
fn integer_adversarial_domains_preserve_unbounded_lexical_values() {
    let long = "9".repeat(10_001);
    let negative = format!("-{long}");
    for value in ["1", "2", "18446744073709551615", HUGE, &long] {
        assert_eq!(RepairFindingLineV1::try_new(value).unwrap().as_str(), value);
    }
    for value in ["0", "1", HUGE, &long] {
        assert_eq!(
            RepairContextEpochV1::try_new(value).unwrap().as_str(),
            value
        );
    }
    for value in ["2", "3", HUGE, &long] {
        assert_eq!(
            RepairAttemptNumberV1::try_new(value).unwrap().as_str(),
            value
        );
    }
    for value in [
        "0",
        "1",
        "-1",
        HUGE,
        "-18446744073709551616",
        "-9223372036854775809",
        &long,
        &negative,
    ] {
        assert_eq!(RepairExitCodeV1::try_new(value).unwrap().as_str(), value);
    }
    for value in [
        "", "00", "01", "+1", "+2", "1.0", "2.0", "1e0", "2e0", " ", " 1", "1 ", "1\n", "\t2", "١",
        "２", "1🦀", "🦀", "−1", "\0",
    ] {
        assert!(
            RepairFindingLineV1::try_new(value).is_err(),
            "line: {value:?}"
        );
        assert!(
            RepairContextEpochV1::try_new(value).is_err(),
            "epoch: {value:?}"
        );
        assert!(
            RepairAttemptNumberV1::try_new(value).is_err(),
            "attempt: {value:?}"
        );
        assert!(RepairExitCodeV1::try_new(value).is_err(), "exit: {value:?}");
    }
    for value in ["0", "-1", "-2", "-0", "-01"] {
        assert!(RepairFindingLineV1::try_new(value).is_err());
        assert!(RepairAttemptNumberV1::try_new(value).is_err());
    }
    assert!(RepairAttemptNumberV1::try_new("1").is_err());
    for value in ["-1", "-2", "-0", "-01"] {
        assert!(RepairContextEpochV1::try_new(value).is_err());
    }
    for value in ["-0", "-01", "--1", "-1.0", "-1e0", "-١"] {
        assert!(RepairExitCodeV1::try_new(value).is_err());
    }
}

#[test]
fn quality_floor_all_nine_explicit_parent_child_combinations() {
    use QualityFloor::{Balanced, Economy, Frontier};
    for (parent, child, accepted) in [
        (Frontier, Frontier, true),
        (Frontier, Balanced, false),
        (Frontier, Economy, false),
        (Balanced, Frontier, true),
        (Balanced, Balanced, true),
        (Balanced, Economy, false),
        (Economy, Frontier, true),
        (Economy, Balanced, true),
        (Economy, Economy, true),
    ] {
        let mut input = CapsuleInput::valid();
        input.parent_quality_floor = parent;
        input.required_quality_floor = child;
        let result = input.build();
        if accepted {
            assert_eq!(result.unwrap().required_quality_floor(), child);
        } else {
            assert_eq!(result, Err(CapsuleError::RepairQualityFloorWeakened));
        }
    }
}

#[test]
fn identifiers_count_unicode_scalars_without_normalization() {
    for field in [
        "task_id",
        "original_task_id",
        "parent_attempt_id",
        "finding_id",
    ] {
        for value in [
            String::new(),
            "🦀".repeat(200),
            "🦀".repeat(201),
            OPEN.into(),
            " ".into(),
        ] {
            let mut input = CapsuleInput::valid();
            let result = if field == "finding_id" {
                let mut finding = FindingInput::valid();
                finding.finding_id = value.clone();
                finding.build().map(|f| f.finding_id().to_owned())
            } else {
                match field {
                    "task_id" => input.task_id = value.clone(),
                    "original_task_id" => input.original_task_id = value.clone(),
                    "parent_attempt_id" => input.parent_attempt_id = Some(value.clone()),
                    _ => unreachable!(),
                }
                input.build().map(|c| match field {
                    "task_id" => c.task_id().to_owned(),
                    "original_task_id" => c.original_task_id().to_owned(),
                    "parent_attempt_id" => c.parent_attempt_id().unwrap().to_owned(),
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
fn required_nonempty_fields_reject() {
    for field in [
        "original_objective",
        "original_acceptance_criteria",
        "blocking_findings",
        "repair_scope",
        "allowed_write_paths",
    ] {
        let mut input = CapsuleInput::valid();
        match field {
            "original_objective" => input.original_objective.clear(),
            "original_acceptance_criteria" => input.original_acceptance_criteria.clear(),
            "blocking_findings" => input.blocking_findings.clear(),
            "repair_scope" => input.repair_scope.clear(),
            "allowed_write_paths" => input.allowed_write_paths.clear(),
            _ => unreachable!(),
        }
        assert_eq!(input.build(), Err(CapsuleError::EmptyField { field }));
    }
    let mut input = FindingInput::valid();
    input.description.clear();
    assert_eq!(
        input.build(),
        Err(CapsuleError::EmptyField {
            field: "description"
        })
    );
    let mut input = CheckInput::valid();
    input.command.clear();
    assert_eq!(
        input.build(),
        Err(CapsuleError::EmptyField { field: "command" })
    );
}

#[test]
fn both_sha_fields_reject_malformed_ascii_and_unicode() {
    for value in [
        String::new(),
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        "ａ".repeat(40),
        "🦀".repeat(10),
        format!("{SHA}\n"),
        format!(" {SHA}"),
    ] {
        let mut capsule = CapsuleInput::valid();
        capsule.current_sha = value.clone();
        assert_eq!(
            capsule.build(),
            Err(CapsuleError::InvalidSha {
                field: "current_sha"
            })
        );
        let mut check = CheckInput::valid();
        check.code_sha = value;
        assert_eq!(
            check.build(),
            Err(CapsuleError::InvalidSha { field: "code_sha" })
        );
    }
}

#[test]
fn branch_prefix_is_only_data_constraint() {
    for value in [
        "",
        "build/foo",
        "feature/foo",
        "runtime-a4/foo",
        "Runtime-a3/foo",
        " runtime-a3/foo",
        "🦀",
    ] {
        let mut input = CapsuleInput::valid();
        input.branch = value.into();
        assert_eq!(input.build(), Err(CapsuleError::InvalidBranch));
    }
    for value in [
        "runtime-a3/",
        "runtime-a3/TASK-004-R1",
        "runtime-a3/ 🦀 ../\n",
    ] {
        let mut input = CapsuleInput::valid();
        input.branch = value.into();
        assert_eq!(input.build().unwrap().branch(), value);
    }
}

#[test]
fn option_absence_and_present_empty_remain_distinct() {
    let mut input = CapsuleInput::valid();
    input.parent_attempt_id = None;
    input.a4_findings = None;
    input.failed_checks = None;
    input.prior_attempt_summary = None;
    input.relevant_context_refs = None;
    input.forbidden_write_paths = None;
    let absent = input.clone().build().unwrap();
    assert_eq!(absent.parent_attempt_id(), None);
    assert_eq!(absent.a4_findings(), None);
    assert_eq!(absent.failed_checks(), None);
    assert_eq!(absent.prior_attempt_summary(), None);
    assert_eq!(absent.relevant_context_refs(), None);
    assert_eq!(absent.forbidden_write_paths(), None);
    input.a4_findings = Some(vec![]);
    input.failed_checks = Some(vec![]);
    input.prior_attempt_summary = Some(String::new());
    input.relevant_context_refs = Some(vec![]);
    input.forbidden_write_paths = Some(vec![]);
    input.worktree.clear();
    input.allowed_write_paths = vec![String::new()];
    let present = input.build().unwrap();
    assert_eq!(present.a4_findings(), Some([].as_slice()));
    assert_eq!(present.failed_checks(), Some([].as_slice()));
    assert_eq!(present.prior_attempt_summary(), Some(""));
    assert_eq!(present.relevant_context_refs(), Some([].as_slice()));
    assert_eq!(present.forbidden_write_paths(), Some([].as_slice()));
    assert_eq!(present.worktree(), "");
    assert_eq!(present.allowed_write_paths(), &[String::new()]);
    let mut finding = FindingInput::valid();
    finding.path = None;
    finding.line = None;
    finding.evidence_ref = None;
    finding.confidence = None;
    finding.source = None;
    let finding = finding.build().unwrap();
    assert_eq!(finding.path(), None);
    assert_eq!(finding.line(), None);
    assert_eq!(finding.evidence_ref(), None);
    assert_eq!(finding.confidence(), None);
    assert_eq!(finding.source(), None);
    let mut check = CheckInput::valid();
    check.timed_out = None;
    check.started_at = None;
    check.finished_at = None;
    check.output_ref = None;
    check.result = None;
    let check = check.build().unwrap();
    assert_eq!(check.timed_out(), None);
    assert_eq!(check.started_at(), None);
    assert_eq!(check.finished_at(), None);
    assert_eq!(check.output_ref(), None);
    assert_eq!(check.result(), None);
}

#[test]
fn historical_argv_exit_codes_and_timeout_are_preserved_without_execution() {
    for argv in [
        vec![String::new()],
        vec!["not-an-executable".into()],
        strings(),
        vec!["$(never execute)".into(), ";".into(), "\0".into()],
    ] {
        for code in [
            HUGE.to_owned(),
            format!("-{HUGE}"),
            "8".repeat(10_001),
            format!("-{}", "8".repeat(10_001)),
        ] {
            for timed_out in [None, Some(true), Some(false)] {
                let mut input = CheckInput::valid();
                input.command = argv.clone();
                input.exit_code = RepairExitCodeV1::try_new(code.clone()).unwrap();
                input.timed_out = timed_out;
                let check = input.build().unwrap();
                assert_eq!(check.command(), argv);
                assert_eq!(check.exit_code().as_str(), code);
                assert_eq!(check.timed_out(), timed_out);
            }
        }
    }
}

#[test]
fn original_target_and_rejected_sha_survive_independent_findings() {
    let input = CapsuleInput::valid();
    let before = input.clone().build().unwrap();
    let mut changed = input;
    changed.a4_findings = None;
    changed.blocking_findings = vec![finding()]; // false flag is lawful; no filtering/coupling.
    changed.repair_scope = "a different supplied scope".into();
    let after = changed.build().unwrap();
    assert_eq!(before.original_objective(), OPEN);
    assert_eq!(after.original_objective(), before.original_objective());
    assert_eq!(
        after.original_acceptance_criteria(),
        before.original_acceptance_criteria()
    );
    assert_eq!(after.original_acceptance_criteria(), criteria());
    assert_eq!(after.current_sha(), SHA);
    assert_eq!(after.blocking_findings(), &[finding()]);
    assert_eq!(after.a4_findings(), None);
    let r = &after.relevant_context_refs().unwrap()[0];
    assert_eq!(r.target(), OPEN);
    assert_eq!(r.digest(), Some(OPEN));
    assert_eq!(r.section(), Some(OPEN));
}

#[test]
fn exact_snapshot_vocabularies_and_every_variant_storage() {
    assert_eq!(
        RepairFindingSeverityV1::ALL.map(|v| v.as_str()),
        ["INFO", "LOW", "MEDIUM", "HIGH", "CRITICAL"]
    );
    for value in RepairFindingSeverityV1::ALL {
        let mut input = FindingInput::valid();
        input.severity = value;
        assert_eq!(input.build().unwrap().severity(), value);
    }
    assert_eq!(
        RepairFindingCategoryV1::ALL.map(|v| v.as_str()),
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
            "DOC_GAP"
        ]
    );
    for value in RepairFindingCategoryV1::ALL {
        let mut input = FindingInput::valid();
        input.category = value;
        assert_eq!(input.build().unwrap().category(), value);
    }
    assert_eq!(
        RepairFindingConfidenceV1::ALL.map(|v| v.as_str()),
        ["HIGH", "MEDIUM", "LOW"]
    );
    for value in RepairFindingConfidenceV1::ALL {
        let mut input = FindingInput::valid();
        input.confidence = Some(value);
        assert_eq!(input.build().unwrap().confidence(), Some(value));
    }
    assert_eq!(
        RepairFindingSourceV1::ALL.map(|v| v.as_str()),
        [
            "LLM_REVIEW",
            "STATIC_ANALYSIS",
            "DEPENDENCY_SCAN",
            "TEST",
            "CONFIG_CHECK"
        ]
    );
    for value in RepairFindingSourceV1::ALL {
        let mut input = FindingInput::valid();
        input.source = Some(value);
        assert_eq!(input.build().unwrap().source(), Some(value));
    }
    assert_eq!(
        RepairCheckSourceV1::ALL.map(|v| v.as_str()),
        [
            "WORKER_EXECUTION",
            "BROKER_EXECUTION",
            "REVIEW_EXECUTION",
            "GIT_PROVENANCE"
        ]
    );
    for value in RepairCheckSourceV1::ALL {
        let mut input = CheckInput::valid();
        input.source = value;
        assert_eq!(input.build().unwrap().source(), value);
    }
    assert_eq!(
        RepairCheckResultV1::ALL.map(|v| v.as_str()),
        ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]
    );
    for value in RepairCheckResultV1::ALL {
        let mut input = CheckInput::valid();
        input.result = Some(value);
        assert_eq!(input.build().unwrap().result(), Some(value));
    }
}
