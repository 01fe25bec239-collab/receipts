use rusqlite::Connection;

/// Rewinds a current store to the exact v16 image by dropping every v17 object.
pub(crate) fn remove_v17(conn: &Connection) {
    remove_v18(conn);
    let mut sql = String::new();
    for statement in crate::migrations::v0017_decision_records::SQL.split(";\n") {
        let mut words = statement.split_whitespace();
        if let (Some("CREATE"), Some(kind), Some(name)) = (words.next(), words.next(), words.next())
        {
            sql.push_str(&format!("DROP {kind} {name};\n"));
        }
    }
    // Triggers first (on pre-existing tables), then the new tables.
    let (triggers, tables): (Vec<_>, Vec<_>) = sql.lines().partition(|l| l.contains("TRIGGER"));
    let mut ordered = triggers.join("\n");
    ordered.push('\n');
    ordered.push_str(&tables.into_iter().rev().collect::<Vec<_>>().join("\n"));
    ordered.push_str("\nDELETE FROM state_schema_version WHERE version=17;");
    conn.execute_batch(&ordered).unwrap();
}

use crate::decision_records::{
    DecisionRecordKind as K, DecisionRecordV1, ProvenanceContradictionV1 as C,
    ProvenanceLinkKind as L,
};
use crate::error::{RecordFailureCode as Code, StateError};
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const S: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn schema(kind: K) -> serde_json::Value {
    let text = match kind {
        K::RoutingDecision => include_str!("../../schemas/RoutingDecision.schema.json"),
        K::RegistryCapability => include_str!("../../schemas/ModelCapability.schema.json"),
        K::ModelObservation => include_str!("../../schemas/ModelObservation.schema.json"),
        K::AvailabilityObservation => include_str!("../../schemas/AvailabilityState.schema.json"),
        K::ReviewRequest => include_str!("../../schemas/ReviewRequest.schema.json"),
        K::A4Review => include_str!("../../schemas/A4Review.schema.json"),
        K::IntegrationRequest => include_str!("../../schemas/IntegrationRequest.schema.json"),
        K::IntegrationDecision => include_str!("../../schemas/IntegrationDecision.schema.json"),
    };
    serde_json::from_str(text).unwrap()
}

pub(crate) fn oracle(kind: K, json: &str) -> bool {
    let validator = jsonschema::validator_for(&schema(kind)).unwrap();
    validator.is_valid(&serde_json::from_str(json).unwrap())
}

/// (kind, storage id, minimal-but-meaningful valid record)
fn fixtures() -> Vec<(K, &'static str, String)> {
    vec![
        (
            K::RoutingDecision,
            "rd-1",
            r#"{"decision_id":"rd-1","request_id":"rq-1","task_id":"t-1","outcome":"SELECTED","selected_provider":"p","registry_freshness":{"stale":false},"mode":"AUTO_CURRENT","user_involved":false}"#.into(),
        ),
        (
            K::RegistryCapability,
            "cap-1",
            r#"{"subject_kind":"MODEL","provider_id":"p","model_id":"m","capability":"code.edit","value":{"x":[1,2.5,123456789012345678901234567890]},"confidence":"LOCAL_EMPIRICAL","observed_at":"2026-10-29T00:00:00Z","sample_size":null,"lifecycle_state":"ROUTABLE"}"#.into(),
        ),
        (
            K::ModelObservation,
            "obs-1",
            r#"{"observation_id":"obs-1","provider_id":"p","model_id":"m","runtime_id":"r","task_id":"t-1","task_class":"impl","outcome":"REJECTED","repair_count":0,"cost":null,"tokens":12,"observed_at":"x"}"#.into(),
        ),
        (
            K::AvailabilityObservation,
            "av-1",
            r#"{"provider_id":"p","state":"RATE_LIMITED","retry_after_seconds":30,"signal_source":"EXIT_CODE","observed_at":"2026-10-29T00:00:00Z"}"#.into(),
        ),
        (
            K::ReviewRequest,
            "rr-1",
            format!(r#"{{"request_id":"rr-1","task_id":"t-1","attempt_id":"att-1","implementation_sha":"{A}","baseline_sha":"{S}","objective":"o","acceptance_criteria":[{{"id":"c1","description":"d","kind":"DETERMINISTIC"}}],"allowed_write_paths":["src/**"],"assurance_profile":"STANDARD","review_scope":"FULL","context_epoch":0,"attempt_number":1}}"#),
        ),
        (
            K::A4Review,
            "rev-1",
            format!(r#"{{"review_id":"rev-1","task_id":"t-1","reviewed_sha":"{A}","verdict":"PASS","independence_attested":true,"blocking_findings":[],"nonblocking_findings":[{{"finding_id":"f-1","severity":"LOW","category":"DOC_GAP","description":"d","blocking":false}}],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#),
        ),
        (
            K::IntegrationRequest,
            "ir-1",
            format!(r#"{{"request_id":"ir-1","gate_level":"A2_ACCEPTANCE","source_branch":"b","source_sha":"{A}","target_ref":"main","tasks_included":[{{"task_id":"t-1","implementation_sha":"{A}","review_sha":"{A}","a4_verdict":"PASS"}}]}}"#),
        ),
        (
            K::IntegrationDecision,
            "id-1",
            format!(r#"{{"decision_id":"id-1","request_id":"ir-1","outcome":"ACCEPT","checks_evaluated":[{{"check":"ci","result":"PASS"}}],"provenance_chain":[{{"task_id":"t-1","start_sha":"{S}","implementation_sha":"{A}","review_sha":"{A}"}}],"integration_sha":"{B}","decided_at":"2026-10-29T00:00:00Z"}}"#),
        ),
    ]
}

fn record(kind: K, id: &str, json: &str) -> DecisionRecordV1 {
    DecisionRecordV1::parse("p", kind, id, json).unwrap()
}

fn refused(result: Result<impl std::fmt::Debug, StateError>) -> Code {
    match result.unwrap_err() {
        StateError::DecisionRecord { code, .. } => code,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn every_record_kind_appends_reads_reopens_and_matches_schema_oracle() {
    let tmp = TempDir::new("decision-all");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    for (kind, id, json) in fixtures() {
        assert!(oracle(kind, &json), "{kind:?} fixture must be schema-valid");
        repo.append_decision_record(&record(kind, id, &json))
            .unwrap();
    }
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    for (kind, id, json) in fixtures() {
        let found = repo.find_decision_record("p", kind, id).unwrap().unwrap();
        assert_eq!(found.record_json(), json, "exact bytes kept");
        assert_eq!(found.record_sha256().len(), 64);
        assert_eq!(
            repo.list_decision_records("p", kind, None).unwrap(),
            vec![found]
        );
    }
    let review = repo
        .find_decision_record("p", K::A4Review, "rev-1")
        .unwrap()
        .unwrap();
    assert_eq!(
        (review.task_id(), review.subject_sha()),
        (Some("t-1"), Some(A))
    );
    let decision = repo
        .find_decision_record("p", K::RoutingDecision, "rd-1")
        .unwrap()
        .unwrap();
    assert_eq!(decision.link_id(), Some("rq-1"));
}

#[test]
fn routing_decision_is_append_only() {
    let tmp = TempDir::new("decision-append-only");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let (kind, id, json) = fixtures().remove(0);
    repo.append_decision_record(&record(kind, id, &json))
        .unwrap();
    let other = json.replace("SELECTED", "BLOCKED");
    assert_eq!(
        refused(repo.append_decision_record(&record(kind, id, &other))),
        Code::AlreadyExists
    );
    assert_eq!(
        refused(repo.append_decision_record(&record(kind, id, &json))),
        Code::AlreadyExists
    );
    for sql in [
        "UPDATE decision_records SET record_json='{}'",
        "DELETE FROM decision_records",
        "INSERT OR REPLACE INTO decision_records SELECT * FROM decision_records",
    ] {
        assert!(repo.connection().execute_batch(sql).is_err(), "{sql}");
    }
    assert_eq!(
        repo.find_decision_record("p", kind, id)
            .unwrap()
            .unwrap()
            .record_json(),
        json
    );
}

#[test]
fn records_are_project_isolated_and_task_filtered() {
    let tmp = TempDir::new("decision-isolation");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let (kind, id, json) = fixtures().remove(5);
    repo.append_decision_record(&record(kind, id, &json))
        .unwrap();
    repo.append_decision_record(&DecisionRecordV1::parse("q", kind, id, &json).unwrap())
        .unwrap();
    let other_task = json
        .replace("\"rev-1\"", "\"rev-2\"")
        .replace("\"t-1\"", "\"t-2\"");
    repo.append_decision_record(&record(kind, "rev-2", &other_task))
        .unwrap();
    assert!(repo.find_decision_record("r", kind, id).unwrap().is_none());
    assert_eq!(
        repo.list_decision_records("p", kind, None).unwrap().len(),
        2
    );
    let t1 = repo.list_decision_records("p", kind, Some("t-1")).unwrap();
    assert_eq!((t1.len(), t1[0].project_id()), (1, "p"));
    assert_eq!(
        repo.list_decision_records("q", kind, None).unwrap().len(),
        1
    );
    assert!(
        repo.list_decision_records("p", K::ReviewRequest, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn top_level_shape_refusals_agree_with_schema_oracle() {
    let fixtures = fixtures();
    let cases: Vec<(K, &str, String)> = vec![
        (
            K::RoutingDecision,
            "rd-1",
            fixtures[0].2.replace(r#""mode":"AUTO_CURRENT","#, ""),
        ),
        (
            K::RoutingDecision,
            "rd-1",
            fixtures[0].2.replace("SELECTED", "selected"),
        ),
        (
            K::RoutingDecision,
            "rd-1",
            fixtures[0]
                .2
                .replace("\"user_involved\":false", "\"extra\":1"),
        ),
        (
            K::RoutingDecision,
            "rd-1",
            fixtures[0]
                .2
                .replace("\"user_involved\":false", "\"user_involved\":\"no\""),
        ),
        (
            K::RegistryCapability,
            "cap-1",
            fixtures[1]
                .2
                .replace("\"sample_size\":null", "\"sample_size\":-1"),
        ),
        (
            K::RegistryCapability,
            "cap-1",
            fixtures[1]
                .2
                .replace("\"sample_size\":null", "\"sample_size\":1.5"),
        ),
        (
            K::ModelObservation,
            "obs-1",
            fixtures[2]
                .2
                .replace("\"repair_count\":0", "\"repair_count\":null"),
        ),
        (
            K::AvailabilityObservation,
            "av-1",
            fixtures[3]
                .2
                .replace("\"provider_id\":\"p\"", "\"provider_id\":\"\""),
        ),
        (
            K::ReviewRequest,
            "rr-1",
            fixtures[4].2.replace(A, &A.to_uppercase()),
        ),
        (
            K::ReviewRequest,
            "rr-1",
            fixtures[4]
                .2
                .replace("\"attempt_number\":1", "\"attempt_number\":0"),
        ),
        (
            K::ReviewRequest,
            "rr-1",
            fixtures[4]
                .2
                .replace("\"context_epoch\":0", "\"context_epoch\":\"0\""),
        ),
        (
            K::A4Review,
            "rev-1",
            fixtures[5].2.replace(
                "\"independence_attested\":true",
                "\"independence_attested\":1",
            ),
        ),
        (
            K::IntegrationRequest,
            "ir-1",
            fixtures[6].2.replace(
                "\"request_id\":\"ir-1\"",
                &format!("\"request_id\":\"{}\"", "é".repeat(201)),
            ),
        ),
        (
            K::IntegrationDecision,
            "id-1",
            fixtures[7]
                .2
                .replace("\"integration_sha\":\"", "\"integration_sha\":1,\"x\":\""),
        ),
        (
            K::IntegrationDecision,
            "id-1",
            fixtures[7]
                .2
                .replace(r#"[{"check":"ci","result":"PASS"}]"#, "[ ]"),
        ),
    ];
    for (kind, id, json) in cases {
        assert!(!oracle(kind, &json), "oracle must refuse {json}");
        assert!(
            DecisionRecordV1::parse("p", kind, id, &json).is_err(),
            "{json}"
        );
    }
    // Schema-valid large/exponent integers and floats stay raw tokens.
    for value in [
        "0",
        "1e2",
        "100.0",
        "1e400",
        "123456789012345678901234567890",
    ] {
        let json = fixtures[2]
            .2
            .replace("\"repair_count\":0", &format!("\"repair_count\":{value}"));
        let parsed = DecisionRecordV1::parse("p", K::ModelObservation, "obs-1", &json).unwrap();
        assert!(parsed.record_json().contains(value));
    }
}

#[test]
fn duplicate_keys_identity_mismatch_and_wrapper_ids_are_refused() {
    let fixtures = fixtures();
    for json in [
        r#"{"decision_id":"rd-1","decision_id":"rd-1","request_id":"rq","outcome":"BLOCKED","registry_freshness":{"stale":true},"mode":"AUTO_CURRENT"}"#,
        r#"{"decision_id":"rd-1","request_id":"rq","outcome":"BLOCKED","registry_freshness":{"stale":true,"stale":false},"mode":"AUTO_CURRENT"}"#,
        r#"{"decision_id":"rd-1","request_id":"rq","outcome":"BLOCKED","registry_freshness":{"stale":true},"mode":"AUTO_CURRENT"} []"#,
        r#"["decision_id"]"#,
    ] {
        assert_eq!(
            refused(DecisionRecordV1::parse(
                "p",
                K::RoutingDecision,
                "rd-1",
                json
            )),
            Code::InvalidShape
        );
    }
    assert_eq!(
        refused(DecisionRecordV1::parse(
            "p",
            K::RoutingDecision,
            "other",
            &fixtures[0].2
        )),
        Code::IdentityMismatch
    );
    for (project, id) in [("", "rd-1"), ("p", ""), (&*"x".repeat(201), "rd-1")] {
        assert_eq!(
            refused(DecisionRecordV1::parse(
                project,
                K::RoutingDecision,
                id,
                &fixtures[0].2
            )),
            Code::InvalidShape
        );
    }
}

#[test]
fn tampered_stored_record_fails_closed() {
    let tmp = TempDir::new("decision-tamper");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let (kind, id, json) = fixtures().remove(0);
    repo.append_decision_record(&record(kind, id, &json))
        .unwrap();
    repo.connection()
        .execute_batch("DROP TRIGGER decision_records_no_update; UPDATE decision_records SET record_json=replace(record_json,'SELECTED','BLOCKED');")
        .unwrap();
    assert_eq!(
        refused(repo.find_decision_record("p", kind, id)),
        Code::CorruptStore
    );
    assert_eq!(
        refused(repo.list_decision_records("p", kind, None)),
        Code::CorruptStore
    );
}

fn chain_repo(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    for (kind, id, json) in fixtures() {
        repo.append_decision_record(&record(kind, id, &json))
            .unwrap();
    }
    (tmp, repo)
}

#[test]
fn provenance_chain_reports_missing_link() {
    let (_tmp, mut repo) = chain_repo("prov-missing");
    let empty = repo.provenance_chain("p", "t-1").unwrap();
    assert_eq!(empty.missing, L::ALL.to_vec());
    assert!(!empty.is_complete_and_consistent());
    repo.append_provenance_link("p", "t-1", L::StartSha, S)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::ImplementationSha, A)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::ReviewSha, A)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::VerdictRef, "rev-1")
        .unwrap();
    let partial = repo.provenance_chain("p", "t-1").unwrap();
    assert_eq!(
        partial.missing,
        vec![L::AcceptanceRef, L::WorkstreamSha, L::MainSha]
    );
    assert!(partial.contradictions.is_empty());
    repo.append_provenance_link("p", "t-1", L::AcceptanceRef, "id-1")
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::WorkstreamSha, B)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::MainSha, B)
        .unwrap();
    let full = repo.provenance_chain("p", "t-1").unwrap();
    assert!(full.is_complete_and_consistent(), "{full:?}");
    assert_eq!(full.link(L::MainSha), Some(B));
    // Another project/task sees none of it.
    assert_eq!(repo.provenance_chain("q", "t-1").unwrap().missing.len(), 7);
    assert_eq!(repo.provenance_chain("p", "t-2").unwrap().missing.len(), 7);
}

#[test]
fn provenance_link_cannot_be_rewritten() {
    let (_tmp, mut repo) = chain_repo("prov-immutable");
    repo.append_provenance_link("p", "t-1", L::ImplementationSha, A)
        .unwrap();
    for value in [A, B] {
        assert!(matches!(
            repo.append_provenance_link("p", "t-1", L::ImplementationSha, value),
            Err(StateError::DecisionRecord {
                code: Code::AlreadyExists,
                ..
            })
        ));
    }
    for sql in [
        "UPDATE provenance_links SET link_value='x'",
        "DELETE FROM provenance_links",
    ] {
        assert!(repo.connection().execute_batch(sql).is_err());
    }
    for (kind, value) in [
        (L::StartSha, "A".repeat(40)),
        (L::StartSha, "a".repeat(39)),
        (L::VerdictRef, String::new()),
        (L::VerdictRef, "x".repeat(201)),
    ] {
        assert!(matches!(
            repo.append_provenance_link("p", "t-1", kind, &value),
            Err(StateError::DecisionRecord {
                code: Code::InvalidShape,
                ..
            })
        ));
    }
    assert_eq!(
        repo.provenance_chain("p", "t-1")
            .unwrap()
            .link(L::ImplementationSha),
        Some(A)
    );
}

#[test]
fn provenance_chain_reports_contradictions_with_stored_records() {
    let (_tmp, mut repo) = chain_repo("prov-contradict");
    repo.append_provenance_link("p", "t-1", L::StartSha, B)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::ImplementationSha, B)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::ReviewSha, S)
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::VerdictRef, "rev-1")
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::AcceptanceRef, "id-1")
        .unwrap();
    repo.append_provenance_link("p", "t-1", L::MainSha, A)
        .unwrap();
    let chain = repo.provenance_chain("p", "t-1").unwrap();
    assert_eq!(
        chain.contradictions,
        vec![
            C::ReviewShaNotImplementationSha,
            C::VerdictShaMismatch,
            C::AcceptanceChainMismatch,
            C::MainShaMismatch
        ]
    );
    repo.append_provenance_link("p", "t-9", L::VerdictRef, "absent")
        .unwrap();
    repo.append_provenance_link("p", "t-9", L::AcceptanceRef, "absent")
        .unwrap();
    assert_eq!(
        repo.provenance_chain("p", "t-9").unwrap().contradictions,
        vec![C::DanglingVerdictRef, C::DanglingAcceptanceRef]
    );
    repo.append_provenance_link("p", "t-8", L::VerdictRef, "rev-1")
        .unwrap();
    assert_eq!(
        repo.provenance_chain("p", "t-8").unwrap().contradictions,
        vec![C::VerdictTaskMismatch]
    );
}

#[test]
fn v16_store_migrates_to_v17_preserving_rows_and_rejects_partial_v17() {
    let tmp = TempDir::new("decision-migrate");
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    repo.connection()
        .execute_batch("INSERT INTO logical_role(role_id,project_id,role_type,status,current_context_epoch) VALUES ('sentinel','p','RUNTIME_A1','ACTIVE','0')")
        .unwrap();
    remove_v17(repo.connection());
    crate::graph_repository::verify_v16(repo.connection(), crate::GraphPhase::Read).unwrap();
    // A pre-existing colliding object makes the v17 step fail atomically.
    repo.connection()
        .execute_batch("CREATE TABLE provenance_links (x)")
        .unwrap();
    drop(repo);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
    assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
    let conn = rusqlite::Connection::open(tmp.db_path()).unwrap();
    conn.execute_batch("DROP TABLE provenance_links").unwrap();
    crate::graph_repository::verify_v16(&conn, crate::GraphPhase::Read).unwrap();
    drop(conn);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        22
    );
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(repo.count_table_rows("logical_role").unwrap(), 1);
    assert_eq!(repo.count_table_rows("decision_records").unwrap(), 0);
    // A dropped v17 trigger is detected as corruption on open.
    repo.connection()
        .execute_batch("DROP TRIGGER provenance_links_no_delete")
        .unwrap();
    drop(repo);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
}

#[test]
fn stored_metadata_drift_is_corruption_before_task_filtering() {
    for (kind, id, json) in fixtures() {
        for column in ["task_id", "link_id", "subject_sha"] {
            for value in [None, Some(A)] {
                let tmp = TempDir::new("metadata-drift");
                let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
                let admitted = record(kind, id, &json);
                let original = match column {
                    "task_id" => admitted.task_id(),
                    "link_id" => admitted.link_id(),
                    _ => admitted.subject_sha(),
                };
                if original == value {
                    continue;
                }
                repo.append_decision_record(&admitted).unwrap();
                let guard: String = repo
                    .connection()
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE name='decision_records_no_update'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                repo.connection()
                    .execute_batch("DROP TRIGGER decision_records_no_update")
                    .unwrap();
                repo.connection()
                    .execute(&format!("UPDATE decision_records SET {column}=?1"), [value])
                    .unwrap();
                repo.connection().execute_batch(&guard).unwrap();
                let writes = repo.connection().total_changes();
                assert_eq!(
                    refused(repo.find_decision_record("p", kind, id)),
                    Code::CorruptStore
                );
                for filter in [None, Some("t-1"), Some(A), Some("absent")] {
                    assert_eq!(
                        refused(repo.list_decision_records("p", kind, filter)),
                        Code::CorruptStore
                    );
                }
                assert!(
                    repo.list_decision_records("q", kind, None)
                        .unwrap()
                        .is_empty()
                );
                assert_eq!(repo.connection().total_changes(), writes);
            }
        }
    }
}

#[test]
fn deep_duplicate_validation_subprocess() {
    const CHILD: &str = "RECEIPTS_DEEP_KEYS_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let arrays = format!("{}0{}", "[".repeat(20_000), "]".repeat(20_000));
        let mixed = format!(
            "{}{}{}",
            "{\"x\":[".repeat(4_000),
            r#"{"a":1e400}"#,
            "]}".repeat(4_000)
        );
        for nested in [&arrays, &mixed] {
            let raw = serde_json::value::RawValue::from_string(nested.clone()).unwrap();
            crate::decision_records::unique_keys(&raw).unwrap();
            let json = format!(
                r#"{{"subject_kind":"MODEL","capability":"c","value":{nested},"confidence":"UNVERIFIED","observed_at":"t"}}"#
            );
            assert_eq!(
                DecisionRecordV1::parse("p", K::RegistryCapability, "c", &json)
                    .unwrap()
                    .record_json(),
                json
            );
            let json = format!(
                r#"{{"manifest_id":"m","role_id":"r","epoch":0,"sources":[{{"ref":{{"ref_type":"URL","target":"x"}},"source_class":"REFERENCE"}}],"derived_state":{{"x":{nested}}}}}"#
            );
            let wire = crate::wire::ContextManifestWireV1::parse_json(&json).unwrap();
            assert!(wire.to_json().unwrap().contains(nested));
        }
        let duplicate = format!(
            "{}{}{}",
            "[".repeat(20_000),
            r#"{"a":1,"\u0061":2}"#,
            "]".repeat(20_000)
        );
        let raw = serde_json::value::RawValue::from_string(duplicate).unwrap();
        assert!(crate::decision_records::unique_keys(&raw).is_err());
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "decision_records_tests::deep_duplicate_validation_subprocess",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "deep validation child: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("deep validation timeout");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The pre-repair implementation, kept only as an independent oracle. It asks
/// serde for every child `RawValue` at every level, so it rescans nested
/// suffixes (quadratic in depth) but its semantics are serde's own.
fn reference_unique_keys(raw: &serde_json::value::RawValue) -> bool {
    use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
    use serde_json::value::RawValue;
    struct Children;
    impl<'de> Visitor<'de> for Children {
        type Value = Vec<&'de RawValue>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("JSON container")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut seen = std::collections::HashSet::new();
            let mut children = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                if !seen.insert(key) {
                    return Err(de::Error::custom("duplicate key"));
                }
                children.push(map.next_value::<&RawValue>()?);
            }
            Ok(children)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut children = Vec::new();
            while let Some(child) = seq.next_element::<&RawValue>()? {
                children.push(child);
            }
            Ok(children)
        }
    }
    let mut pending = vec![raw];
    while let Some(value) = pending.pop() {
        let children = match value.get().as_bytes().first() {
            Some(b'{') => value.deserialize_map(Children),
            Some(b'[') => value.deserialize_seq(Children),
            _ => continue,
        };
        match children {
            Ok(children) => pending.extend(children),
            Err(_) => return false,
        }
    }
    true
}

fn keys_ok(json: &str) -> bool {
    let raw = serde_json::value::RawValue::from_string(json.to_string()).unwrap();
    let verdict = crate::decision_records::unique_keys(&raw).is_ok();
    assert_eq!(verdict, reference_unique_keys(&raw), "oracle: {json}");
    verdict
}

#[test]
fn unique_keys_refuses_exactly_same_object_decoded_duplicates() {
    // (document, accepted)
    for (json, accepted) in [
        (r#"{"a":1,"a":2}"#, false),
        (r#"{"a":1,"\u0061":2}"#, false),
        (r#"{"\"":1,"\u0022":2}"#, false),
        (r#"{"\\":1,"\u005c":2}"#, false),
        (r#"{"é":1,"\u00e9":2}"#, false),
        (r#"{"\ud83d\ude00":1,"😀":2}"#, false),
        (r#"{"x":[{"k":1},{"k":2,"k":3}]}"#, false),
        (r#"[[{"a":{"b":1,"b":1}}]]"#, false),
        (r#"{"a":{"k":1},"b":{"k":2},"c":[{"k":3},{"k":4}]}"#, true),
        (r#"{"a":{"a":{"a":1}}}"#, true),
        (r#"{"a":[{"a":1},{"a":2}],"b":[[{"a":3}]]}"#, true),
        (r#"{"a":"a","b":"a"}"#, true),
        // Delimiters, quotes and backslashes inside strings are data.
        (r#"{"a":"}],\"{[,:","b":"\\","c":"\\\\\"}"}"#, true),
        (r#"{"a":"\"a\":1,\"a\":2"}"#, true),
        (r#"{"}":1,"]":2,"{":3,"[":4,",":5,":":6}"#, true),
        (r#"{"}":1,"}":2}"#, false),
        (r#"{"a\\":{"a":1},"a":[]}"#, true),
        (r#"{"a\\":1,"a\\":2}"#, false),
        // Exact numbers and literals are not keys.
        (
            r#"{"a":1e400,"b":123456789012345678901234567890,"c":-0.0,"d":[null,true,false]}"#,
            true,
        ),
        (r#"{"a":[1,2,3],"a":1e400}"#, false),
        // Scalars and empty containers.
        ("0", true),
        ("-1.5e+10", true),
        (r#""{""#, true),
        ("[]", true),
        ("{}", true),
        (r#"{"":1,"":2}"#, false),
        // A key serde cannot decode (lone surrogate) is refused, as before.
        (r#"{"\ud800":1}"#, false),
        (r#"{"a":"\ud800"}"#, true),
        (r#"{"":{"":{}}}"#, true),
    ] {
        assert_eq!(keys_ok(json), accepted, "{json}");
    }
}

#[test]
fn unique_keys_matches_oracle_on_generated_documents() {
    // Deterministic xorshift corpus: small documents over a tiny key alphabet
    // (plain and escaped spellings of the same keys), so duplicates are common.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }
    fn value(state: &mut u64, depth: u32, out: &mut String) {
        const KEYS: [&str; 6] = ["a", r"\u0061", "b", r#"\""#, r"\\", "}"];
        const SCALARS: [&str; 6] = ["1", "1e400", r#""a""#, r#""}],\"""#, "null", "[]"];
        match next(state) % if depth == 0 { 1 } else { 3 } {
            0 => out.push_str(SCALARS[(next(state) % 6) as usize]),
            1 => {
                out.push('[');
                for i in 0..next(state) % 4 {
                    if i > 0 {
                        out.push(',');
                    }
                    value(state, depth - 1, out);
                }
                out.push(']');
            }
            _ => {
                out.push('{');
                for i in 0..next(state) % 4 {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(KEYS[(next(state) % 6) as usize]);
                    out.push_str("\":");
                    value(state, depth - 1, out);
                }
                out.push('}');
            }
        }
    }
    let (mut state, mut accepted, mut refused) = (0x9e37_79b9_7f4a_7c15_u64, 0, 0);
    for _ in 0..3_000 {
        let mut json = String::new();
        value(&mut state, 5, &mut json);
        if keys_ok(&json) {
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    // The corpus must exercise both verdicts, or agreement proves nothing.
    assert!(accepted > 300 && refused > 300, "{accepted}/{refused}");
}

#[test]
fn unique_keys_work_is_linear_in_nesting_depth() {
    // The repaired walk visits each byte once. Calibrate against serde's own
    // single linear skip of the same text in the same process, so the check is
    // about scaling and tolerates slow or loaded machines. The pre-repair
    // path rescanned nested suffixes and was thousands of times slower here.
    use std::time::Instant;
    fn best_of(runs: u32, mut work: impl FnMut()) -> std::time::Duration {
        (0..runs)
            .map(|_| {
                let start = Instant::now();
                work();
                start.elapsed()
            })
            .min()
            .unwrap()
    }
    for nested in [
        format!("{}0{}", "[".repeat(20_000), "]".repeat(20_000)),
        format!(
            "{}{}{}",
            "{\"x\":[".repeat(4_000),
            r#"{"a":1e400}"#,
            "]}".repeat(4_000)
        ),
    ] {
        let raw = serde_json::value::RawValue::from_string(nested).unwrap();
        let linear = best_of(3, || {
            serde_json::from_str::<serde::de::IgnoredAny>(raw.get()).unwrap();
        });
        let walk = best_of(3, || {
            crate::decision_records::unique_keys(&raw).unwrap();
        });
        assert!(
            walk < linear * 200 + std::time::Duration::from_millis(50),
            "walk {walk:?} vs one serde pass {linear:?}"
        );
    }
}

#[test]
fn nested_duplicate_in_open_fields_is_refused_by_both_callers() {
    let open = r#"{"x":[{"k":1,"\u006b":2}]}"#;
    let json = format!(
        r#"{{"subject_kind":"MODEL","capability":"c","value":{open},"confidence":"UNVERIFIED","observed_at":"t"}}"#
    );
    assert_eq!(
        refused(DecisionRecordV1::parse(
            "p",
            K::RegistryCapability,
            "c",
            &json
        )),
        Code::InvalidShape
    );
    let json = format!(
        r#"{{"manifest_id":"m","role_id":"r","epoch":0,"sources":[{{"ref":{{"ref_type":"URL","target":"x"}},"source_class":"REFERENCE"}}],"derived_state":{open}}}"#
    );
    assert!(crate::wire::ContextManifestWireV1::parse_json(&json).is_err());
}

/// Test-only rewind, used solely on stores with the legacy one-version shape.
pub(crate) fn remove_v18(conn: &Connection) {
    remove_v19(conn);
    let head: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    if head < 18 {
        return;
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    for statement in crate::migrations::v0018_manifest_successors::SQL.split(";\n") {
        let mut words = statement.split_whitespace();
        if let (Some("CREATE"), Some(kind @ ("TRIGGER" | "INDEX")), Some(name)) =
            (words.next(), words.next(), words.next())
        {
            conn.execute_batch(&format!("DROP {kind} {name}")).unwrap();
        }
    }
    conn.execute_batch("DROP TABLE context_manifest_seal")
        .unwrap();
    let create = crate::migrations::v0018_manifest_successors::SQL
        .split(";\n")
        .next()
        .unwrap()
        .replace(
            "role_id TEXT NOT NULL REFERENCES",
            "role_id TEXT NOT NULL UNIQUE REFERENCES",
        );
    conn.execute_batch(&format!("{create}; INSERT INTO context_manifest_v18 SELECT * FROM context_manifest; DROP TABLE context_manifest; ALTER TABLE context_manifest_v18 RENAME TO context_manifest; DELETE FROM state_schema_version WHERE version=18; PRAGMA foreign_keys=ON;")).unwrap();
}

pub(crate) fn remove_v22(conn: &Connection) {
    let head: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    if head < 22 {
        return;
    }
    conn.execute_batch("DROP TABLE review_claim_seal; DROP TABLE context_rehydration_order; DELETE FROM state_schema_version WHERE version=22;").unwrap();
}

pub(crate) fn remove_v21(conn: &Connection) {
    remove_v22(conn);
    let head: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    if head < 21 {
        return;
    }
    conn.execute_batch("DROP TABLE atomic_publication_availability; DROP TABLE atomic_publication; DROP TABLE review_claim; DELETE FROM state_schema_version WHERE version=21;").unwrap();
}

pub(crate) fn remove_v20(conn: &Connection) {
    remove_v21(conn);
    let head: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    if head < 20 {
        return;
    }
    conn.execute_batch("DROP TABLE event_proposal_receipt; DROP TABLE availability_fact; DROP TABLE state_store_identity; DELETE FROM state_schema_version WHERE version=20;").unwrap();
}

pub(crate) fn remove_v19(conn: &Connection) {
    remove_v20(conn);
    let head: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    if head < 19 {
        return;
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let mut deps = Vec::new();
    let mut defs = Vec::new();
    for table in crate::migrations::v0019_row_identity::TABLES {
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        defs.push((*table, sql));
        let mut stmt=conn.prepare("SELECT type,name,sql FROM sqlite_master WHERE tbl_name=?1 AND type IN ('trigger','index') AND sql IS NOT NULL").unwrap();
        deps.extend(
            stmt.query_map([table], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap),
        );
    }
    for (kind, name, _) in &deps {
        conn.execute_batch(&format!("DROP {kind} {name}")).unwrap();
    }
    for (table, sql) in defs {
        let tail = sql[sql.find('(').unwrap()..]
            .trim_end_matches(" WITHOUT ROWID")
            .trim_end_matches(',');
        let name = if table == "context_manifest" {
            format!("\"{table}\"")
        } else {
            table.to_owned()
        };
        conn.execute_batch(&format!("CREATE TABLE rewind19 {tail}; INSERT INTO rewind19 SELECT * FROM {table}; DROP TABLE {table}; CREATE TABLE {name} {tail}; INSERT INTO {table} SELECT * FROM rewind19; DROP TABLE rewind19;")).unwrap();
    }
    for (_, _, sql) in deps {
        conn.execute_batch(&sql).unwrap();
    }
    conn.execute_batch("DELETE FROM state_schema_version WHERE version=19; PRAGMA foreign_keys=ON")
        .unwrap();
}
