use crate::*;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
type Error = IntegrationRequestConstructionError;
type Nullable = IntegrationDecisionNullableString;
type Signed = IntegrationRequestSignedInteger;
type Positive = IntegrationRequestPositiveInteger;

fn task(id: &str) -> IntegrationRequestTask {
    IntegrationRequestTask::new(
        id.into(),
        SHA.into(),
        OTHER_SHA.into(),
        IntegrationRequestA4Verdict::Pass,
        None,
        Nullable::Omitted,
    )
    .unwrap()
}
fn request(
    id: &str,
    sha: &str,
    tasks: Vec<IntegrationRequestTask>,
) -> Result<IntegrationRequestNonTemporalCore, Error> {
    IntegrationRequestNonTemporalCore::new(
        id.into(),
        IntegrationRequestGateLevel::A2Acceptance,
        "".into(),
        sha.into(),
        "".into(),
        tasks,
        None,
        None,
        None,
        None,
        None,
    )
}
fn finding(
    id: &str,
    description: &str,
    blocking: bool,
) -> Result<IntegrationRequestOpenFinding, Error> {
    IntegrationRequestOpenFinding::new(
        id.into(),
        A4ReviewFindingSeverity::Info,
        A4ReviewFindingCategory::Correctness,
        description.into(),
        blocking,
        None,
        None,
        None,
        None,
        None,
    )
}
fn check(command: Vec<String>, sha: &str) -> Result<IntegrationRequestPostMergeCheck, Error> {
    IntegrationRequestPostMergeCheck::new(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        command,
        Signed::from_decimal("0").unwrap(),
        sha.into(),
        None,
        None,
        None,
    )
}
fn reference(kind: WorkspaceCheckpointRefType) -> WorkspaceCheckpointRef {
    WorkspaceCheckpointRef::new(kind, " ", Some("".into()), Some("".into())).unwrap()
}

#[test]
fn minimum_request_and_clone() {
    let value = request("r", SHA, vec![task("t")]).unwrap();
    assert_eq!(value.request_id(), "r");
    assert_eq!(
        value.gate_level(),
        IntegrationRequestGateLevel::A2Acceptance
    );
    assert_eq!(value.source_branch(), "");
    assert_eq!(value.source_sha(), &CommitSha::parse(SHA).unwrap());
    assert_eq!(value.target_ref(), "");
    assert_eq!(value.tasks_included(), &[task("t")]);
    assert_eq!(value.assurance_profile(), None);
    assert_eq!(value.post_merge_checks(), None);
    assert_eq!(value.open_nonblocking_findings(), None);
    assert_eq!(value.known_limitations(), None);
    assert_eq!(value.attestations(), None);
    assert_eq!(value, value.clone());
    assert_eq!(value, request("r", SHA, vec![task("t")]).unwrap());
    assert_eq!(value.tasks_included()[0].implementation_sha().as_str(), SHA);
    assert_eq!(value.tasks_included()[0].review_sha().as_str(), OTHER_SHA);
    assert_eq!(value.tasks_included()[0].findings_disposition(), None);
    assert_eq!(value.tasks_included()[0].merge_commit(), &Nullable::Omitted);
}

#[test]
fn identifier_character_boundaries_and_exact_preservation() {
    for unit in ["a", "界", "🦀"] {
        for len in [0, 1, 200, 201] {
            let id = unit.repeat(len);
            let valid = (1..=200).contains(&len);
            let r = request(&id, SHA, vec![task("t")]);
            let t = IntegrationRequestTask::new(
                id.clone(),
                SHA.into(),
                OTHER_SHA.into(),
                IntegrationRequestA4Verdict::Pass,
                None,
                Nullable::Omitted,
            );
            let f = finding(&id, " ", false);
            if valid {
                assert_eq!(r.unwrap().request_id(), id);
                assert_eq!(t.unwrap().task_id(), id);
                assert_eq!(f.unwrap().finding_id(), id);
            } else {
                assert_eq!(r.unwrap_err(), Error::InvalidIdentifier("request_id"));
                assert_eq!(t.unwrap_err(), Error::InvalidIdentifier("task_id"));
                assert_eq!(f.unwrap_err(), Error::InvalidIdentifier("finding_id"));
            }
        }
    }
    for id in [" ", " e\u{301}\t\n", "É"] {
        assert_eq!(request(id, SHA, vec![task(id)]).unwrap().request_id(), id);
        assert_eq!(task(id).task_id(), id);
        assert_eq!(finding(id, " ", false).unwrap().finding_id(), id);
    }
}

#[test]
fn required_nonempty_collections_and_description() {
    assert_eq!(
        request("r", SHA, vec![]).unwrap_err(),
        Error::EmptyTasksIncluded
    );
    assert_eq!(check(vec![], SHA).unwrap_err(), Error::EmptyCommand);
    assert_eq!(
        finding("f", "", false).unwrap_err(),
        Error::EmptyFindingDescription
    );
    assert_eq!(finding("f", " ", false).unwrap().description(), " ");
    assert_eq!(check(vec!["".into()], SHA).unwrap().command(), &[""]);
}

#[test]
fn each_sha_field_independently_rejects_malformed_values() {
    for bad in [
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        "abc1234".into(),
        "HEAD".into(),
        "é".repeat(20),
    ] {
        assert_eq!(
            request("r", &bad, vec![task("t")]).unwrap_err(),
            Error::MalformedSha("source_sha")
        );
        assert_eq!(
            IntegrationRequestTask::new(
                "t".into(),
                bad.clone(),
                OTHER_SHA.into(),
                IntegrationRequestA4Verdict::Pass,
                None,
                Nullable::Omitted
            )
            .unwrap_err(),
            Error::MalformedSha("implementation_sha")
        );
        assert_eq!(
            IntegrationRequestTask::new(
                "t".into(),
                SHA.into(),
                bad.clone(),
                IntegrationRequestA4Verdict::Pass,
                None,
                Nullable::Omitted
            )
            .unwrap_err(),
            Error::MalformedSha("review_sha")
        );
        assert_eq!(
            check(vec!["".into()], &bad).unwrap_err(),
            Error::MalformedSha("code_sha")
        );
    }
}

#[test]
fn gate_and_narrow_verdict_exact_vocabularies() {
    assert_eq!(
        IntegrationRequestGateLevel::ALL.map(|v| v.as_str()),
        ["A2_ACCEPTANCE", "A1_INTEGRATION"]
    );
    assert_eq!(
        IntegrationRequestA4Verdict::ALL.map(|v| v.as_str()),
        ["PASS", "PASS_WITH_NONBLOCKING_FINDINGS"]
    );
    for gate in IntegrationRequestGateLevel::ALL {
        for verdict in IntegrationRequestA4Verdict::ALL {
            let t = IntegrationRequestTask::new(
                "t".into(),
                SHA.into(),
                OTHER_SHA.into(),
                verdict,
                Some("arbitrary".into()),
                Nullable::String("not-a-sha".into()),
            )
            .unwrap();
            let r = IntegrationRequestNonTemporalCore::new(
                "r".into(),
                gate,
                "".into(),
                SHA.into(),
                "".into(),
                vec![t],
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!(r.gate_level(), gate);
            assert_eq!(r.tasks_included()[0].a4_verdict(), verdict);
        }
    }
}

#[test]
fn opaque_strings_and_task_order_duplicates() {
    for opaque in ["", " ", "not a ref: 🦀\n"] {
        let tasks = vec![task("b"), task("a"), task("b")];
        let r = IntegrationRequestNonTemporalCore::new(
            "r".into(),
            IntegrationRequestGateLevel::A1Integration,
            opaque.into(),
            SHA.into(),
            opaque.into(),
            tasks.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.source_branch(), opaque);
        assert_eq!(r.target_ref(), opaque);
        assert_eq!(r.tasks_included(), tasks);
        let t = IntegrationRequestTask::new(
            "t".into(),
            SHA.into(),
            OTHER_SHA.into(),
            IntegrationRequestA4Verdict::Pass,
            Some(opaque.into()),
            Nullable::String(opaque.into()),
        )
        .unwrap();
        assert_eq!(t.findings_disposition(), Some(opaque));
        assert_eq!(t.merge_commit(), &Nullable::String(opaque.into()));
    }
}

#[test]
fn merge_commit_all_structural_states_are_distinct() {
    let states = [
        Nullable::Omitted,
        Nullable::Null,
        Nullable::String("".into()),
        Nullable::String("x".into()),
        Nullable::String("not-a-sha".into()),
        Nullable::String(SHA.into()),
    ];
    for (i, state) in states.iter().enumerate() {
        let t = IntegrationRequestTask::new(
            "t".into(),
            SHA.into(),
            OTHER_SHA.into(),
            IntegrationRequestA4Verdict::Pass,
            None,
            state.clone(),
        )
        .unwrap();
        assert_eq!(t.merge_commit(), state);
        for (j, other) in states.iter().enumerate() {
            assert_eq!(state == other, i == j);
        }
    }
}

#[test]
fn arbitrary_integer_values_and_canonical_equality() {
    let zero = Signed::from_decimal("0").unwrap();
    for spelling in ["0", "+0", "-0", "000", "-000", "+000"] {
        let value = Signed::from_decimal(spelling).unwrap();
        assert_eq!(value, zero);
        assert!(!value.is_negative());
        assert_eq!(value.decimal_digits(), "0");
        assert_eq!(
            Positive::from_decimal(spelling).unwrap_err(),
            Error::NonPositiveInteger
        );
    }
    for (spelling, negative) in [
        ("00123", false),
        ("+123", false),
        ("-00123", true),
        ("-123", true),
    ] {
        let value = Signed::from_decimal(spelling).unwrap();
        assert_eq!(value.is_negative(), negative);
        assert_eq!(value.decimal_digits(), "123");
        assert_eq!(
            value,
            Signed::from_decimal(if negative { "-123" } else { "123" }).unwrap()
        );
    }
    assert_ne!(
        Signed::from_decimal("123").unwrap(),
        Signed::from_decimal("-123").unwrap()
    );
    for magnitude in [
        "1".into(),
        "170141183460469231731687303715884105728".into(),
        "170141183460469231731687303715884105729".into(),
        "340282366920938463463374607431768211456".into(),
        "1234567890".repeat(1000),
    ] {
        for negative in [false, true] {
            let spelling = format!("{}{magnitude}", if negative { "-" } else { "+000" });
            let value = Signed::from_decimal(&spelling).unwrap();
            assert_eq!(value.decimal_digits(), magnitude);
            assert_eq!(value.is_negative(), negative);
            assert_eq!(value, value.clone());
            let c = IntegrationRequestPostMergeCheck::new(
                WorkspaceCheckpointCheckSource::GitProvenance,
                vec!["".into()],
                value.clone(),
                SHA.into(),
                None,
                None,
                None,
            )
            .unwrap();
            assert_eq!(c.exit_code(), &value);
            if negative {
                assert_eq!(
                    Positive::from_decimal(&spelling).unwrap_err(),
                    Error::NonPositiveInteger
                );
            } else {
                let line = Positive::from_decimal(&spelling).unwrap();
                assert_eq!(line.decimal_digits(), magnitude);
                assert_eq!(line, line.clone());
                assert_eq!(line, Positive::from_decimal(&magnitude).unwrap());
                let f = IntegrationRequestOpenFinding::new(
                    "f".into(),
                    A4ReviewFindingSeverity::Info,
                    A4ReviewFindingCategory::Style,
                    "d".into(),
                    true,
                    None,
                    Some(line.clone()),
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(f.line(), Some(&line));
            }
        }
    }
}

#[test]
fn integer_convenience_constructor_rejects_non_decimal_spellings() {
    for spelling in [
        "", "+", "-", "1.5", "1.0", "1e3", " 1", "1 ", "++1", "--1", "+-1", "１２", "١", "0x10",
        "1_000", "1\n",
    ] {
        assert_eq!(
            Signed::from_decimal(spelling).unwrap_err(),
            Error::InvalidDecimalInteger
        );
        assert_eq!(
            Positive::from_decimal(spelling).unwrap_err(),
            Error::InvalidDecimalInteger
        );
    }
    assert_eq!(
        Positive::from_decimal("-1").unwrap_err(),
        Error::NonPositiveInteger
    );
}

#[test]
fn post_merge_check_vocabulary_optionals_and_argv_preservation() {
    assert_eq!(
        WorkspaceCheckpointCheckSource::ALL.map(|v| v.as_str()),
        [
            "WORKER_EXECUTION",
            "BROKER_EXECUTION",
            "REVIEW_EXECUTION",
            "GIT_PROVENANCE"
        ]
    );
    assert_eq!(
        ReviewCapsuleCheckResult::ALL.map(|v| v.as_str()),
        ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]
    );
    let minimal = check(vec!["".into()], SHA).unwrap();
    assert_eq!(minimal.exit_code(), &Signed::from_decimal("0").unwrap());
    assert_eq!(minimal.code_sha().as_str(), SHA);
    assert_eq!(minimal.timed_out(), None);
    assert_eq!(minimal.output_ref(), None);
    assert_eq!(minimal.result(), None);
    for source in WorkspaceCheckpointCheckSource::ALL {
        for result in ReviewCapsuleCheckResult::ALL {
            let mut variants = Vec::new();
            for timed_out in [None, Some(false), Some(true)] {
                let argv = vec![
                    "".into(),
                    "arg".into(),
                    "".into(),
                    "$(never executed)".into(),
                ];
                let output = reference(WorkspaceCheckpointRefType::ArtifactId);
                let c = IntegrationRequestPostMergeCheck::new(
                    source,
                    argv.clone(),
                    Signed::from_decimal("-1").unwrap(),
                    SHA.into(),
                    timed_out,
                    Some(output.clone()),
                    Some(result),
                )
                .unwrap();
                assert_eq!(c.source(), source);
                assert_eq!(c.command(), argv);
                assert_eq!(c.result(), Some(result));
                assert_eq!(c.timed_out(), timed_out);
                assert_eq!(c.output_ref(), Some(&output));
                assert_eq!(c, c.clone());
                variants.push(c);
            }
            assert_ne!(variants[0], variants[1]);
            assert_ne!(variants[1], variants[2]);
            assert_ne!(variants[0], variants[2]);
        }
    }
}

#[test]
fn references_reuse_exact_domain_in_both_records() {
    assert_eq!(
        WorkspaceCheckpointRefType::ALL.map(|v| v.as_str()),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    for kind in WorkspaceCheckpointRefType::ALL {
        assert!(WorkspaceCheckpointRef::new(kind, "", None, None).is_err());
        let r = reference(kind);
        assert_eq!(r.ref_type(), kind);
        assert_eq!(r.target(), " ");
        assert_eq!(r.digest(), Some(""));
        assert_eq!(r.section(), Some(""));
        let c = IntegrationRequestPostMergeCheck::new(
            WorkspaceCheckpointCheckSource::ReviewExecution,
            vec!["".into()],
            Signed::from_decimal("0").unwrap(),
            SHA.into(),
            None,
            Some(r.clone()),
            None,
        )
        .unwrap();
        let f = IntegrationRequestOpenFinding::new(
            "f".into(),
            A4ReviewFindingSeverity::Info,
            A4ReviewFindingCategory::DocGap,
            "d".into(),
            false,
            None,
            None,
            Some(r.clone()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(c.output_ref(), Some(&r));
        assert_eq!(f.evidence_ref(), Some(&r));
    }
}

#[test]
fn finding_vocabulary_and_blocking_are_data() {
    assert_eq!(
        A4ReviewFindingSeverity::ALL.map(|v| v.as_str()),
        ["INFO", "LOW", "MEDIUM", "HIGH", "CRITICAL"]
    );
    assert_eq!(
        A4ReviewFindingCategory::ALL.map(|v| v.as_str()),
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
    assert_eq!(
        A4ReviewFindingConfidence::ALL.map(|v| v.as_str()),
        ["HIGH", "MEDIUM", "LOW"]
    );
    assert_eq!(
        A4ReviewFindingSource::ALL.map(|v| v.as_str()),
        [
            "LLM_REVIEW",
            "STATIC_ANALYSIS",
            "DEPENDENCY_SCAN",
            "TEST",
            "CONFIG_CHECK"
        ]
    );
    let minimal = finding("f", "d", false).unwrap();
    assert_eq!(minimal.path(), None);
    assert_eq!(minimal.line(), None);
    assert_eq!(minimal.evidence_ref(), None);
    assert_eq!(minimal.confidence(), None);
    assert_eq!(minimal.source(), None);
    for severity in A4ReviewFindingSeverity::ALL {
        for category in A4ReviewFindingCategory::ALL {
            for confidence in A4ReviewFindingConfidence::ALL {
                for source in A4ReviewFindingSource::ALL {
                    for blocking in [false, true] {
                        for path in [None, Some("".into()), Some("not/a/resolved/path".into())] {
                            let f = IntegrationRequestOpenFinding::new(
                                "f".into(),
                                severity,
                                category,
                                " ".into(),
                                blocking,
                                path.clone(),
                                None,
                                None,
                                Some(confidence),
                                Some(source),
                            )
                            .unwrap();
                            assert_eq!(f.severity(), severity);
                            assert_eq!(f.category(), category);
                            assert_eq!(f.confidence(), Some(confidence));
                            assert_eq!(f.source(), Some(source));
                            assert_eq!(f.blocking(), blocking);
                            assert_eq!(f.path(), path.as_deref());
                            assert_eq!(f, f.clone());
                        }
                    }
                }
            }
        }
    }
}

fn attestations(values: [Option<bool>; 6]) -> IntegrationRequestAttestations {
    IntegrationRequestAttestations::new(
        values[0], values[1], values[2], values[3], values[4], values[5],
    )
}
fn attestation_values(a: &IntegrationRequestAttestations) -> [Option<bool>; 6] {
    [
        a.all_tasks_accepted(),
        a.review_sha_equals_implementation_sha(),
        a.no_commit_after_review(),
        a.no_unresolved_blocking_finding(),
        a.no_frozen_artifact_modified(),
        a.no_out_of_scope_write(),
    ]
}

#[test]
fn attestations_preserve_every_optional_boolean_combination() {
    // Exhaust all 3^6 subsets/values, including empty, all true/false, and mixed.
    let mut seen = Vec::new();
    for mut code in 0..729 {
        let values = std::array::from_fn(|_| {
            let value = [None, Some(false), Some(true)][code % 3];
            code /= 3;
            value
        });
        let a = attestations(values);
        assert_eq!(attestation_values(&a), values);
        assert_eq!(a, a.clone());
        assert!(!seen.contains(&a));
        seen.push(a);
    }
}

#[test]
fn optional_arrays_and_attestation_object_distinguish_absent_empty_nonempty() {
    let absent = request("r", SHA, vec![task("t")]).unwrap();
    let empty = IntegrationRequestNonTemporalCore::new(
        "r".into(),
        IntegrationRequestGateLevel::A2Acceptance,
        "".into(),
        SHA.into(),
        "".into(),
        vec![task("t")],
        None,
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        Some(attestations([None; 6])),
    )
    .unwrap();
    assert_eq!(empty.post_merge_checks(), Some([].as_slice()));
    assert_eq!(empty.open_nonblocking_findings(), Some([].as_slice()));
    assert_eq!(empty.known_limitations(), Some([].as_slice()));
    assert_eq!(empty.attestations(), Some(&attestations([None; 6])));
    assert_ne!(absent, empty);
    // Independently vary each optional property against the minimum.
    for field in 0..4 {
        let r = IntegrationRequestNonTemporalCore::new(
            "r".into(),
            IntegrationRequestGateLevel::A2Acceptance,
            "".into(),
            SHA.into(),
            "".into(),
            vec![task("t")],
            None,
            (field == 0).then(Vec::new),
            (field == 1).then(Vec::new),
            (field == 2).then(Vec::new),
            (field == 3).then(|| attestations([None; 6])),
        )
        .unwrap();
        assert_ne!(absent, r);
    }
    assert_eq!(
        AssuranceProfile::ALL.map(|v| v.as_str()),
        ["LIGHT", "STANDARD", "HIGH_ASSURANCE"]
    );
    for profile in AssuranceProfile::ALL {
        let checks = vec![
            check(vec!["b".into()], SHA).unwrap(),
            check(vec!["a".into()], SHA).unwrap(),
            check(vec!["b".into()], SHA).unwrap(),
        ];
        let findings = vec![
            finding("b", "d", true).unwrap(),
            finding("a", "d", false).unwrap(),
            finding("b", "d", true).unwrap(),
        ];
        for limitations in [
            vec!["".to_string()],
            vec!["b".into(), "".into(), "a".into(), "b".into()],
        ] {
            let a = attestations([
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                None,
                Some(true),
            ]);
            let r = IntegrationRequestNonTemporalCore::new(
                "r".into(),
                IntegrationRequestGateLevel::A1Integration,
                "".into(),
                SHA.into(),
                "".into(),
                vec![task("t")],
                Some(profile),
                Some(checks.clone()),
                Some(findings.clone()),
                Some(limitations.clone()),
                Some(a.clone()),
            )
            .unwrap();
            assert_eq!(r.assurance_profile(), Some(profile));
            assert_eq!(r.post_merge_checks(), Some(checks.as_slice()));
            assert_eq!(r.open_nonblocking_findings(), Some(findings.as_slice()));
            assert_eq!(r.known_limitations(), Some(limitations.as_slice()));
            assert_eq!(r.attestations(), Some(&a));
            assert_eq!(r, r.clone());
        }
    }
}
