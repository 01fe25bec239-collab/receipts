//! Actual cross-process State writer transfer; no recovery action is issued.
mod common;
use common::{d4, fixture};
use receipts_review_integration::state::{StateError, StateWriteRefusalV1, TrustedCoreStore};
use std::{
    fs,
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "writer fixture deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn spawn(root: &Path, mode: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["writer_probe", "--exact", "--ignored", "--nocapture"])
        .env("D4_WRITER_ROOT", root)
        .env("D4_WRITER_MODE", mode)
        .spawn()
        .unwrap()
}
fn join(mut child: Child) {
    until(|| child.try_wait().unwrap().is_some());
    assert!(child.wait().unwrap().success());
}
fn expected(root: &Path, id: &str, sha: &str) {
    fs::write(root.join("expected-id"), id).unwrap();
    fs::write(root.join("expected-sha"), sha).unwrap();
}

#[test]
#[ignore]
fn writer_probe() {
    let root = std::path::PathBuf::from(std::env::var_os("D4_WRITER_ROOT").unwrap());
    let mode = std::env::var("D4_WRITER_MODE").unwrap();
    let id = fs::read_to_string(root.join("expected-id")).unwrap();
    let sha = fs::read_to_string(root.join("expected-sha")).unwrap();
    let opened = TrustedCoreStore::open_exclusive(root.join("state/state.db"));
    if mode == "blocked" {
        assert!(matches!(
            opened,
            Err(StateError::WriteAuthorityRefused {
                reason: StateWriteRefusalV1::WriterLockHeld
            })
        ));
        return;
    }
    let mut store = opened.unwrap();
    if mode == "stale" {
        assert_eq!(
            d4::current_unfinished(&store, &id, "ws-1", &sha),
            Err(d4::Refusal::StaleAttempt)
        );
        fs::write(root.join("stale-refused"), b"no action").unwrap();
        return;
    }
    assert_eq!(d4::current_unfinished(&store, &id, "ws-1", &sha), Ok(()));
    fs::write(root.join("acquired"), b"").unwrap();
    until(|| root.join("release").exists());
    if mode == "complete" {
        d4::finish(&mut store, &sha);
    } else {
        assert_eq!(mode, "hold");
    }
    drop(store);
    fs::write(root.join("released"), b"").unwrap();
}

#[test]
fn controller_releases_supervisor_verifies_same_store_and_excludes_competitors() {
    let f = fixture("d4-transfer");
    let id = d4::seed(&f.db, "ws-1", &f.implementation);
    expected(&f.dir.0, &id, &f.implementation);
    let controller = TrustedCoreStore::open_exclusive(&f.db).unwrap();
    assert_eq!(
        d4::current_unfinished(&controller, "foreign", "ws-1", &f.implementation),
        Err(d4::Refusal::ForeignStore)
    );
    assert_eq!(
        d4::current_unfinished(&controller, &id, "foreign", &f.implementation),
        Err(d4::Refusal::ForeignAttempt)
    );
    join(spawn(&f.dir.0, "blocked"));
    drop(controller);
    let supervisor = spawn(&f.dir.0, "hold");
    until(|| f.dir.0.join("acquired").exists());
    join(spawn(&f.dir.0, "blocked"));
    fs::write(f.dir.0.join("release"), b"").unwrap();
    join(supervisor);
    let reopened = TrustedCoreStore::open_exclusive(&f.db).unwrap();
    assert_eq!(
        d4::current_unfinished(&reopened, &id, "ws-1", &f.implementation),
        Ok(())
    );
}

#[test]
fn competitor_wins_transfer_window_then_supervisor_refuses_changed_current_state() {
    let f = fixture("d4-transfer-race");
    let id = d4::seed(&f.db, "ws-1", &f.implementation);
    expected(&f.dir.0, &id, &f.implementation);
    let controller = TrustedCoreStore::open_exclusive(&f.db).unwrap();
    assert_eq!(
        d4::current_unfinished(&controller, &id, "ws-1", &f.implementation),
        Ok(())
    );
    drop(controller);
    let competitor = spawn(&f.dir.0, "complete");
    until(|| f.dir.0.join("acquired").exists());
    join(spawn(&f.dir.0, "blocked"));
    fs::write(f.dir.0.join("release"), b"").unwrap();
    join(competitor);
    let store = TrustedCoreStore::open_exclusive(&f.db).unwrap();
    let before = store
        .reader()
        .find_attempt(&receipts_review_integration::state::AttemptId::parse(d4::ATTEMPT).unwrap())
        .unwrap()
        .unwrap();
    assert!(before.is_finished());
    drop(store);
    join(spawn(&f.dir.0, "stale"));
    assert_eq!(
        fs::read(f.dir.0.join("stale-refused")).unwrap(),
        b"no action"
    );
    let store = TrustedCoreStore::open_exclusive(&f.db).unwrap();
    assert_eq!(
        store.reader().find_attempt(before.attempt_id()).unwrap(),
        Some(before)
    );
}
