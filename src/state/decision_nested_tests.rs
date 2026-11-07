//! Nested schema admission for the eight persisted decision families.
//!
//! Every negative case is also refused by the pinned `jsonschema` oracle (a
//! test-only dependency), so the table cannot drift from the owner schemas.
//! Exact-number and open-field cases bypass the oracle, whose number model is
//! `f64`-limited.

use serde_json::{Value, json};

use crate::decision_records::{DecisionRecordKind as K, DecisionRecordV1};
use crate::decision_records_tests::oracle;
use crate::error::StateError;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn eref() -> Value {
    json!({"ref_type":"REPO_PATH","target":"src/x.rs","digest":"sha256:1","section":"s"})
}
fn finding() -> Value {
    json!({"finding_id":"f-1","severity":"HIGH","category":"CORRECTNESS","description":"d",
        "blocking":true,"path":"src/x.rs","line":7,"evidence_ref":eref(),"confidence":"MEDIUM",
        "source":"TEST"})
}
fn exec_check() -> Value {
    json!({"source":"REVIEW_EXECUTION","command":["cargo","test"],"exit_code":0,"code_sha":A,
        "timed_out":false,"started_at":"s","finished_at":"f","output_ref":eref(),"result":"PASS"})
}

/// (kind, storage id, fully populated valid document)
fn full(kind: K) -> (&'static str, Value) {
    match kind {
        K::RoutingDecision => (
            "rd-1",
            json!({
            "decision_id":"rd-1","request_id":"rq-1","task_id":"t-1","occurred_at":"x",
            "outcome":"SELECTED","selected_provider":"p","selected_model":"m","selected_runtime":"r",
            "selection_reason":{"quality_floor_applied":"BALANCED","hard_filters_passed":["a","b"],
                "score_components":{"expected_implementation_cost":1.5,"expected_rejection_probability":0.25,
                    "expected_repair_cost":null,"expected_review_cost":2,"latency_estimate_seconds":3.5},
                "decisive_factor":"cost"},
            "capability_evidence":[{"capability":"coding","value":{"n":[1,2.5,null]},"confidence":"LOCAL_EMPIRICAL",
                "source_ref":eref(),"observed_at":"o","sample_size":4}],
            "confidence":"HIGH","availability_at_decision":"AVAILABLE","estimated_cost_class":"LOW",
            "alternative_candidates":[{"provider_id":"p2","model_id":"m2","runtime_id":"r2","score":0.5,
                "rejection_reason":"slower"}],
            "registry_freshness":{"model_list_age_seconds":-3,"capability_age_seconds":null,
                "calibration_sample_size":10,"stale":false},
            "mode":"AUTO_CURRENT","user_involved":false,"user_pin_applied":false,"fallback_from":"rd-0"}),
        ),
        K::RegistryCapability => (
            "cap-1",
            json!({
            "subject_kind":"MODEL_RUNTIME_PAIR","provider_id":"p","model_id":"m","runtime_id":"r",
            "capability":"code.edit","value":{"x":[1,2.5]},"confidence":"OFFICIAL_VERIFIED",
            "source_ref":eref(),"observed_at":"o","sample_size":3,"lifecycle_state":"CALIBRATING"}),
        ),
        K::ModelObservation => (
            "obs-1",
            json!({
            "observation_id":"obs-1","provider_id":"p","model_id":"m","runtime_id":"r","task_id":"t-1",
            "task_class":"impl","outcome":"ACCEPTED_AFTER_REPAIR","repair_count":2,"cost":0.5,"tokens":9,
            "latency_seconds":1.25,"cost_to_accepted_result":null,"difficulty_hint":"HIGH",
            "excluded_from_calibration":false,"observed_at":"o"}),
        ),
        K::AvailabilityObservation => (
            "av-1",
            json!({
            "provider_id":"p","model_id":"m","runtime_id":"r","state":"DEGRADED","retry_after_seconds":null,
            "signal_source":"PROBE","observed_at":"o","note":"n"}),
        ),
        K::ReviewRequest => (
            "rr-1",
            json!({
            "request_id":"rr-1","task_id":"t-1","attempt_id":"att-1","workstream_id":"ws-1",
            "baseline_sha":B,"implementation_sha":A,"branch":"b","objective":"o",
            "acceptance_criteria":[{"id":"c1","description":"d","kind":"SEMANTIC","check_command":["x"],
                "rationale":"r"}],
            "non_goals":["n"],"allowed_write_paths":["src/**"],"architecture_refs":[eref()],
            "contract_refs":[eref()],"a3_handoff_ref":eref(),"assurance_profile":"HIGH_ASSURANCE",
            "review_scope":"REPAIR_VERIFICATION",
            "review_policy":{"required":true,"distinct_provider":"PREFERRED","reviewer_floor":"FRONTIER",
                "excluded_session_refs":["s1"]},
            "prior_review_ids":["rev-0"],"attempt_number":2,"requested_at":"q","context_epoch":0}),
        ),
        K::A4Review => (
            "rev-1",
            json!({
            "review_id":"rev-1","task_id":"t-1","reviewed_sha":A,
            "reviewer":{"provider_id":"p","model_id":"m","runtime_id":"r","session_ref":"s"},
            "reviewed_at":"x","independence_attested":true,"verdict":"PASS_WITH_NONBLOCKING_FINDINGS",
            "blocking_findings":[finding()],"nonblocking_findings":[finding()],
            "dimensions":[{"dimension":"CORRECTNESS","assessment":"SATISFIED","note":"n"}],
            "reproduction":{"performed":true,"checks":[exec_check()],"limitation":null},
            "unauthorized_file_changes":["x"],"recommended_action":"ACCEPT_WITH_FINDINGS_LOGGED",
            "rationale":"r"}),
        ),
        K::IntegrationRequest => (
            "ir-1",
            json!({
            "request_id":"ir-1","gate_level":"A1_INTEGRATION","source_branch":"b","source_sha":A,
            "target_ref":"main",
            "tasks_included":[{"task_id":"t-1","implementation_sha":A,"review_sha":A,"a4_verdict":"PASS",
                "findings_disposition":"d","merge_commit":null}],
            "assurance_profile":"STANDARD","post_merge_checks":[exec_check()],
            "open_nonblocking_findings":[finding()],"known_limitations":["l"],
            "attestations":{"all_tasks_accepted":true,"review_sha_equals_implementation_sha":true,
                "no_commit_after_review":true,"no_unresolved_blocking_finding":true,
                "no_frozen_artifact_modified":true,"no_out_of_scope_write":true}}),
        ),
        K::IntegrationDecision => (
            "id-1",
            json!({
            "decision_id":"id-1","request_id":"ir-1","outcome":"ACCEPT",
            "checks_evaluated":[{"check":"ci","result":"PASS","detail":"d"}],
            "provenance_chain":[{"task_id":"t-1","start_sha":B,"implementation_sha":A,"review_sha":A,
                "acceptance_sha":null,"integration_sha":"anything"}],
            "unmet_conditions":["u"],"integration_sha":null,"decided_at":"d"}),
        ),
    }
}

fn parse(kind: K, value: &Value) -> Result<DecisionRecordV1, StateError> {
    let (id, _) = full(kind);
    DecisionRecordV1::parse("p", kind, id, &value.to_string())
}

#[test]
fn fully_populated_documents_are_admitted_and_match_the_oracle() {
    for kind in K::ALL {
        let (id, doc) = full(kind);
        let text = doc.to_string();
        assert!(oracle(kind, &text), "{kind:?} fixture must be schema-valid");
        let parsed = DecisionRecordV1::parse("p", kind, id, &text).unwrap();
        assert_eq!(parsed.record_json(), text, "exact bytes kept");
    }
}

enum Op {
    Set(Value),
    Remove,
}
use Op::{Remove, Set};

#[test]
fn nested_violations_are_refused_and_agree_with_the_oracle() {
    let bad = || json!("BAD");
    let cases: Vec<(K, &str, Op)> = vec![
        // RoutingDecision.selection_reason
        (K::RoutingDecision, "/selection_reason/extra", Set(json!(1))),
        (
            K::RoutingDecision,
            "/selection_reason/quality_floor_applied",
            Set(bad()),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/hard_filters_passed",
            Set(json!("a")),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/hard_filters_passed/0",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/decisive_factor",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/score_components/extra",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/score_components/expected_review_cost",
            Set(json!("2")),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/score_components/expected_rejection_probability",
            Set(json!(1.5)),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/score_components/expected_rejection_probability",
            Set(json!(-0.1)),
        ),
        (
            K::RoutingDecision,
            "/selection_reason/score_components/latency_estimate_seconds",
            Set(json!(true)),
        ),
        (K::RoutingDecision, "/selection_reason", Set(json!([]))),
        // capability_evidence
        (
            K::RoutingDecision,
            "/capability_evidence/0/capability",
            Remove,
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/confidence",
            Remove,
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/confidence",
            Set(bad()),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/extra",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/source_ref/target",
            Remove,
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/source_ref/target",
            Set(json!("")),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/source_ref/ref_type",
            Set(bad()),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/source_ref/extra",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/sample_size",
            Set(json!(-1)),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0/sample_size",
            Set(json!(1.5)),
        ),
        (
            K::RoutingDecision,
            "/capability_evidence/0",
            Set(json!("x")),
        ),
        (K::RoutingDecision, "/capability_evidence", Set(json!({}))),
        // alternatives and freshness
        (
            K::RoutingDecision,
            "/alternative_candidates/0/rejection_reason",
            Remove,
        ),
        (
            K::RoutingDecision,
            "/alternative_candidates/0/provider_id",
            Set(json!("")),
        ),
        (
            K::RoutingDecision,
            "/alternative_candidates/0/extra",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/alternative_candidates/0/score",
            Set(json!("1")),
        ),
        (K::RoutingDecision, "/registry_freshness/stale", Remove),
        (
            K::RoutingDecision,
            "/registry_freshness/stale",
            Set(json!("no")),
        ),
        (
            K::RoutingDecision,
            "/registry_freshness/extra",
            Set(json!(1)),
        ),
        (
            K::RoutingDecision,
            "/registry_freshness/model_list_age_seconds",
            Set(json!(1.5)),
        ),
        (
            K::RoutingDecision,
            "/registry_freshness/calibration_sample_size",
            Set(json!("1")),
        ),
        (K::RoutingDecision, "/confidence", Set(bad())),
        // ModelCapability.source_ref (the value stays open)
        (K::RegistryCapability, "/source_ref/ref_type", Set(bad())),
        (K::RegistryCapability, "/source_ref/target", Remove),
        (K::RegistryCapability, "/source_ref/digest", Set(json!(1))),
        (
            K::RegistryCapability,
            "/source_ref/section",
            Set(json!(null)),
        ),
        // ModelObservation / AvailabilityState numbers
        (K::ModelObservation, "/tokens", Set(json!(1.5))),
        (K::ModelObservation, "/cost", Set(json!("1"))),
        (K::ModelObservation, "/latency_seconds", Set(json!([]))),
        (
            K::AvailabilityObservation,
            "/retry_after_seconds",
            Set(json!(-1)),
        ),
        (
            K::AvailabilityObservation,
            "/retry_after_seconds",
            Set(json!(0.5)),
        ),
        // ReviewRequest
        (K::ReviewRequest, "/acceptance_criteria/0/id", Remove),
        (
            K::ReviewRequest,
            "/acceptance_criteria/0/description",
            Set(json!("")),
        ),
        (K::ReviewRequest, "/acceptance_criteria/0/kind", Set(bad())),
        (
            K::ReviewRequest,
            "/acceptance_criteria/0/check_command",
            Set(json!("x")),
        ),
        (
            K::ReviewRequest,
            "/acceptance_criteria/0/check_command/0",
            Set(json!(1)),
        ),
        (
            K::ReviewRequest,
            "/acceptance_criteria/0/rationale",
            Set(json!(1)),
        ),
        (
            K::ReviewRequest,
            "/acceptance_criteria/0/extra",
            Set(json!(1)),
        ),
        (K::ReviewRequest, "/acceptance_criteria/0", Set(json!(1))),
        (K::ReviewRequest, "/non_goals/0", Set(json!(1))),
        (K::ReviewRequest, "/allowed_write_paths", Set(json!([]))),
        (K::ReviewRequest, "/allowed_write_paths/0", Set(json!(1))),
        (K::ReviewRequest, "/architecture_refs/0/target", Remove),
        (K::ReviewRequest, "/contract_refs/0/ref_type", Set(bad())),
        (K::ReviewRequest, "/a3_handoff_ref/target", Set(json!(""))),
        (K::ReviewRequest, "/review_policy/extra", Set(json!(1))),
        (K::ReviewRequest, "/review_policy/required", Set(json!(1))),
        (
            K::ReviewRequest,
            "/review_policy/distinct_provider",
            Set(bad()),
        ),
        (
            K::ReviewRequest,
            "/review_policy/reviewer_floor",
            Set(bad()),
        ),
        (
            K::ReviewRequest,
            "/review_policy/excluded_session_refs/0",
            Set(json!(1)),
        ),
        (K::ReviewRequest, "/prior_review_ids/0", Set(json!(""))),
        (
            K::ReviewRequest,
            "/prior_review_ids/0",
            Set(json!("x".repeat(201))),
        ),
        (K::ReviewRequest, "/prior_review_ids/0", Set(json!(1))),
        (K::ReviewRequest, "/workstream_id", Set(json!(""))),
        // A4Review
        (K::A4Review, "/reviewer/extra", Set(json!(1))),
        (K::A4Review, "/reviewer/provider_id", Set(json!(""))),
        (K::A4Review, "/reviewer/session_ref", Set(json!(1))),
        (K::A4Review, "/blocking_findings/0/finding_id", Remove),
        (K::A4Review, "/blocking_findings/0/severity", Set(bad())),
        (K::A4Review, "/blocking_findings/0/category", Set(bad())),
        (
            K::A4Review,
            "/blocking_findings/0/description",
            Set(json!("")),
        ),
        (
            K::A4Review,
            "/blocking_findings/0/blocking",
            Set(json!("true")),
        ),
        (K::A4Review, "/blocking_findings/0/path", Set(json!(1))),
        (K::A4Review, "/blocking_findings/0/line", Set(json!(0))),
        (K::A4Review, "/blocking_findings/0/line", Set(json!(1.5))),
        (
            K::A4Review,
            "/blocking_findings/0/evidence_ref/target",
            Remove,
        ),
        (K::A4Review, "/blocking_findings/0/confidence", Set(bad())),
        (K::A4Review, "/blocking_findings/0/source", Set(bad())),
        (K::A4Review, "/blocking_findings/0/extra", Set(json!(1))),
        (K::A4Review, "/nonblocking_findings/0/severity", Remove),
        (K::A4Review, "/nonblocking_findings/0", Set(json!([]))),
        (K::A4Review, "/dimensions/0/dimension", Set(bad())),
        (K::A4Review, "/dimensions/0/assessment", Remove),
        (K::A4Review, "/dimensions/0/note", Set(json!(1))),
        (K::A4Review, "/dimensions/0/extra", Set(json!(1))),
        (K::A4Review, "/reproduction/extra", Set(json!(1))),
        (K::A4Review, "/reproduction/performed", Set(json!(1))),
        (K::A4Review, "/reproduction/limitation", Set(json!(1))),
        (K::A4Review, "/reproduction/checks/0/code_sha", Remove),
        (
            K::A4Review,
            "/reproduction/checks/0/code_sha",
            Set(json!(A.to_uppercase())),
        ),
        (
            K::A4Review,
            "/reproduction/checks/0/command",
            Set(json!([])),
        ),
        (
            K::A4Review,
            "/reproduction/checks/0/command/0",
            Set(json!(1)),
        ),
        (
            K::A4Review,
            "/reproduction/checks/0/exit_code",
            Set(json!(1.5)),
        ),
        (K::A4Review, "/reproduction/checks/0/source", Set(bad())),
        (K::A4Review, "/reproduction/checks/0/result", Set(bad())),
        (
            K::A4Review,
            "/reproduction/checks/0/timed_out",
            Set(json!(0)),
        ),
        (
            K::A4Review,
            "/reproduction/checks/0/output_ref/ref_type",
            Set(bad()),
        ),
        (K::A4Review, "/unauthorized_file_changes/0", Set(json!(1))),
        (K::A4Review, "/recommended_action", Set(bad())),
        // IntegrationRequest
        (
            K::IntegrationRequest,
            "/tasks_included/0/review_sha",
            Remove,
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/review_sha",
            Set(json!("abc")),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/implementation_sha",
            Set(json!(A.to_uppercase())),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/a4_verdict",
            Set(json!("REJECT")),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/task_id",
            Set(json!("")),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/merge_commit",
            Set(json!(1)),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/findings_disposition",
            Set(json!(1)),
        ),
        (
            K::IntegrationRequest,
            "/tasks_included/0/extra",
            Set(json!(1)),
        ),
        (
            K::IntegrationRequest,
            "/post_merge_checks/0/exit_code",
            Set(json!("0")),
        ),
        (
            K::IntegrationRequest,
            "/post_merge_checks/0/code_sha",
            Set(json!("short")),
        ),
        (
            K::IntegrationRequest,
            "/open_nonblocking_findings/0/category",
            Set(bad()),
        ),
        (K::IntegrationRequest, "/known_limitations/0", Set(json!(1))),
        (
            K::IntegrationRequest,
            "/attestations/extra",
            Set(json!(true)),
        ),
        (
            K::IntegrationRequest,
            "/attestations/all_tasks_accepted",
            Set(json!("yes")),
        ),
        // IntegrationDecision
        (K::IntegrationDecision, "/checks_evaluated/0/check", Remove),
        (
            K::IntegrationDecision,
            "/checks_evaluated/0/result",
            Set(bad()),
        ),
        (
            K::IntegrationDecision,
            "/checks_evaluated/0/detail",
            Set(json!(1)),
        ),
        (
            K::IntegrationDecision,
            "/checks_evaluated/0/extra",
            Set(json!(1)),
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/task_id",
            Remove,
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/review_sha",
            Remove,
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/start_sha",
            Set(json!("abc")),
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/acceptance_sha",
            Set(json!(1)),
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/integration_sha",
            Set(json!(true)),
        ),
        (
            K::IntegrationDecision,
            "/provenance_chain/0/extra",
            Set(json!(1)),
        ),
        (K::IntegrationDecision, "/provenance_chain/0", Set(json!(1))),
        (K::IntegrationDecision, "/unmet_conditions/0", Set(json!(1))),
        (K::IntegrationDecision, "/integration_sha", Set(json!(1))),
    ];
    assert!(cases.len() > 100);
    for (kind, pointer, op) in cases {
        let (_, mut doc) = full(kind);
        let (parent, leaf) = pointer.rsplit_once('/').unwrap();
        let target = if parent.is_empty() {
            &mut doc
        } else {
            doc.pointer_mut(parent)
                .unwrap_or_else(|| panic!("{kind:?} {pointer}"))
        };
        match (op, target) {
            (Set(v), Value::Object(map)) => {
                map.insert(leaf.to_string(), v);
            }
            (Set(v), Value::Array(items)) => items[leaf.parse::<usize>().unwrap()] = v,
            (Remove, Value::Object(map)) => {
                assert!(map.remove(leaf).is_some(), "{kind:?} {pointer}");
            }
            _ => panic!("bad case {kind:?} {pointer}"),
        }
        let text = doc.to_string();
        assert!(
            !oracle(kind, &text),
            "oracle must refuse {kind:?} {pointer}: {text}"
        );
        assert!(
            parse(kind, &doc).is_err(),
            "State must refuse {kind:?} {pointer}: {text}"
        );
    }
}

#[test]
fn open_fields_absent_null_and_optional_distinctions_are_preserved() {
    // ModelCapability.value and capability_evidence[].value are open.
    for value in [
        r#"null"#,
        r#"true"#,
        r#""s""#,
        r#"[1,{"k":[null,1e400]}]"#,
        r#"{"deep":{"er":[123456789012345678901234567890,-0,0.10,1E5]}}"#,
        r#"[]"#,
    ] {
        let text = format!(
            r#"{{"subject_kind":"MODEL","capability":"c","value":{value},"confidence":"UNVERIFIED","observed_at":"o"}}"#
        );
        let r = DecisionRecordV1::parse("p", K::RegistryCapability, "cap", &text).unwrap();
        assert_eq!(r.record_json(), text);
        let evidence = format!(
            r#"{{"decision_id":"d","request_id":"r","outcome":"BLOCKED","registry_freshness":{{"stale":true}},"mode":"AUTO_CURRENT","capability_evidence":[{{"capability":"c","value":{value},"confidence":"UNVERIFIED"}}]}}"#
        );
        assert!(DecisionRecordV1::parse("p", K::RoutingDecision, "d", &evidence).is_ok());
    }
    // Absent value is distinct from null value, and both are kept verbatim.
    let absent =
        r#"{"subject_kind":"MODEL","capability":"c","confidence":"UNVERIFIED","observed_at":"o"}"#;
    let null = r#"{"subject_kind":"MODEL","capability":"c","value":null,"confidence":"UNVERIFIED","observed_at":"o"}"#;
    let a = DecisionRecordV1::parse("p", K::RegistryCapability, "c1", absent).unwrap();
    let n = DecisionRecordV1::parse("p", K::RegistryCapability, "c1", null).unwrap();
    assert_ne!(a.record_sha256(), n.record_sha256());
    // Optional nested nulls (nullable fields) and omissions both admit.
    let (_, mut doc) = full(K::RoutingDecision);
    doc["selection_reason"]["score_components"] = json!({});
    doc["capability_evidence"][0]
        .as_object_mut()
        .unwrap()
        .remove("sample_size");
    assert!(parse(K::RoutingDecision, &doc).is_ok());
}

#[test]
fn nested_exact_numbers_have_no_invented_ceiling_or_float_conversion() {
    let decision = |probability: &str, age: &str, sample: &str| {
        format!(
            r#"{{"decision_id":"d","request_id":"r","outcome":"SELECTED","selection_reason":{{"score_components":{{"expected_rejection_probability":{probability}}}}},"capability_evidence":[{{"capability":"c","confidence":"UNVERIFIED","sample_size":{sample}}}],"registry_freshness":{{"stale":false,"model_list_age_seconds":{age}}},"mode":"AUTO_CURRENT"}}"#
        )
    };
    let admit = |p: &str, a: &str, s: &str| {
        let text = decision(p, a, s);
        DecisionRecordV1::parse("p", K::RoutingDecision, "d", &text)
            .map(|r| r.record_json().to_string())
    };
    // Schema-admitted integral spellings and huge integers stay raw.
    for (p, a, s) in [
        ("0", "-12345678901234567890123456789", "1e2"),
        ("-0", "1.0", "100.0"),
        ("1", "1e400", "123456789012345678901234567890123456789"),
        ("1.0", "-0", "0"),
        ("1.000000000000000000000000000", "5", "0"),
        ("0.99999999999999999999999999", "5", "0"),
        ("1e-400", "5", "0"),
        ("0.1e1", "5", "0"),
        ("10e-1", "5", "0"),
    ] {
        let kept = admit(p, a, s).unwrap_or_else(|_| panic!("{p} {a} {s}"));
        assert_eq!(kept, decision(p, a, s), "exact bytes");
    }
    // One part in 10^30 above one is above the schema maximum; f64 would
    // round it to 1.0 and wrongly admit it.
    for p in [
        "1.000000000000000000000000000001",
        "1.0000000000000000000000000000000000001e0",
        "1e1",
        "1e400",
        "2",
        "-1e-400",
    ] {
        assert!(admit(p, "5", "0").is_err(), "{p}");
    }
    for (a, s) in [
        ("1.5", "0"),
        ("1e-1", "0"),
        ("5", "-1"),
        ("5", "1.5"),
        ("5", "-1e-400"),
    ] {
        assert!(admit("0", a, s).is_err(), "{a} {s}");
    }
}

#[test]
fn stored_nested_content_is_readmitted_on_read() {
    let tmp = TempDir::new("decision-nested-read");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let (id, doc) = full(K::A4Review);
    let record = DecisionRecordV1::parse("p", K::A4Review, id, &doc.to_string()).unwrap();
    repo.append_decision_record(&record).unwrap();
    assert!(
        repo.find_decision_record("p", K::A4Review, id)
            .unwrap()
            .is_some()
    );
    // Corruption fixture: nested damage with a matching digest still fails
    // closed because the nested shape is re-admitted, not just hashed.
    let mut damaged = doc.clone();
    damaged["blocking_findings"][0]["severity"] = json!("BAD");
    let text = damaged.to_string();
    let digest = {
        use sha2::{Digest, Sha256};
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    repo.connection()
        .execute_batch("DROP TRIGGER decision_records_no_update")
        .unwrap();
    repo.connection()
        .execute(
            "UPDATE decision_records SET record_json=?1, record_sha256=?2",
            rusqlite::params![text, digest],
        )
        .unwrap();
    assert!(matches!(
        repo.find_decision_record("p", K::A4Review, id),
        Err(StateError::DecisionRecord {
            code: crate::error::RecordFailureCode::CorruptStore,
            ..
        })
    ));
}
