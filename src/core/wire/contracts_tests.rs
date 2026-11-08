//! Six remaining OR codecs: fixtures, independent jsonschema agreement on
//! positive and negative samples, verbatim and operational roundtrips,
//! null/absence and numeric-domain controls.

use serde_json::{Value, json};

use super::*;
use crate::capsules::QualityFloor;

fn read(path: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap()
}

fn validator(name: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&read(&format!("schemas/{name}.schema.json"))).unwrap()
}

/// Our decoder accepts exactly what the frozen schema accepts.
fn agree(name: &str, decode: impl Fn(&str) -> bool, samples: &[Value]) {
    let schema = validator(name);
    for sample in samples {
        assert_eq!(
            decode(&sample.to_string()),
            schema.is_valid(sample),
            "{name}: {sample}"
        );
    }
}

fn edit(base: &Value, f: impl FnOnce(&mut Value)) -> Value {
    let mut v = base.clone();
    f(&mut v);
    v
}

fn drop_key(base: &Value, path: &[&str]) -> Value {
    edit(base, |v| {
        let mut cur = v;
        for p in &path[..path.len() - 1] {
            cur = &mut cur[*p];
        }
        cur.as_object_mut().unwrap().remove(path[path.len() - 1]);
    })
}

fn same(text: &str, expected: &Value) {
    assert_eq!(&serde_json::from_str::<Value>(text).unwrap(), expected);
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn task() -> Value {
    json!({
        "task_id": "t", "workstream_id": "ws-a", "attempt_number": 1,
        "task_type": "IMPLEMENTATION", "objective": "Implement the scheduler.",
        "acceptance_criteria": [{"id": "c1", "description": "tests pass", "kind": "DETERMINISTIC",
                                  "check_command": ["cargo", "test"]}],
        "baseline_sha": SHA, "start_sha": SHA,
        "allowed_write_paths": ["src/core/scheduler/**"], "forbidden_write_paths": [],
        "relevant_context_refs": [{"ref_type": "REPO_PATH", "target": "docs/x.md", "section": "s"}],
        "dependencies": ["dep"], "quality_floor": "BALANCED",
        "required_capabilities": ["graph.core"],
        "verification_plan": [{"source": "WORKER_EXECUTION", "command": ["cargo", "test"],
                               "exit_code": 0, "code_sha": SHA}],
        "review_policy": {"required": true, "distinct_provider": "PREFERRED"},
        "assurance_profile": "STANDARD",
        "cost_policy": {"max_cost": 2.5, "priority": "HIGH"},
        "time_budget_seconds": 600, "branch": "runtime-a3/t", "worktree": "/w/t",
        "remote_publish_policy": "LOCAL_ONLY", "stop_conditions": ["two failures"],
        "context_epoch": 7
    })
}

fn decode_task(t: &str) -> bool {
    TaskCapsuleWireV1::from_json(t).is_ok()
}

#[test]
fn task_capsule_codec_matches_schema_and_roundtrips() {
    let base = task();
    let mut samples = vec![base.clone()];
    for key in [
        "task_id",
        "verification_plan",
        "context_epoch",
        "required_capabilities",
    ] {
        samples.push(drop_key(&base, &[key]));
    }
    samples.extend([
        edit(&base, |v| v["extra"] = json!(1)),
        edit(&base, |v| v["attempt_number"] = json!(0)),
        edit(&base, |v| v["attempt_number"] = json!(1.5)),
        edit(&base, |v| v["context_epoch"] = json!(-1)),
        edit(&base, |v| v["branch"] = json!("main")),
        edit(&base, |v| v["start_sha"] = json!("ABC")),
        edit(&base, |v| v["task_type"] = json!("CHORE")),
        edit(&base, |v| v["acceptance_criteria"] = json!([])),
        edit(&base, |v| v["required_capabilities"] = json!([])),
        edit(&base, |v| v["allowed_write_paths"] = json!([])),
        edit(&base, |v| v["parent_task_id"] = json!(null)),
        edit(&base, |v| v["cost_policy"]["max_cost"] = json!(null)),
        edit(&base, |v| v["cost_policy"]["max_cost"] = json!("1")),
        edit(&base, |v| v["review_policy"]["bogus"] = json!(true)),
        edit(&base, |v| v["verification_plan"][0]["command"] = json!([])),
        edit(&base, |v| {
            v["relevant_context_refs"][0]["target"] = json!("")
        }),
        edit(&base, |v| v["worktree"] = json!("")),
        edit(&base, |v| v["task_id"] = json!("x".repeat(201))),
    ]);
    agree("TaskCapsule", decode_task, &samples);

    let wire = TaskCapsuleWireV1::from_json(&base.to_string()).unwrap();
    same(&wire.to_json(), &base);
    let capsule = wire.to_task_capsule().unwrap();
    assert_eq!(capsule.context_epoch(), 7);
    assert_eq!(capsule.verification_plan()[0].expected_exit_code(), 0);
    let encoded = TaskCapsuleWireV1::from_task_capsule(&capsule).unwrap();
    same(&encoded.to_json(), &base);
    assert_eq!(encoded.to_task_capsule().unwrap(), capsule);
}

#[test]
fn task_capsule_operational_domain_is_explicit() {
    let base = task();
    let unsupported = |v: Value| {
        TaskCapsuleWireV1::from_json(&v.to_string())
            .unwrap()
            .to_task_capsule()
    };
    for (sample, field) in [
        (
            edit(&base, |v| {
                v["context_epoch"] = json!(9223372036854775808u64)
            }),
            "context_epoch",
        ),
        (
            edit(&base, |v| v["attempt_number"] = json!(1.0)),
            "attempt_number",
        ),
        (
            edit(&base, |v| {
                v["verification_plan"][0]["exit_code"] = json!(1e0)
            }),
            "exit_code",
        ),
        (
            edit(&base, |v| {
                v["verification_plan"][0]["source"] = json!("REVIEW_EXECUTION")
            }),
            "verification_plan",
        ),
        (
            edit(&base, |v| {
                v["verification_plan"][0]["code_sha"] = json!("f".repeat(40))
            }),
            "verification_plan",
        ),
        (
            edit(&base, |v| {
                v["verification_plan"][0]["result"] = json!("PASS")
            }),
            "verification_plan",
        ),
        (
            edit(&base, |v| v["required_capabilities"] = json!(["Graph"])),
            "required_capabilities",
        ),
    ] {
        assert_eq!(unsupported(sample), Err(GraphWireError::Unsupported(field)));
    }
    // A wire number keeps its exact lexeme even when it is not canonical.
    let lexical = base
        .to_string()
        .replace("\"context_epoch\":7", "\"context_epoch\":7.0");
    assert!(
        TaskCapsuleWireV1::from_json(&lexical)
            .unwrap()
            .to_json()
            .contains("7.0")
    );
    // max_cost null is distinct from absent and survives the operational roundtrip.
    let null = edit(&base, |v| v["cost_policy"]["max_cost"] = json!(null));
    let capsule = TaskCapsuleWireV1::from_json(&null.to_string())
        .unwrap()
        .to_task_capsule()
        .unwrap();
    assert_eq!(capsule.cost_policy().unwrap().max_cost(), Some(None));
    same(
        &TaskCapsuleWireV1::from_task_capsule(&capsule)
            .unwrap()
            .to_json(),
        &null,
    );
    let absent = drop_key(&base, &["cost_policy", "max_cost"]);
    let capsule = TaskCapsuleWireV1::from_json(&absent.to_string())
        .unwrap()
        .to_task_capsule()
        .unwrap();
    assert_eq!(capsule.cost_policy().unwrap().max_cost(), None);
    // A capsule without required capabilities cannot be represented.
    let empty = crate::capsules::TaskCapsule::try_new(
        "t".into(),
        "ws".into(),
        None,
        1,
        crate::capsules::TaskType::Docs,
        "o".into(),
        capsule.acceptance_criteria().to_vec(),
        None,
        SHA.into(),
        SHA.into(),
        vec!["docs/**".into()],
        vec![],
        None,
        None,
        None,
        None,
        QualityFloor::Economy,
        vec![],
        None,
        vec![],
        None,
        crate::capsules::AssuranceProfileSelector::Light,
        None,
        None,
        None,
        "runtime-a3/t".into(),
        "/w".into(),
        None,
        vec!["s".into()],
        None,
        0,
    )
    .unwrap();
    assert_eq!(
        TaskCapsuleWireV1::from_task_capsule(&empty),
        Err(GraphWireError::Unsupported("required_capabilities"))
    );
}

fn repair() -> Value {
    json!({
        "task_id": "t-R1", "original_task_id": "t", "parent_attempt_id": "at1",
        "attempt_number": 2, "current_sha": SHA, "original_objective": "Implement.",
        "original_acceptance_criteria": [{"id": "c1", "description": "d", "kind": "SEMANTIC"}],
        "a4_findings": [{"finding_id": "f1", "severity": "HIGH", "category": "CORRECTNESS",
                         "description": "bug", "blocking": true, "line": 12,
                         "evidence_ref": {"ref_type": "ARTIFACT_ID", "target": "a"}}],
        "blocking_findings": [{"finding_id": "f1", "severity": "HIGH", "category": "CORRECTNESS",
                               "description": "bug", "blocking": true}],
        "failed_checks": [{"source": "REVIEW_EXECUTION", "command": ["cargo", "test"],
                           "exit_code": -101, "code_sha": SHA, "timed_out": false,
                           "started_at": "2026-11-08T00:00:00Z", "result": "FAIL"}],
        "repair_scope": "fix the bug", "prior_attempt_summary": "tried",
        "allowed_write_paths": ["src/**"], "forbidden_write_paths": ["docs/**"],
        "required_quality_floor": "FRONTIER", "branch": "runtime-a3/t-R1", "worktree": "",
        "context_epoch": 7
    })
}

#[test]
fn repair_capsule_codec_matches_schema_and_keeps_unbounded_integers() {
    let big = "123456789012345678901234567890";
    let text = repair()
        .to_string()
        .replace("\"context_epoch\":7", &format!("\"context_epoch\":{big}"));
    // serde_json would round this lexeme through f64: keep it as text only.
    let base = repair();
    let mut samples = vec![base.clone()];
    for key in [
        "blocking_findings",
        "current_sha",
        "worktree",
        "context_epoch",
    ] {
        samples.push(drop_key(&base, &[key]));
    }
    samples.extend([
        edit(&base, |v| v["attempt_number"] = json!(1)),
        edit(&base, |v| v["blocking_findings"] = json!([])),
        edit(&base, |v| {
            v["blocking_findings"][0]["category"] = json!("VIBES")
        }),
        edit(&base, |v| v["a4_findings"][0]["line"] = json!(0)),
        edit(&base, |v| v["failed_checks"][0]["exit_code"] = json!(1.5)),
        edit(&base, |v| v["failed_checks"][0]["timed_out"] = json!(null)),
        edit(&base, |v| v["repair_scope"] = json!("")),
        edit(&base, |v| v["prior_attempt_summary"] = json!(null)),
        edit(&base, |v| v["required_quality_floor"] = json!("LOW")),
    ]);
    agree(
        "RepairCapsule",
        |t| RepairCapsuleWireV1::from_json(t).is_ok(),
        &samples,
    );

    let wire = RepairCapsuleWireV1::from_json(&text).unwrap();
    assert!(wire.to_json().contains(big), "exact lexeme kept");
    let capsule = wire.to_repair_capsule(QualityFloor::Balanced).unwrap();
    assert_eq!(capsule.context_epoch().as_str(), big);
    assert_eq!(
        capsule.failed_checks().unwrap()[0].exit_code().as_str(),
        "-101"
    );
    let encoded = RepairCapsuleWireV1::from_repair_capsule(&capsule);
    assert_eq!(encoded, wire);
    // The parent floor is enforced on conversion.
    let economy = edit(&base, |v| v["required_quality_floor"] = json!("ECONOMY"));
    assert_eq!(
        RepairCapsuleWireV1::from_json(&economy.to_string())
            .unwrap()
            .to_repair_capsule(QualityFloor::Frontier),
        Err(GraphWireError::Unsupported("RepairCapsule"))
    );
    for (sample, field) in [
        (
            edit(&base, |v| v["attempt_number"] = json!(2.0)),
            "attempt_number",
        ),
        (
            edit(&base, |v| v["a4_findings"][0]["line"] = json!(1e1)),
            "line",
        ),
        (
            edit(&base, |v| {
                v["failed_checks"][0]["started_at"] = json!("yesterday")
            }),
            "started_at",
        ),
    ] {
        assert_eq!(
            RepairCapsuleWireV1::from_json(&sample.to_string())
                .unwrap()
                .to_repair_capsule(QualityFloor::Balanced),
            Err(GraphWireError::Unsupported(field))
        );
    }
}

#[test]
fn dispatch_admission_fixtures_roundtrip_and_negatives_refuse() {
    let schema = validator("DispatchAdmissionDecision");
    for name in [
        "C_anthropic_subscription_worker_denied",
        "D_openai_subscription_worker_unresolved",
        "E_openai_user_api_allowed",
    ] {
        let fixture = read(&format!("fixtures/admission/{name}.json"));
        assert!(schema.is_valid(&fixture));
        let wire = DispatchAdmissionDecisionWireV1::from_json(&fixture.to_string()).unwrap();
        same(&wire.to_json(), &fixture);
        let decision = wire.to_decision().unwrap();
        let encoded = DispatchAdmissionDecisionWireV1::from_decision(&decision);
        same(&encoded.to_json(), &fixture);
        assert_eq!(encoded.to_decision().unwrap(), decision);
    }
    let base = read("fixtures/admission/E_openai_user_api_allowed.json");
    let samples = [
        base.clone(),
        drop_key(&base, &["axis_results", "safety"]),
        drop_key(&base, &["selected_provider"]),
        drop_key(&base, &["denial_reason"]),
        edit(&base, |v| {
            v["axis_results"]["safety"]["result"] = json!("MAYBE")
        }),
        edit(&base, |v| v["axis_results"]["safety"]["extra"] = json!("x")),
        edit(&base, |v| {
            v["axis_results"]["provider_policy"]["policy_status"] = json!("OK")
        }),
        edit(&base, |v| v["failing_axis"] = json!("BUDGET")),
        edit(&base, |v| v["selected_provider"] = json!(3)),
        edit(&base, |v| v["decision_id"] = json!("")),
    ];
    agree(
        "DispatchAdmissionDecision",
        |t| DispatchAdmissionDecisionWireV1::from_json(t).is_ok(),
        &samples,
    );
    // Schema-valid but operationally contradictory: ALLOW naming a failing axis.
    let contradictory = edit(&base, |v| v["failing_axis"] = json!("SAFETY"));
    assert!(schema.is_valid(&contradictory));
    assert_eq!(
        DispatchAdmissionDecisionWireV1::from_json(&contradictory.to_string())
            .unwrap()
            .to_decision(),
        Err(GraphWireError::Unsupported("DispatchAdmissionDecision"))
    );
    // Absent and null selections both read as none; none encodes as null.
    let absent = drop_key(
        &drop_key(&base, &["selected_provider"]),
        &["selected_runtime"],
    );
    let absent = edit(&absent, |v| {
        v["outcome"] = json!("DENY");
        v["failing_axis"] = json!("SAFETY");
        v["denial_reason"] = json!("SAFETY_CHECK_PENDING");
        v["axis_results"]["safety"]["result"] = json!("FAIL");
    });
    let decision = DispatchAdmissionDecisionWireV1::from_json(&absent.to_string())
        .unwrap()
        .to_decision()
        .unwrap();
    assert_eq!(decision.core().core().selected_provider(), None);
    let encoded: Value =
        serde_json::from_str(&DispatchAdmissionDecisionWireV1::from_decision(&decision).to_json())
            .unwrap();
    assert_eq!(encoded["selected_provider"], Value::Null);
}

#[test]
fn feature_admission_fixtures_roundtrip_and_negatives_refuse() {
    for name in ["S22_free_invokes_pro", "S26_pro_cache_lost"] {
        let fixture = read(&format!("fixtures/admission/{name}.json"));
        let wire = FeatureAdmissionDecisionWireV1::from_json(&fixture.to_string()).unwrap();
        same(&wire.to_json(), &fixture);
        let decision = wire.to_decision().unwrap();
        same(
            &FeatureAdmissionDecisionWireV1::from_decision(&decision).to_json(),
            &fixture,
        );
    }
    let base = read("fixtures/admission/S22_free_invokes_pro.json");
    let samples = [
        base.clone(),
        edit(&base, |v| v["upgrade_info"] = json!(null)),
        edit(&base, |v| v["upgrade_info"] = json!("upgrade")),
        edit(&base, |v| v["upgrade_info"] = json!(1)),
        edit(&base, |v| v["outcome"] = json!("PROVIDER_DOWN")),
        edit(&base, |v| v["capability_id"] = json!("Review")),
        edit(&base, |v| v["capability_id"] = json!("review")),
        edit(&base, |v| v["dispatch_permitted"] = json!("no")),
        edit(&base, |v| v["activation_state"] = json!("UNKNOWN")),
        drop_key(&base, &["activation_state"]),
        edit(&base, |v| v["tier_id"] = json!(null)),
    ];
    agree(
        "FeatureAdmissionDecision",
        |t| FeatureAdmissionDecisionWireV1::from_json(t).is_ok(),
        &samples,
    );
    // upgrade_info null and string stay distinct operationally.
    for sample in [&samples[1], &samples[2]] {
        let decision = FeatureAdmissionDecisionWireV1::from_json(&sample.to_string())
            .unwrap()
            .to_decision()
            .unwrap();
        same(
            &FeatureAdmissionDecisionWireV1::from_decision(&decision).to_json(),
            sample,
        );
    }
}

fn catalog() -> Value {
    json!({"catalog_version": 3, "features": [
        {"capability_id": "graph.core", "name": "Graph", "description": "Core graph",
         "tier_required": "free", "status": "AVAILABLE_FREE"},
        {"capability_id": "review.independent_a4", "name": "", "description": "Independent A4",
         "tier_required": "pro", "status": "LOCKED_REQUIRES_PRO", "upgrade_hint": "Upgrade"}]})
}

#[test]
fn feature_capability_set_codec_matches_schema_and_roundtrips() {
    let base = catalog();
    let samples = [
        base.clone(),
        edit(&base, |v| v["features"] = json!([])),
        edit(&base, |v| v["catalog_version"] = json!(0)),
        edit(&base, |v| v["catalog_version"] = json!("3")),
        edit(&base, |v| v["features"][0]["status"] = json!("HIDDEN")),
        edit(&base, |v| v["features"][0]["description"] = json!("")),
        edit(&base, |v| v["features"][0]["tier_required"] = json!("")),
        drop_key(&base, &["catalog_version"]),
        edit(&base, |v| v["features"][1]["upgrade_hint"] = json!(null)),
    ];
    agree(
        "FeatureCapabilitySet",
        |t| FeatureCapabilitySetWireV1::from_json(t).is_ok(),
        &samples,
    );
    let wire = FeatureCapabilitySetWireV1::from_json(&base.to_string()).unwrap();
    let set = wire.to_capability_set().unwrap();
    same(
        &FeatureCapabilitySetWireV1::from_capability_set(&set).to_json(),
        &base,
    );
    let huge = base.to_string().replace(
        "\"catalog_version\":3",
        "\"catalog_version\":123456789012345678901234567890",
    );
    let set = FeatureCapabilitySetWireV1::from_json(&huge)
        .unwrap()
        .to_capability_set()
        .unwrap();
    assert_eq!(set.catalog_version(), "123456789012345678901234567890");
    let float = base
        .to_string()
        .replace("\"catalog_version\":3", "\"catalog_version\":3.0");
    assert_eq!(
        FeatureCapabilitySetWireV1::from_json(&float)
            .unwrap()
            .to_capability_set(),
        Err(GraphWireError::Unsupported("catalog_version"))
    );
}

fn evaluation() -> Value {
    json!({"evaluation_id": "ev1", "goal_id": "goal", "project_id": "p", "state": "INCOMPLETE",
           "deterministic_layer": {"passed": false, "conditions": [
               {"condition": "tests", "result": "FAIL"}, {"condition": "", "result": "NOT_APPLICABLE"}]},
           "integrated_sha": SHA, "context_epoch": 0, "evaluated_at": "2026-11-08T00:00:00Z"})
}

#[test]
fn goal_evaluation_codec_is_deterministic_only() {
    let base = evaluation();
    let samples = [
        base.clone(),
        edit(&base, |v| v["state"] = json!("DONE")),
        drop_key(&base, &["deterministic_layer", "passed"]),
        edit(&base, |v| v["semantic_layer"] = json!({"performed": false})),
        edit(&base, |v| v["semantic_layer"] = json!({"performed": "no"})),
        edit(&base, |v| {
            v["gaps"] = json!([{"criterion_id": "c", "description": "d"}])
        }),
        edit(&base, |v| v["gaps"] = json!([{"criterion_id": "c"}])),
        edit(&base, |v| {
            v["convergence"] = json!({"repeated_gap_count": 2, "escalated": true})
        }),
        edit(&base, |v| {
            v["convergence"] = json!({"repeated_gap_count": -1})
        }),
        edit(&base, |v| v["context_epoch"] = json!(-1)),
        edit(&base, |v| v["integrated_sha"] = json!("abc")),
        edit(&base, |v| {
            v["deterministic_layer"]["conditions"][0]["result"] = json!("OK")
        }),
    ];
    agree(
        "GoalEvaluation",
        |t| GoalEvaluationWireV1::from_json(t).is_ok(),
        &samples,
    );
    let core = GoalEvaluationWireV1::from_json(&base.to_string())
        .unwrap()
        .to_deterministic_core()
        .unwrap();
    same(
        &GoalEvaluationWireV1::from_deterministic_core(&core).to_json(),
        &base,
    );
    for (sample, field) in [
        (&samples[3], "semantic_layer"),
        (&samples[5], "gaps"),
        (&samples[7], "convergence"),
    ] {
        assert_eq!(
            GoalEvaluationWireV1::from_json(&sample.to_string())
                .unwrap()
                .to_deterministic_core(),
            Err(GraphWireError::Unsupported(field))
        );
    }
    // COMPLETE with a failed deterministic layer is schema-valid but refused.
    let complete = edit(&base, |v| v["state"] = json!("COMPLETE"));
    assert!(validator("GoalEvaluation").is_valid(&complete));
    assert_eq!(
        GoalEvaluationWireV1::from_json(&complete.to_string())
            .unwrap()
            .to_deterministic_core(),
        Err(GraphWireError::Unsupported("GoalEvaluation"))
    );
}

#[test]
fn max_cost_conversion_is_exact_or_unsupported() {
    let base = task().to_string();
    let convert = |lexeme: &str| {
        TaskCapsuleWireV1::from_json(
            &base.replace("\"max_cost\":2.5", &format!("\"max_cost\":{lexeme}")),
        )
        .unwrap()
        .to_task_capsule()
    };
    // F2: these used to succeed as 9007199254740992.0 and 0.0.
    for lossy in [
        "9007199254740993",
        "1e-400",
        "1e400",
        "0.1000000000000000055511151231257827021181583404541015626",
    ] {
        assert_eq!(
            convert(lossy).map(|c| c.cost_policy().unwrap().max_cost()),
            Err(GraphWireError::Unsupported("max_cost")),
            "{lossy}"
        );
    }
    // Values the operational number holds exactly convert and re-encode equal.
    for (lexeme, expected) in [
        ("2.5", 2.5),
        ("9007199254740992", 9_007_199_254_740_992.0),
        ("0.1", 0.1),
        ("1e22", 1e22),
        ("1E2", 100.0),
        ("5e-324", 5e-324),
        ("0", 0.0),
    ] {
        let capsule = convert(lexeme).unwrap();
        assert_eq!(
            capsule.cost_policy().unwrap().max_cost(),
            Some(Some(expected)),
            "{lexeme}"
        );
        let encoded = TaskCapsuleWireV1::from_task_capsule(&capsule).unwrap();
        let back = encoded.to_task_capsule().unwrap();
        assert_eq!(back.cost_policy(), capsule.cost_policy(), "{lexeme}");
    }
}

#[test]
fn encoder_never_emits_schema_invalid_task_capsule_wire() {
    let capsule = TaskCapsuleWireV1::from_json(&task().to_string())
        .unwrap()
        .to_task_capsule()
        .unwrap();
    let rebuild = |task_type, plan: Vec<crate::capsules::VerificationPlanEntryV1>| {
        crate::capsules::TaskCapsule::try_new(
            "t".into(),
            "ws".into(),
            None,
            1,
            task_type,
            "o".into(),
            capsule.acceptance_criteria().to_vec(),
            None,
            SHA.into(),
            SHA.into(),
            vec!["docs/**".into()],
            vec![],
            None,
            None,
            None,
            None,
            QualityFloor::Economy,
            vec![crate::CapabilityName::new("graph.core").unwrap()],
            None,
            plan,
            None,
            crate::capsules::AssuranceProfileSelector::Light,
            None,
            None,
            None,
            "runtime-a3/t".into(),
            "/w".into(),
            None,
            vec!["s".into()],
            None,
            0,
        )
        .unwrap()
    };
    // F3: DOCS/INVESTIGATION may plan nothing, the schema needs one entry.
    for task_type in [
        crate::capsules::TaskType::Docs,
        crate::capsules::TaskType::Investigation,
    ] {
        let empty = rebuild(task_type, vec![]);
        assert_eq!(
            TaskCapsuleWireV1::from_task_capsule(&empty),
            Err(GraphWireError::Unsupported("verification_plan"))
        );
        let planned = rebuild(task_type, capsule.verification_plan().to_vec());
        let wire = TaskCapsuleWireV1::from_task_capsule(&planned).unwrap();
        // Whatever encodes must decode again through the strict schema check.
        assert_eq!(TaskCapsuleWireV1::from_json(&wire.to_json()), Ok(wire));
    }
}
