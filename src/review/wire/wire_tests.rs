//! RI-WIRE conformance: one valid maximal and one minimal synthetic fixture
//! per schema round-trip; negative controls cover unknown/duplicate/missing
//! members, types, enums, patterns, `minItems`, nulls and integer domains.
//! State's independent `DecisionRecordV1` shape validator is used as a
//! second oracle for the four kinds State stores.

use super::ReviewWireErrorKind as K;
use super::*;
use receipts_state::{DecisionRecordKind, DecisionRecordV1};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn r#ref() -> String {
    r#"{"ref_type":"REPO_PATH","target":"src/x.rs","digest":"sha256:00","section":"s"}"#.into()
}
fn check(source: &str, sha: &str) -> String {
    format!(
        r#"{{"source":"{source}","command":["cargo","test"],"exit_code":0,"code_sha":"{sha}","timed_out":false,"started_at":"2026-11-08T00:00:00Z","finished_at":"2026-11-08T00:00:01Z","output_ref":{},"result":"PASS"}}"#,
        r#ref()
    )
}
fn finding(id: &str, blocking: bool) -> String {
    format!(
        r#"{{"finding_id":"{id}","severity":"HIGH","category":"CORRECTNESS","description":"d","blocking":{blocking},"path":"src/x.rs","line":3,"evidence_ref":{},"confidence":"HIGH","source":"LLM_REVIEW"}}"#,
        r#ref()
    )
}

fn review_request_full() -> String {
    format!(
        r#"{{"request_id":"rq","task_id":"t","attempt_id":"a","workstream_id":"w","baseline_sha":"{A}","implementation_sha":"{B}","branch":"","objective":"o","acceptance_criteria":[{{"id":"c1","description":"d","kind":"DETERMINISTIC","check_command":["true"],"rationale":""}}],"non_goals":[],"allowed_write_paths":["src/**"],"architecture_refs":[{r}],"contract_refs":[],"a3_handoff_ref":{r},"assurance_profile":"STANDARD","review_scope":"FULL","review_policy":{{"required":true,"distinct_provider":"REQUIRED","reviewer_floor":"FRONTIER","excluded_session_refs":["s1"]}},"prior_review_ids":["p"],"attempt_number":2,"requested_at":"2026-11-08T00:00:00Z","context_epoch":123456789012345678901234567890}}"#,
        r = r#ref()
    )
}
fn review_request_min() -> String {
    format!(
        r#"{{"request_id":"rq","task_id":"t","attempt_id":"a","baseline_sha":"{A}","implementation_sha":"{B}","objective":"o","acceptance_criteria":[{{"id":"c1","description":"d","kind":"SEMANTIC"}}],"allowed_write_paths":["src/**"],"assurance_profile":"LIGHT","review_scope":"SECURITY","context_epoch":0}}"#
    )
}
fn capsule_full() -> String {
    format!(
        r#"{{"review_id":"rv","task_id":"t","attempt_id":"a","baseline_sha":"{A}","implementation_sha":"{B}","objective":"o","acceptance_criteria":[{{"id":"c1","description":"d","kind":"DETERMINISTIC","check_command":["true"]}}],"non_goals":["n"],"architecture_refs":[],"contract_refs":[{r}],"diff":{r},"allowed_write_paths":["src/**"],"checks":[{c}],"security_requirements":[],"review_scope":"REPAIR_VERIFICATION","severity_policy":{{"blocking_categories":["CORRECTNESS"],"nonblocking_categories":[]}},"reproduction_required":true,"structured_output_schema":{r},"context_epoch":7}}"#,
        r = r#ref(),
        c = check("WORKER_EXECUTION", B)
    )
}
fn capsule_min() -> String {
    format!(
        r#"{{"review_id":"rv","task_id":"t","attempt_id":"a","baseline_sha":"{A}","implementation_sha":"{B}","objective":"o","acceptance_criteria":[{{"id":"c1","description":"d","kind":"SEMANTIC"}}],"diff":{{"ref_type":"ARTIFACT_ID","target":"diff"}},"allowed_write_paths":["src/**"],"review_scope":"FULL","severity_policy":{{"blocking_categories":["X"]}},"reproduction_required":false,"context_epoch":0}}"#
    )
}
fn handoff_full(blocker: &str) -> String {
    format!(
        r#"{{"task_id":"t","attempt_id":"a","implementer":{{"provider_id":"p","model_id":"m","runtime_id":"r","binding_id":"b"}},"start_sha":"{A}","final_sha":"{B}","branch":"br","files_changed":["src/x.rs"],"files_created":[],"files_deleted":["y"],"contracts_consumed":[{{"contract_id":"c","version":"1"}}],"implementation_summary":"s","checks":[{c}],"not_run":[],"evidence_labels":[{{"claim":"c","label":"TESTED"}}],"known_limitations":["k"],"assumptions":[],"security_notes":"","subtask_requests":["st"],"open_questions":[],"evidence_refs":[{r}],"ready_for_a4":true{blocker}}}"#,
        r = r#ref(),
        c = check("WORKER_EXECUTION", B)
    )
}
fn handoff_min() -> String {
    format!(
        r#"{{"task_id":"t","attempt_id":"a","start_sha":"{A}","final_sha":"{B}","branch":"br","files_changed":[],"checks":[],"ready_for_a4":false}}"#
    )
}
fn a4_full(limitation: &str) -> String {
    format!(
        r#"{{"review_id":"rv","task_id":"t","reviewed_sha":"{B}","reviewer":{{"provider_id":"p","model_id":"m","runtime_id":"r","session_ref":""}},"reviewed_at":"2026-11-08T00:00:00Z","independence_attested":true,"verdict":"REJECT","blocking_findings":[{f1}],"nonblocking_findings":[{f2}],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"NOT_SATISFIED","note":"n"}}],"reproduction":{{"performed":true,"checks":[{c}]{limitation}}},"unauthorized_file_changes":[],"recommended_action":"REPAIR_REQUIRED","rationale":"r"}}"#,
        f1 = finding("f1", true),
        f2 = finding("f2", false),
        c = check("REVIEW_EXECUTION", B)
    )
}
fn a4_min() -> String {
    format!(
        r#"{{"review_id":"rv","task_id":"t","reviewed_sha":"{B}","independence_attested":true,"verdict":"PASS","blocking_findings":[],"nonblocking_findings":[],"dimensions":[{{"dimension":"SCOPE_COMPLIANCE","assessment":"SATISFIED"}}]}}"#
    )
}
fn ireq_full() -> String {
    format!(
        r#"{{"request_id":"ir","gate_level":"A2_ACCEPTANCE","source_branch":"b","source_sha":"{B}","target_ref":"main","tasks_included":[{{"task_id":"t","implementation_sha":"{B}","review_sha":"{B}","a4_verdict":"PASS_WITH_NONBLOCKING_FINDINGS","findings_disposition":"logged","merge_commit":null}}],"assurance_profile":"HIGH_ASSURANCE","post_merge_checks":[{pm}],"open_nonblocking_findings":[{f}],"known_limitations":[],"attestations":{{"all_tasks_accepted":true,"review_sha_equals_implementation_sha":true,"no_commit_after_review":false,"no_unresolved_blocking_finding":true,"no_frozen_artifact_modified":true,"no_out_of_scope_write":true}}}}"#,
        pm = check("BROKER_EXECUTION", A).replace(
            r#""exit_code":0"#,
            r#""exit_code":-99999999999999999999999999"#
        ),
        f = finding("o1", false).replace(r#""line":3"#, r#""line":99999999999999999999999"#)
    )
}
fn ireq_min() -> String {
    format!(
        r#"{{"request_id":"ir","gate_level":"A1_INTEGRATION","source_branch":"","source_sha":"{B}","target_ref":"","tasks_included":[{{"task_id":"t","implementation_sha":"{B}","review_sha":"{B}","a4_verdict":"PASS"}}]}}"#
    )
}
fn idec_full() -> String {
    format!(
        r#"{{"decision_id":"d","request_id":"ir","outcome":"ACCEPT","checks_evaluated":[{{"check":"c","result":"NOT_APPLICABLE","detail":""}}],"provenance_chain":[{{"task_id":"t","start_sha":"{A}","implementation_sha":"{B}","review_sha":"{B}","acceptance_sha":null,"integration_sha":"anything"}}],"unmet_conditions":[],"integration_sha":null,"decided_at":"2026-11-08T00:00:00Z"}}"#
    )
}
fn idec_min() -> String {
    r#"{"decision_id":"d","request_id":"ir","outcome":"HUMAN_REQUIRED","checks_evaluated":[{"check":"c","result":"FAIL"}],"decided_at":"2026-11-08T00:00:00Z"}"#.into()
}
fn safety_full() -> String {
    format!(
        r#"{{"interruption_id":"i","task_id":"t","attempt_id":"a","provider_id":"anything-new","model_id":"m","state":"POLICY_BLOCKED","detection_confidence":"EXPLICIT_PROVIDER_SIGNAL","task_classified_defensive":false,"capsule_narrowed":false,"retry_attempted":false,"retry_provider_id":"q","deterministic_tooling_used":false,"terminal_outcome":"HUMAN_REQUIRED","observed_at":"2026-11-08T00:00:00Z","preserved_evidence_refs":[{}]}}"#,
        r#ref()
    )
}
fn safety_min() -> String {
    r#"{"interruption_id":"i","task_id":"t","attempt_id":"a","provider_id":"p","state":"UNKNOWN","observed_at":"2026-11-08T00:00:00Z"}"#.into()
}

/// Exact canonical re-encoding: fixtures are already in schema order with
/// canonical integers, so encode(decode(x)) == x byte-for-byte.
macro_rules! roundtrip {
    ($name:ident, $decode:ident, $encode:ident, [$($fixture:expr),+]) => {
        #[test]
        fn $name() {
            for fixture in [$($fixture),+] {
                let decoded = $decode(&fixture).unwrap_or_else(|e| panic!("{e}: {fixture}"));
                let encoded = $encode(&decoded);
                assert_eq!(encoded, fixture);
                assert_eq!($encode(&$decode(&encoded).unwrap()), encoded);
            }
        }
    };
}

roundtrip!(
    review_request_valid_fixture_roundtrips,
    decode_review_request,
    encode_review_request,
    [review_request_full(), review_request_min()]
);
roundtrip!(
    review_capsule_valid_fixture_roundtrips,
    decode_review_capsule,
    encode_review_capsule,
    [capsule_full(), capsule_min()]
);
roundtrip!(
    a3_handoff_valid_fixture_roundtrips,
    decode_a3_handoff,
    encode_a3_handoff,
    [
        handoff_full(r#","blocker":null"#),
        handoff_full(r#","blocker":"waiting""#),
        handoff_full(""),
        handoff_min()
    ]
);
roundtrip!(
    a4_review_valid_fixture_roundtrips,
    decode_a4_review,
    encode_a4_review,
    [
        a4_full(r#","limitation":null"#),
        a4_full(r#","limitation":"x""#),
        a4_full(""),
        a4_min()
    ]
);
roundtrip!(
    integration_request_valid_fixture_roundtrips,
    decode_integration_request,
    encode_integration_request,
    [ireq_full(), ireq_min()]
);
roundtrip!(
    integration_decision_valid_fixture_roundtrips,
    decode_integration_decision,
    encode_integration_decision,
    [idec_full(), idec_min()]
);
roundtrip!(
    safety_interruption_valid_fixture_roundtrips,
    decode_safety_interruption,
    encode_safety_interruption,
    [safety_full(), safety_min()]
);

#[test]
fn state_shape_validator_admits_every_encoded_state_kind() {
    let cases = [
        (
            DecisionRecordKind::ReviewRequest,
            "rq",
            encode_review_request(&decode_review_request(&review_request_full()).unwrap()),
        ),
        (
            DecisionRecordKind::ReviewRequest,
            "rq",
            encode_review_request(&decode_review_request(&review_request_min()).unwrap()),
        ),
        (
            DecisionRecordKind::A4Review,
            "rv",
            encode_a4_review(&decode_a4_review(&a4_full(r#","limitation":null"#)).unwrap()),
        ),
        (
            DecisionRecordKind::A4Review,
            "rv",
            encode_a4_review(&decode_a4_review(&a4_min()).unwrap()),
        ),
        (
            DecisionRecordKind::IntegrationRequest,
            "ir",
            encode_integration_request(&decode_integration_request(&ireq_full()).unwrap()),
        ),
        (
            DecisionRecordKind::IntegrationRequest,
            "ir",
            encode_integration_request(&decode_integration_request(&ireq_min()).unwrap()),
        ),
        (
            DecisionRecordKind::IntegrationDecision,
            "d",
            encode_integration_decision(&decode_integration_decision(&idec_full()).unwrap()),
        ),
        (
            DecisionRecordKind::IntegrationDecision,
            "d",
            encode_integration_decision(&decode_integration_decision(&idec_min()).unwrap()),
        ),
    ];
    for (kind, id, json) in cases {
        DecisionRecordV1::parse("p", kind, id, &json)
            .unwrap_or_else(|e| panic!("{kind:?}: {e} {json}"));
    }
}

fn kind_of<T>(r: Result<T, ReviewWireError>) -> (String, K) {
    match r {
        Ok(_) => panic!("expected refusal"),
        Err(e) => (e.path, e.kind),
    }
}

/// Mutates one fixture and expects a refusal of `kind` at `path`.
fn refuse<T>(
    decode: fn(&str) -> Result<T, ReviewWireError>,
    fixture: &str,
    from: &str,
    to: &str,
    path: &str,
    kind: K,
) {
    assert!(fixture.contains(from), "mutation {from} not in fixture");
    let mutated = fixture.replacen(from, to, 1);
    assert_eq!(
        kind_of(decode(&mutated)),
        (path.to_owned(), kind),
        "{mutated}"
    );
}

#[test]
fn review_request_negative_fixture_rejected() {
    let f = review_request_full();
    let d = decode_review_request;
    refuse(
        d,
        &f,
        r#""request_id":"rq","#,
        r#""request_id":"rq","extra":1,"#,
        "/extra",
        K::UnknownField,
    );
    refuse(
        d,
        &f,
        r#""task_id":"t","#,
        r#""task_id":"t","task_id":"t","#,
        "/",
        K::DuplicateKey,
    );
    refuse(
        d,
        &f,
        r#""objective":"o","#,
        "",
        "/objective",
        K::MissingField,
    );
    refuse(d, &f, B, "B", "/implementation_sha", K::InvalidValue);
    refuse(
        d,
        &f,
        r#""acceptance_criteria":[{"#,
        r#""acceptance_criteria":[],"x":[{"#,
        "/acceptance_criteria",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""allowed_write_paths":["src/**"]"#,
        r#""allowed_write_paths":[]"#,
        "/allowed_write_paths",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""review_scope":"FULL""#,
        r#""review_scope":"full""#,
        "/review_scope",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""distinct_provider":"REQUIRED""#,
        r#""distinct_provider":null"#,
        "/review_policy/distinct_provider",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""attempt_number":2"#,
        r#""attempt_number":0"#,
        "/attempt_number",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""attempt_number":2"#,
        r#""attempt_number":2.5"#,
        "/attempt_number",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""context_epoch":123456789012345678901234567890"#,
        r#""context_epoch":-1"#,
        "/context_epoch",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""request_id":"rq""#,
        &format!(r#""request_id":"{}""#, "x".repeat(201)),
        "/request_id",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""prior_review_ids":["p"]"#,
        r#""prior_review_ids":[""]"#,
        "/prior_review_ids/0",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""check_command":["true"]"#,
        r#""check_command":"true""#,
        "/acceptance_criteria/0/check_command",
        K::WrongType,
    );
}

#[test]
fn review_capsule_negative_fixture_rejected() {
    let f = capsule_full();
    let d = decode_review_capsule;
    refuse(
        d,
        &f,
        r#""reproduction_required":true"#,
        r#""reproduction_required":"true""#,
        "/reproduction_required",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""blocking_categories":["CORRECTNESS"]"#,
        r#""blocking_categories":[]"#,
        "/severity_policy/blocking_categories",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""command":["cargo","test"]"#,
        r#""command":[]"#,
        "/checks/0/command",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""exit_code":0"#,
        r#""exit_code":99999999999999999999"#,
        "/checks/0/exit_code",
        K::UnsupportedValue,
    );
    refuse(
        d,
        &f,
        r#""result":"PASS""#,
        r#""result":"OK""#,
        "/checks/0/result",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""ref_type":"REPO_PATH","target":"src/x.rs""#,
        r#""ref_type":"FILE","target":"src/x.rs""#,
        "/contract_refs/0/ref_type",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""diff":{"ref_type":"REPO_PATH","target":"src/x.rs""#,
        r#""diff":{"ref_type":"REPO_PATH","target":"""#,
        "/diff/target",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""started_at":"2026-11-08T00:00:00Z""#,
        r#""started_at":"yesterday""#,
        "/checks/0/started_at",
        K::UnsupportedValue,
    );
    refuse(
        d,
        &f,
        r#""context_epoch":7"#,
        r#""context_epoch":7}"#,
        "/",
        K::Syntax,
    );
}

#[test]
fn a3_handoff_negative_fixture_rejected() {
    let f = handoff_full(r#","blocker":null"#);
    let d = decode_a3_handoff;
    refuse(
        d,
        &f,
        r#""ready_for_a4":true"#,
        r#""ready_for_a4":null"#,
        "/ready_for_a4",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""blocker":null"#,
        r#""blocker":1"#,
        "/blocker",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""binding_id":"b""#,
        r#""binding_id":"""#,
        "/implementer/binding_id",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""label":"TESTED""#,
        r#""label":"PROVEN""#,
        "/evidence_labels/0/label",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""files_changed":["src/x.rs"],"#,
        "",
        "/files_changed",
        K::MissingField,
    );
    refuse(
        d,
        &f,
        r#""source":"WORKER_EXECUTION""#,
        r#""source":"WORKER""#,
        "/checks/0/source",
        K::InvalidValue,
    );
}

#[test]
fn a4_review_negative_fixture_rejected() {
    let f = a4_full(r#","limitation":null"#);
    let d = decode_a4_review;
    refuse(
        d,
        &f,
        r#""verdict":"REJECT""#,
        r#""verdict":"FAIL""#,
        "/verdict",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""dimensions":[{"#,
        r#""dimensions":[],"x":[{"#,
        "/dimensions",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""line":3"#,
        r#""line":0"#,
        "/blocking_findings/0/line",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""line":3"#,
        r#""line":99999999999999999999999"#,
        "/blocking_findings/0/line",
        K::UnsupportedValue,
    );
    refuse(
        d,
        &f,
        r#""category":"CORRECTNESS""#,
        r#""category":"BUG""#,
        "/blocking_findings/0/category",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""independence_attested":true,"#,
        "",
        "/independence_attested",
        K::MissingField,
    );
    refuse(
        d,
        &f,
        r#""provider_id":"p""#,
        r#""provider_id":"""#,
        "/reviewer/provider_id",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""performed":true"#,
        r#""performed":true,"performed":true"#,
        "/",
        K::DuplicateKey,
    );
}

#[test]
fn integration_request_negative_fixture_rejected() {
    let f = ireq_full();
    let d = decode_integration_request;
    refuse(
        d,
        &f,
        r#""a4_verdict":"PASS_WITH_NONBLOCKING_FINDINGS""#,
        r#""a4_verdict":"REJECT""#,
        "/tasks_included/0/a4_verdict",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""merge_commit":null"#,
        r#""merge_commit":5"#,
        "/tasks_included/0/merge_commit",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        r#""gate_level":"A2_ACCEPTANCE""#,
        r#""gate_level":"A3""#,
        "/gate_level",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""no_commit_after_review":false"#,
        r#""no_commit_after_review":false,"forged":true"#,
        "/attestations/forged",
        K::UnknownField,
    );
}

#[test]
fn integration_decision_negative_fixture_rejected() {
    let f = idec_full();
    let d = decode_integration_decision;
    refuse(
        d,
        &f,
        r#""checks_evaluated":[{"#,
        r#""checks_evaluated":[],"x":[{"#,
        "/checks_evaluated",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""outcome":"ACCEPT""#,
        r#""outcome":"PASS""#,
        "/outcome",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""decided_at":"2026-11-08T00:00:00Z""#,
        r#""decided_at":null"#,
        "/decided_at",
        K::WrongType,
    );
    refuse(
        d,
        &f,
        &format!(r#""start_sha":"{A}""#),
        r#""start_sha":null"#,
        "/provenance_chain/0/start_sha",
        K::WrongType,
    );
}

#[test]
fn safety_interruption_negative_fixture_rejected() {
    let f = safety_full();
    let d = decode_safety_interruption;
    refuse(
        d,
        &f,
        r#""state":"POLICY_BLOCKED""#,
        r#""state":"BLOCKED""#,
        "/state",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""provider_id":"anything-new""#,
        r#""provider_id":"""#,
        "/provider_id",
        K::InvalidValue,
    );
    refuse(
        d,
        &f,
        r#""observed_at":"2026-11-08T00:00:00Z","#,
        "",
        "/observed_at",
        K::MissingField,
    );
    refuse(
        d,
        &f,
        r#""capsule_narrowed":false"#,
        r#""capsule_narrowed":null"#,
        "/capsule_narrowed",
        K::WrongType,
    );
}

#[test]
fn numeric_domain_preserves_exact_integers_and_refuses_fractions() {
    // 1.0 / 1e3 / -0 are JSON-Schema integers; they canonicalise.
    for (token, canonical) in [
        ("7.0", "7"),
        ("7e0", "7"),
        ("70e-1", "7"),
        ("0.7e1", "7"),
        ("-0", "0"),
    ] {
        let f = capsule_min().replace(
            r#""context_epoch":0"#,
            &format!(r#""context_epoch":{token}"#),
        );
        let c = decode_review_capsule(&f).unwrap();
        assert_eq!(c.context_epoch().decimal_digits(), canonical, "{token}");
    }
    for token in ["7.5", "1e-1", "0.1e0"] {
        let f = capsule_min().replace(
            r#""context_epoch":0"#,
            &format!(r#""context_epoch":{token}"#),
        );
        assert_eq!(
            kind_of(decode_review_capsule(&f)).1,
            K::WrongType,
            "{token}"
        );
    }
    let huge = "1".repeat(300);
    let f = capsule_min().replace(
        r#""context_epoch":0"#,
        &format!(r#""context_epoch":{huge}"#),
    );
    assert_eq!(
        decode_review_capsule(&f)
            .unwrap()
            .context_epoch()
            .decimal_digits(),
        huge
    );
    let f = capsule_min().replace(r#""context_epoch":0"#, r#""context_epoch":1e999999999"#);
    assert_eq!(kind_of(decode_review_capsule(&f)).1, K::UnsupportedValue);
    let f = capsule_min().replace(r#""context_epoch":0"#, r#""context_epoch":0e999999999"#);
    assert_eq!(
        decode_review_capsule(&f)
            .unwrap()
            .context_epoch()
            .decimal_digits(),
        "0"
    );
}

#[test]
fn syntax_controls_refuse_lone_surrogates_trailing_data_and_depth() {
    let bad_surrogate =
        review_request_min().replace(r#""objective":"o""#, r#""objective":"\ud800""#);
    assert_eq!(kind_of(decode_review_request(&bad_surrogate)).1, K::Syntax);
    let pair = review_request_min().replace(r#""objective":"o""#, r#""objective":"😀""#);
    assert_eq!(
        decode_review_request(&pair).unwrap().core().objective(),
        "\u{1F600}"
    );
    assert_eq!(
        kind_of(decode_review_request(&format!(
            "{} x",
            review_request_min()
        )))
        .1,
        K::Syntax
    );
    let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
    assert_eq!(kind_of(decode_review_request(&deep)).1, K::Syntax);
    assert_eq!(
        kind_of(decode_review_request("[]")),
        ("/".into(), K::WrongType)
    );
}

#[test]
fn absent_null_and_empty_stay_distinct() {
    use crate::{
        A3HandoffBlocker, A4ReviewReproductionLimitation, IntegrationDecisionNullableString,
    };
    assert_eq!(
        decode_a3_handoff(&handoff_full("")).unwrap().blocker(),
        None
    );
    assert_eq!(
        decode_a3_handoff(&handoff_full(r#","blocker":null"#))
            .unwrap()
            .blocker(),
        Some(&A3HandoffBlocker::ExplicitNull)
    );
    let r = decode_a4_review(&a4_full("")).unwrap();
    assert_eq!(
        r.core().reproduction().unwrap().limitation(),
        &A4ReviewReproductionLimitation::Omitted
    );
    let r = decode_a4_review(&a4_full(r#","limitation":null"#)).unwrap();
    assert_eq!(
        r.core().reproduction().unwrap().limitation(),
        &A4ReviewReproductionLimitation::Null
    );
    let d = decode_integration_decision(&idec_full()).unwrap();
    assert_eq!(
        d.core().integration_sha(),
        &IntegrationDecisionNullableString::Null
    );
    assert_eq!(
        decode_integration_decision(&idec_min())
            .unwrap()
            .core()
            .integration_sha(),
        &IntegrationDecisionNullableString::Omitted
    );
    let full = decode_review_request(&review_request_full()).unwrap();
    assert_eq!(full.core().non_goals(), Some(&[][..]));
    assert_eq!(
        decode_review_request(&review_request_min())
            .unwrap()
            .core()
            .non_goals(),
        None
    );
}

// ---- F8: exponent extremes return exact values or Unsupported, never panic.

#[test]
fn exponent_extremes_never_panic_and_stay_exact() {
    use super::json::{NotInteger, integer_digits};
    let min = i64::MIN.to_string();
    let max = i64::MAX.to_string();
    // Zero is zero under any exponent, including ones that overflow i64.
    for token in [
        format!("0e-{}", &min[1..]),
        format!("-0e-{}", &min[1..]),
        format!("0.0e-{}", &min[1..]),
        format!("0e{max}"),
        format!("0.00e{max}"),
        "0e99999999999999999999999".to_owned(),
        "0e-99999999999999999999999".to_owned(),
        "0.0e-99999999999999999999999".to_owned(),
    ] {
        assert_eq!(integer_digits(&token), Ok("0".to_owned()), "{token}");
    }
    // A nonzero value under a huge negative exponent is a fraction.
    for token in [
        format!("1e-{}", &min[1..]),
        format!("10e-{}", &min[1..]),
        "1e-99999999999999999999999".to_owned(),
        format!("1.5e-{}", &min[1..]),
    ] {
        assert_eq!(integer_digits(&token), Err(NotInteger::Fraction), "{token}");
    }
    // A nonzero value under a huge positive exponent is unsupported.
    for token in [
        format!("1e{max}"),
        "1e99999999999999999999999".to_owned(),
        format!("7.5e{max}"),
    ] {
        assert_eq!(
            integer_digits(&token),
            Err(NotInteger::Oversized),
            "{token}"
        );
    }
    // Boundaries just inside the representable range stay exact.
    assert_eq!(integer_digits("1e3"), Ok("1000".to_owned()));
    assert_eq!(integer_digits("1000e-3"), Ok("1".to_owned()));
    // The wire error classes: zero decodes, fraction is WrongType, huge is Unsupported.
    let wire = |token: &str| {
        decode_review_capsule(&capsule_min().replace(
            r#""context_epoch":0"#,
            &format!(r#""context_epoch":{token}"#),
        ))
    };
    assert_eq!(
        wire("0e-9223372036854775808")
            .unwrap()
            .context_epoch()
            .decimal_digits(),
        "0"
    );
    assert_eq!(kind_of(wire("1e-9223372036854775808")).1, K::WrongType);
    assert_eq!(
        kind_of(wire("1e9223372036854775807")).1,
        K::UnsupportedValue
    );
}

// ---- F9: \u requires exactly four hexadecimal digits, in every codec.

#[test]
fn unicode_escapes_require_four_hex_digits_in_every_codec() {
    let poisoned = |fixture: String, escape: &str| {
        let mut f = fixture;
        assert_eq!(f.pop(), Some('}'));
        format!(r#"{f},"zz":"{escape}"}}"#)
    };
    let fixtures: [(&str, String, fn(&str) -> bool); 7] = [
        ("review_request", review_request_min(), |t| {
            decode_review_request(t).is_ok()
        }),
        ("review_capsule", capsule_min(), |t| {
            decode_review_capsule(t).is_ok()
        }),
        ("a3_handoff", handoff_min(), |t| {
            decode_a3_handoff(t).is_ok()
        }),
        ("a4_review", a4_min(), |t| decode_a4_review(t).is_ok()),
        ("integration_request", ireq_min(), |t| {
            decode_integration_request(t).is_ok()
        }),
        ("integration_decision", idec_min(), |t| {
            decode_integration_decision(t).is_ok()
        }),
        ("safety_interruption", safety_min(), |t| {
            decode_safety_interruption(t).is_ok()
        }),
    ];
    let kinds: [fn(&str) -> Option<K>; 7] = [
        |t| decode_review_request(t).err().map(|e| e.kind),
        |t| decode_review_capsule(t).err().map(|e| e.kind),
        |t| decode_a3_handoff(t).err().map(|e| e.kind),
        |t| decode_a4_review(t).err().map(|e| e.kind),
        |t| decode_integration_request(t).err().map(|e| e.kind),
        |t| decode_integration_decision(t).err().map(|e| e.kind),
        |t| decode_safety_interruption(t).err().map(|e| e.kind),
    ];
    for ((name, fixture, _), kind_of_text) in fixtures.iter().zip(kinds) {
        for bad in [
            r"\u+041",
            r"\u-041",
            r"\u 041",
            r"\u0x41",
            r"\u004",
            r"\u00g1",
            r"\u+0+1",
            r"\ud83d\u+e00",
            r"\ud83d\ude0",
        ] {
            let text = poisoned(fixture.clone(), bad);
            assert_eq!(
                kind_of_text(&text),
                Some(K::Syntax),
                "{name}: {bad} must be a syntax error"
            );
        }
        // Valid escapes still pass syntax (the extra member is then refused as unknown).
        for good in [r"A", r"é", r"😀", r"ꯍ"] {
            let text = poisoned(fixture.clone(), good);
            assert_ne!(kind_of_text(&text), Some(K::Syntax), "{name}: {good}");
        }
    }
    // Keys use the same string reader.
    let key = review_request_min().replacen(r#""task_id""#, r#""task\u+069d""#, 1);
    assert_eq!(kind_of(decode_review_request(&key)).1, K::Syntax);
    // The exact original control: "\u+041" must never decode as "A".
    let a = review_request_min().replace(r#""objective":"o""#, r#""objective":"\u+041""#);
    assert!(decode_review_request(&a).is_err());
}
