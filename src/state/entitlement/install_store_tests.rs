use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use rusqlite::Connection;

use super::verification_vectors::*;
use super::*;
use crate::CanonicalTimestampV1;
use InstallCachedEntitlementFailure as Failure;
use InstallEntitlementIngestOutcome as Outcome;
use InstallEntitlementStoreError as Error;

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "receipts-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir.join("install.db"))
    }
    fn open(&self) -> InstallEntitlementRepository {
        InstallEntitlementRepository::open(&self.0).unwrap()
    }
    fn sql(&self) -> Connection {
        Connection::open(&self.0).unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}
fn verifier() -> EntitlementVerifier {
    EntitlementVerifier::new([(
        ProductEntitlementKeyId::new("k".into()).unwrap(),
        PUBLIC_KEY,
    )])
    .unwrap()
}
fn subject() -> ProductEntitlementSubjectId {
    ProductEntitlementSubjectId::new("é".into()).unwrap()
}
fn activation(kind: ActivationStateKind) -> ActivationState {
    let time = CanonicalTimestampV1::parse("2026-09-10T01:02:03.123456789Z").unwrap();
    ActivationState::new(
        ActivationIdentityFields::new(
            kind,
            (kind != ActivationStateKind::NeverActivated).then(|| "é".into()),
            Some("future-tier".into()),
        ),
        Some(time.clone()),
        Some(time.clone()),
        Some(time.clone()),
        Some(time.clone()),
        time,
    )
    .unwrap()
}
fn load(repo: &mut InstallEntitlementRepository) -> InstallEntitlementSnapshot {
    repo.load_reverified(&verifier(), Some(&subject())).unwrap()
}
fn ingest(
    repo: &mut InstallEntitlementRepository,
    raw: &str,
    state: Option<&ActivationState>,
) -> Result<Outcome, Error> {
    repo.ingest_and_persist(raw.as_bytes(), &verifier(), &subject(), state)
}
fn raw(db: &Database) -> Option<Vec<u8>> {
    use rusqlite::OptionalExtension;
    db.sql()
        .query_row(
            "SELECT raw_document FROM install_entitlement_cache",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}
fn corrupt(db: &Database, bytes: &[u8]) {
    db.sql()
        .execute(
            "UPDATE install_entitlement_cache SET raw_document = ?1",
            [bytes],
        )
        .unwrap();
}
fn schema(conn: &Connection) -> Vec<(String, String, String)> {
    conn.prepare(
        "SELECT type, name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
    )
    .unwrap()
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}
// Historical fixtures spell out the durable contract independently of production migrations.
fn historical(db: &Database, version: usize) {
    let conn = db.sql();
    conn.execute_batch(
        "CREATE TABLE install_entitlement_schema_version (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    migration_name TEXT NOT NULL
) STRICT;
INSERT INTO install_entitlement_schema_version VALUES (1, 'schema_foundation');",
    )
    .unwrap();
    if version >= 2 {
        conn.execute_batch(
            "CREATE TABLE install_activation_state (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    activation_state TEXT NOT NULL,
    subject_id TEXT,
    first_activated_at TEXT,
    last_known_tier_id TEXT,
    last_entitlement_seen_at TEXT,
    last_observed_server_time TEXT,
    logged_out_at TEXT,
    recorded_at TEXT NOT NULL
) STRICT;
INSERT INTO install_entitlement_schema_version VALUES (2, 'activation_state');",
        )
        .unwrap();
    }
}
fn assert_v3(db: &Database) {
    let conn = db.sql();
    let versions: Vec<(i64, String)> = conn.prepare("SELECT version, migration_name FROM install_entitlement_schema_version ORDER BY version").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(
        versions,
        vec![
            (1, "schema_foundation".into()),
            (2, "activation_state".into()),
            (3, "entitlement_cache".into())
        ]
    );
    let objects = schema(&conn);
    assert_eq!(
        objects
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "install_activation_state",
            "install_entitlement_cache",
            "install_entitlement_schema_version"
        ]
    );
    let ddl = format!("{objects:?}");
    for forbidden in [
        "project_id",
        "host_id",
        "provider",
        "credential",
        "claude",
        "codex",
        "state_schema_version",
    ] {
        assert!(!ddl.contains(forbidden));
    }
    assert!(conn.execute("INSERT INTO install_activation_state(singleton_id, activation_state, recorded_at) VALUES (2, 'LOGGED_OUT', 'bad')", []).is_err());
    assert!(
        conn.execute(
            "INSERT INTO install_entitlement_cache VALUES (2, X'00')",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO install_entitlement_cache VALUES (1, 'text')",
            []
        )
        .is_err()
    );
}
#[test]
fn empty_bootstrap_and_current_reopen_are_exact_and_noop() {
    let db = Database::new();
    let mut repo = db.open();
    let snapshot = load(&mut repo);
    assert_eq!(snapshot.activation, None);
    assert_eq!(snapshot.cache, InstallCachedEntitlement::Missing);
    assert_v3(&db);
    drop(repo);
    let before = std::fs::read(&db.0).unwrap();
    drop(db.open());
    assert_eq!(before, std::fs::read(&db.0).unwrap());
}
#[test]
fn committed_v1_resumes_v2_then_v3() {
    let db = Database::new();
    historical(&db, 1);
    drop(db.open());
    assert_v3(&db);
}
#[test]
fn committed_v2_resumes_v3_preserving_activation() {
    let db = Database::new();
    historical(&db, 2);
    db.sql().execute("INSERT INTO install_activation_state(singleton_id, activation_state, recorded_at) VALUES (1, 'ACTIVATED_KNOWN', '2026-09-10T00:00:00.000000000Z')", []).unwrap();
    let mut repo = db.open();
    assert_v3(&db);
    assert_eq!(
        load(&mut repo).activation.unwrap().activation_state(),
        ActivationStateKind::ActivatedKnown
    );
}
#[test]
fn unknown_gapped_newer_empty_and_malformed_metadata_fail_unchanged() {
    for mutation in [
        "DELETE FROM install_entitlement_schema_version WHERE version = 1",
        "UPDATE install_entitlement_schema_version SET version = 4 WHERE version = 3",
        "UPDATE install_entitlement_schema_version SET migration_name = 'unknown' WHERE version = 2",
        "DELETE FROM install_entitlement_schema_version",
        "DROP TABLE install_entitlement_schema_version; CREATE TABLE install_entitlement_schema_version(version TEXT); INSERT INTO install_entitlement_schema_version VALUES ('garbage')",
        "DROP TABLE install_activation_state",
        "ALTER TABLE install_entitlement_cache ADD COLUMN tier TEXT",
        "CREATE TRIGGER unexpected AFTER DELETE ON install_entitlement_cache BEGIN DELETE FROM install_activation_state; END",
    ] {
        let db = Database::new();
        drop(db.open());
        db.sql().execute_batch(mutation).unwrap();
        let before = std::fs::read(&db.0).unwrap();
        assert!(
            InstallEntitlementRepository::open(&db.0).is_err(),
            "{mutation}"
        );
        assert_eq!(before, std::fs::read(&db.0).unwrap(), "{mutation}");
    }
}
#[test]
fn foreign_and_project_databases_are_rejected_without_even_journal_mutation() {
    for table in ["unrelated", "state_schema_version"] {
        let db = Database::new();
        db.sql()
            .execute_batch(&format!(
                "CREATE TABLE {table}(payload TEXT); INSERT INTO {table} VALUES ('preserve me');"
            ))
            .unwrap();
        let before = std::fs::read(&db.0).unwrap();
        let before_schema = schema(&db.sql());
        assert_eq!(
            InstallEntitlementRepository::open(&db.0).unwrap_err(),
            Error::UnrecognizedSchema
        );
        assert_eq!(before, std::fs::read(&db.0).unwrap());
        assert_eq!(schema(&db.sql()), before_schema);
        assert_eq!(
            db.sql()
                .query_row(&format!("SELECT payload FROM {table}"), [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "preserve me"
        );
    }
}
#[test]
fn path_authority_rejects_relative_and_never_creates_parent() {
    let db = Database::new();
    let relative = PathBuf::from(format!("receipts-relative-{}.db", std::process::id()));
    assert!(!relative.exists());
    assert_eq!(
        InstallEntitlementRepository::open(&relative).unwrap_err(),
        Error::AbsolutePathRequired
    );
    assert!(!relative.exists());
    let missing = db.0.parent().unwrap().join("missing");
    assert_eq!(
        InstallEntitlementRepository::open(missing.join("db")).unwrap_err(),
        Error::ParentDirectoryMissing
    );
    assert!(!missing.exists());
}
#[test]
fn connection_durability_is_applied_and_read_back() {
    let db = Database::new();
    let repo = db.open();
    assert_eq!(
        repo.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    for (name, value) in [
        ("busy_timeout", 5000),
        ("synchronous", 2),
        ("foreign_keys", 1),
    ] {
        assert_eq!(
            repo.conn
                .query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))
                .unwrap(),
            value
        );
    }
}
#[test]
fn every_activation_kind_roundtrips_without_creating_cache() {
    let db = Database::new();
    for kind in ActivationStateKind::ALL {
        let state = activation(kind);
        let mut repo = db.open();
        repo.persist_activation(&state).unwrap();
        drop(repo);
        let snapshot = load(&mut db.open());
        assert_eq!(snapshot.activation, Some(state));
        assert_eq!(snapshot.cache, InstallCachedEntitlement::Missing);
    }
}
#[test]
fn malformed_activation_is_never_defaulted_repaired_or_reconstructed() {
    for mutation in [
        "activation_state = 'unknown'",
        "recorded_at = 'bad'",
        "first_activated_at = 'bad'",
        "activation_state = 'NEVER_ACTIVATED'",
    ] {
        let db = Database::new();
        let mut repo = db.open();
        ingest(
            &mut repo,
            VALID,
            Some(&activation(ActivationStateKind::ActivatedKnown)),
        )
        .unwrap();
        db.sql()
            .execute(
                &format!("UPDATE install_activation_state SET {mutation}"),
                [],
            )
            .unwrap();
        let before = schema(&db.sql());
        assert_eq!(
            repo.load_reverified(&verifier(), Some(&subject())),
            Err(Error::InvalidActivation)
        );
        assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
        assert_eq!(schema(&db.sql()), before);
    }
}
#[test]
fn exact_wire_survives_restart_and_equivalent_json_keeps_old_bytes() {
    let db = Database::new();
    let mut repo = db.open();
    let old = format!(" \n{VALID}\t");
    assert_eq!(ingest(&mut repo, &old, None), Ok(Outcome::Replace));
    drop(repo);
    let mut repo = db.open();
    let InstallCachedEntitlement::Verified(proof) = load(&mut repo).cache else {
        panic!()
    };
    assert_eq!(proof.raw_document(), old.as_bytes());
    assert_eq!(ingest(&mut repo, VALID, None), Ok(Outcome::KeepExisting));
    assert_eq!(raw(&db), Some(old.into_bytes()));
}
#[test]
fn cache_requires_trusted_subject_and_current_trust_on_every_load() {
    let db = Database::new();
    let mut repo = db.open();
    let state = activation(ActivationStateKind::ActivatedKnown);
    ingest(&mut repo, VALID, Some(&state)).unwrap();
    let snapshot = repo.load_reverified(&verifier(), None).unwrap();
    assert_eq!(snapshot.activation, Some(state.clone()));
    assert_eq!(
        snapshot.cache,
        InstallCachedEntitlement::Unusable(Failure::ExpectedSubjectUnavailable)
    );
    assert!(matches!(
        load(&mut repo).cache,
        InstallCachedEntitlement::Verified(_)
    ));
    let empty = EntitlementVerifier::new([]).unwrap();
    assert_eq!(
        repo.load_reverified(&empty, Some(&subject()))
            .unwrap()
            .cache,
        InstallCachedEntitlement::Unusable(Failure::VerificationRejected(
            EntitlementVerificationError::UnknownKeyId
        ))
    );
    let wrong = ProductEntitlementSubjectId::new("e\u{301}".into()).unwrap();
    assert_eq!(
        repo.load_reverified(&verifier(), Some(&wrong))
            .unwrap()
            .cache,
        InstallCachedEntitlement::Unusable(Failure::ActivationSubjectContradiction)
    );
    assert_eq!(load(&mut repo).activation, Some(state));
    assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
}
fn assert_unknown(snapshot: &InstallEntitlementSnapshot) {
    let state = snapshot.activation.as_ref().unwrap();
    assert_eq!(
        state.activation_state(),
        ActivationStateKind::ActivatedKnown
    );
    let evidence = match snapshot.cache {
        InstallCachedEntitlement::Missing => ObservedEntitlementEvidence::Missing,
        InstallCachedEntitlement::Unusable(_) => ObservedEntitlementEvidence::Corrupt,
        _ => panic!("unusable evidence must not yield proof"),
    };
    assert_eq!(
        resolve_product_entitlement_state(
            &ActivationIdentityFields::new(
                state.activation_state(),
                state.subject_id().map(str::to_owned),
                state.last_known_tier_id().map(str::to_owned)
            ),
            evidence,
            LicensingServiceAvailability::Unavailable,
            LocalClockEvidence::NoRollbackDetected
        ),
        ProductEntitlementState::EntitlementUnknown
    );
}
#[test]
fn corrupt_missing_and_unbound_cache_preserve_activated_unknown_without_cleanup() {
    let db = Database::new();
    let mut repo = db.open();
    let state = activation(ActivationStateKind::ActivatedKnown);
    ingest(&mut repo, VALID, Some(&state)).unwrap();
    assert_unknown(&repo.load_reverified(&verifier(), None).unwrap());
    for bytes in [
        b"{".to_vec(),
        VALID.replace("future-tier", "tampered").into_bytes(),
    ] {
        corrupt(&db, &bytes);
        let snapshot = load(&mut repo);
        assert_unknown(&snapshot);
        assert_eq!(snapshot.activation, Some(state.clone()));
        assert_eq!(raw(&db), Some(bytes));
    }
    db.sql()
        .execute("DELETE FROM install_entitlement_cache", [])
        .unwrap();
    let snapshot = load(&mut repo);
    assert_unknown(&snapshot);
    assert_eq!(snapshot.activation, Some(state));
}
#[test]
fn durable_replay_rejects_lower_divergent_and_altered_signature() {
    let db = Database::new();
    let mut repo = db.open();
    ingest(&mut repo, VALID, None).unwrap();
    drop(repo);
    let mut repo = db.open();
    for (incoming, error) in [
        (
            LOWER.to_owned(),
            EntitlementVerificationError::VersionRollback,
        ),
        (
            DIVERGENT.to_owned(),
            EntitlementVerificationError::EqualVersionDivergence,
        ),
        (
            VALID.replace("LSj0EC8o", "ASj0EC8o"),
            EntitlementVerificationError::SignatureVerificationFailed,
        ),
    ] {
        assert_eq!(
            ingest(
                &mut repo,
                &incoming,
                Some(&activation(ActivationStateKind::LoggedOut))
            ),
            Err(Error::IncomingRejected(error))
        );
        assert_eq!(load(&mut repo).activation, None);
        assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
    }
    let higher = format!("\n{HIGHER} ");
    assert_eq!(ingest(&mut repo, &higher, None), Ok(Outcome::Replace));
    assert_eq!(raw(&db), Some(higher.into_bytes()));
}
#[test]
fn corrupt_current_cache_blocks_authenticated_replacement() {
    let db = Database::new();
    let mut repo = db.open();
    ingest(&mut repo, VALID, None).unwrap();
    corrupt(&db, b"{");
    assert_eq!(
        ingest(
            &mut repo,
            HIGHER,
            Some(&activation(ActivationStateKind::ActivatedKnown))
        ),
        Err(Error::CurrentCacheRejected(
            EntitlementVerificationError::MalformedJson
        ))
    );
    assert_eq!(load(&mut repo).activation, None);
    assert_eq!(raw(&db), Some(b"{".to_vec()));
}
#[test]
fn subject_binding_device_binding_and_oversize_fail_without_mutation() {
    let db = Database::new();
    let mut repo = db.open();
    let state = activation(ActivationStateKind::ActivatedKnown);
    for (incoming, failure) in [
        (
            DECOMPOSED.to_owned(),
            EntitlementVerificationError::SubjectMismatch,
        ),
        (
            BOUND.to_owned(),
            EntitlementVerificationError::UnsupportedDeviceBindingV1,
        ),
        (
            " ".repeat(MAX_ENTITLEMENT_WIRE_BYTES + 1),
            EntitlementVerificationError::DocumentTooLarge,
        ),
    ] {
        assert_eq!(
            ingest(&mut repo, &incoming, Some(&state)),
            Err(Error::IncomingRejected(failure))
        );
        assert_eq!(load(&mut repo).activation, None);
        assert_eq!(raw(&db), None);
    }
    let wrong = ProductEntitlementSubjectId::new("other".into()).unwrap();
    assert_eq!(
        repo.ingest_and_persist(VALID.as_bytes(), &verifier(), &wrong, Some(&state)),
        Err(Error::ActivationSubjectContradiction)
    );
    repo.persist_activation(&state).unwrap();
    assert_eq!(
        repo.ingest_and_persist(VALID.as_bytes(), &verifier(), &wrong, None),
        Err(Error::ActivationSubjectContradiction)
    );
    assert_eq!(raw(&db), None);
    assert_eq!(load(&mut repo).activation, Some(state));
}
#[test]
fn optional_activation_replace_keep_and_clear_commit_together() {
    let db = Database::new();
    let mut repo = db.open();
    let known = activation(ActivationStateKind::ActivatedKnown);
    let logged = activation(ActivationStateKind::LoggedOut);
    assert_eq!(ingest(&mut repo, VALID, Some(&known)), Ok(Outcome::Replace));
    assert_eq!(load(&mut repo).activation, Some(known));
    assert_eq!(
        ingest(&mut repo, VALID, Some(&logged)),
        Ok(Outcome::KeepExisting)
    );
    assert_eq!(load(&mut repo).activation, Some(logged.clone()));
    assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
    repo.clear_cache_and_persist_activation(&logged).unwrap();
    assert_eq!(load(&mut repo).activation, Some(logged));
    assert_eq!(raw(&db), None);
}
#[test]
fn injected_failure_between_activation_and_cache_rolls_back_both_operations() {
    let db = Database::new();
    let mut repo = db.open();
    let old = activation(ActivationStateKind::ActivatedKnown);
    let next = activation(ActivationStateKind::LoggedOut);
    ingest(&mut repo, VALID, Some(&old)).unwrap();
    repo.fail_after_activation = true;
    assert_eq!(
        ingest(&mut repo, HIGHER, Some(&next)),
        Err(Error::InjectedFailure)
    );
    assert_eq!(load(&mut repo).activation, Some(old.clone()));
    assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
    assert_eq!(
        repo.clear_cache_and_persist_activation(&next),
        Err(Error::InjectedFailure)
    );
    assert_eq!(load(&mut repo).activation, Some(old));
    assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
}
#[test]
fn sql_failure_after_activation_rolls_back_both_operations() {
    let db = Database::new();
    let mut repo = db.open();
    let old = activation(ActivationStateKind::ActivatedKnown);
    let next = activation(ActivationStateKind::LoggedOut);
    ingest(&mut repo, VALID, Some(&old)).unwrap();
    for (operation, event) in [(true, "UPDATE"), (false, "DELETE")] {
        let conn = db.sql();
        conn.execute_batch(&format!("CREATE TRIGGER reject_cache BEFORE {event} ON install_entitlement_cache BEGIN SELECT RAISE(ABORT, 'sensitive SQL diagnostic'); END")).unwrap();
        let result = if operation {
            ingest(&mut repo, HIGHER, Some(&next)).map(|_| ())
        } else {
            repo.clear_cache_and_persist_activation(&next)
        };
        assert_eq!(result, Err(Error::Sqlite));
        assert_eq!(load(&mut repo).activation, Some(old.clone()));
        assert_eq!(raw(&db), Some(VALID.as_bytes().to_vec()));
        conn.execute_batch("DROP TRIGGER reject_cache").unwrap();
    }
}
#[test]
fn two_handles_observe_shared_commits_and_stale_writer_cannot_replace_newer() {
    let db = Database::new();
    let mut a = db.open();
    let mut b = db.open();
    ingest(
        &mut a,
        VALID,
        Some(&activation(ActivationStateKind::ActivatedKnown)),
    )
    .unwrap();
    assert!(matches!(
        load(&mut b).cache,
        InstallCachedEntitlement::Verified(_)
    ));
    ingest(&mut a, HIGHER, None).unwrap();
    assert_eq!(
        ingest(
            &mut b,
            VALID,
            Some(&activation(ActivationStateKind::LoggedOut))
        ),
        Err(Error::IncomingRejected(
            EntitlementVerificationError::VersionRollback
        ))
    );
    assert_eq!(raw(&db), Some(HIGHER.as_bytes().to_vec()));
    assert_eq!(
        load(&mut b).activation.unwrap().activation_state(),
        ActivationStateKind::ActivatedKnown
    );
}
#[test]
fn waiting_writer_reads_authority_only_after_obtaining_write_lock() {
    use std::sync::mpsc;
    let db = Database::new();
    let mut a = db.open();
    let mut b = db.open();
    ingest(&mut a, VALID, None).unwrap();
    let tx = a
        .conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "UPDATE install_entitlement_cache SET raw_document = ?1",
        [HIGHER.as_bytes()],
    )
    .unwrap();
    let (started, ready) = mpsc::channel();
    let (done, result) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        started.send(()).unwrap();
        done.send(ingest(&mut b, VALID, None)).unwrap();
    });
    ready.recv().unwrap();
    assert!(matches!(
        result.recv_timeout(std::time::Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    tx.commit().unwrap();
    assert_eq!(
        result.recv().unwrap(),
        Err(Error::IncomingRejected(
            EntitlementVerificationError::VersionRollback
        ))
    );
    writer.join().unwrap();
    assert_eq!(raw(&db), Some(HIGHER.as_bytes().to_vec()));
}
#[test]
fn snapshot_and_error_debug_redact_subject_signature_raw_and_sql_diagnostics() {
    let db = Database::new();
    let mut repo = db.open();
    ingest(
        &mut repo,
        VALID,
        Some(&activation(ActivationStateKind::ActivatedKnown)),
    )
    .unwrap();
    let snapshot = load(&mut repo);
    let text = format!("{repo:?} {snapshot:?}");
    let parsed: serde_json::Value = serde_json::from_str(VALID).unwrap();
    for secret in [VALID, "é", parsed["signature"].as_str().unwrap()] {
        assert!(!text.contains(secret));
    }
    for failure in [
        Error::Sqlite,
        Error::InvalidActivation,
        Error::ActivationSubjectContradiction,
        Error::IncomingRejected(EntitlementVerificationError::SignatureVerificationFailed),
    ] {
        let text = format!("{failure:?} {failure}");
        for secret in [VALID, "é", parsed["signature"].as_str().unwrap()] {
            assert!(!text.contains(secret));
        }
    }
}

#[test]
fn load_uses_one_snapshot_even_when_writer_commits_between_reads() {
    let db = Database::new();
    let mut reader = db.open();
    let mut writer = db.open();
    let old = activation(ActivationStateKind::ActivatedKnown);
    ingest(&mut writer, VALID, Some(&old)).unwrap();
    reader.after_activation_read = Some(Box::new(move || {
        ingest(
            &mut writer,
            HIGHER,
            Some(&activation(ActivationStateKind::LoggedOut)),
        )
        .unwrap();
    }));
    let snapshot = load(&mut reader);
    assert_eq!(snapshot.activation, Some(old));
    let InstallCachedEntitlement::Verified(proof) = snapshot.cache else {
        panic!()
    };
    assert_eq!(proof.raw_document(), VALID.as_bytes());
    let latest = load(&mut reader);
    assert_eq!(
        latest.activation.unwrap().activation_state(),
        ActivationStateKind::LoggedOut
    );
    let InstallCachedEntitlement::Verified(proof) = latest.cache else {
        panic!()
    };
    assert_eq!(proof.raw_document(), HIGHER.as_bytes());
}

#[test]
fn load_is_byte_for_byte_read_only_including_corrupt_evidence() {
    let db = Database::new();
    let mut repo = db.open();
    ingest(
        &mut repo,
        VALID,
        Some(&activation(ActivationStateKind::ActivatedKnown)),
    )
    .unwrap();
    for bytes in [VALID.as_bytes(), b"{"] {
        corrupt(&db, bytes);
        repo.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let before = std::fs::read(&db.0).unwrap();
        let changes = repo.conn.total_changes();
        load(&mut repo);
        repo.load_reverified(&verifier(), None).unwrap();
        assert_eq!(repo.conn.total_changes(), changes);
        assert_eq!(std::fs::read(&db.0).unwrap(), before);
        assert_eq!(
            std::fs::metadata(db.0.with_extension("db-wal"))
                .unwrap()
                .len(),
            0
        );
    }
}

#[test]
fn bypassed_singleton_constraint_does_not_hide_activation_corruption() {
    let db = Database::new();
    let mut repo = db.open();
    repo.persist_activation(&activation(ActivationStateKind::ActivatedKnown))
        .unwrap();
    db.sql().execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE install_activation_state SET singleton_id = 2").unwrap();
    assert_eq!(
        repo.load_reverified(&verifier(), Some(&subject())),
        Err(Error::InvalidActivation)
    );
}

#[test]
fn migration_ledger_failure_rolls_back_new_table_and_preserves_prior_version() {
    for version in [1, 2] {
        let db = Database::new();
        historical(&db, version);
        let mut conn = db.sql();
        conn.execute_batch("CREATE TRIGGER reject_version BEFORE INSERT ON install_entitlement_schema_version BEGIN SELECT RAISE(ABORT, 'injected ledger failure'); END").unwrap();
        let before = schema(&conn);
        {
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            assert_eq!(
                install_store_migrations::apply_step(&tx, version),
                Err(Error::Sqlite)
            );
            // DDL happened before the failing ledger write, but remains uncommitted.
            let new_table = if version == 1 {
                "install_activation_state"
            } else {
                "install_entitlement_cache"
            };
            assert_eq!(
                tx.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = ?1",
                    [new_table],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
        assert_eq!(schema(&conn), before);
        assert_eq!(
            conn.query_row(
                "SELECT max(version) FROM install_entitlement_schema_version",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            version as i64
        );
        conn.execute_batch("DROP TRIGGER reject_version").unwrap();
        drop(conn);
        drop(db.open());
        assert_v3(&db);
    }
}

#[test]
fn absent_identity_fields_roundtrip_and_cache_does_not_invent_activation() {
    let db = Database::new();
    let mut repo = db.open();
    ingest(&mut repo, VALID, None).unwrap();
    assert_eq!(load(&mut repo).activation, None);
    for kind in ActivationStateKind::ALL {
        let state = ActivationState::new(
            ActivationIdentityFields::new(kind, None, None),
            None,
            None,
            None,
            None,
            CanonicalTimestampV1::parse("2026-09-10T00:00:00.000000000Z").unwrap(),
        )
        .unwrap();
        repo.persist_activation(&state).unwrap();
        assert_eq!(load(&mut repo).activation, Some(state));
    }
}

#[test]
fn gapped_lower_version_and_wrong_historical_ddl_are_rejected() {
    for mutation in [
        "UPDATE install_entitlement_schema_version SET version = 2",
        "PRAGMA ignore_check_constraints = ON; UPDATE install_entitlement_schema_version SET version = 0",
        "ALTER TABLE install_entitlement_schema_version ADD COLUMN extra TEXT",
    ] {
        let db = Database::new();
        historical(&db, 1);
        db.sql().execute_batch(mutation).unwrap();
        let before = std::fs::read(&db.0).unwrap();
        assert!(InstallEntitlementRepository::open(&db.0).is_err());
        assert_eq!(std::fs::read(&db.0).unwrap(), before);
    }
}
