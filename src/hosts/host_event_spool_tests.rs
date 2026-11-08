use super::*;
use crate::{
    NormalizedHostEventConfidence, NormalizedHostEventId, NormalizedHostEventInputs,
    NormalizedHostEventType,
};
use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1,
};
use std::os::unix::fs::PermissionsExt;

struct Root(PathBuf);
impl Root {
    fn new(tag: &str) -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = base.join(format!("receipts-spool-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn event(payload_len: usize) -> NormalizedHostEvent {
    let mut payload = OrchestrationJsonObjectV1::default();
    payload.insert(
        "pad".into(),
        OrchestrationJsonValueV1::String("x".repeat(payload_len)),
    );
    NormalizedHostEvent::try_new(NormalizedHostEventInputs {
        event_id: NormalizedHostEventId::try_new("01J9ZZZZZZZZZZZZZZZZZZZZZZ").unwrap(),
        event_type: NormalizedHostEventType::HostSessionStarted,
        host: crate::HostId::ClaudeCode.into(),
        host_session_id: "s".into(),
        project_id: None,
        occurred_at: OrchestrationDateTimeV1::try_new("2026-11-08T00:00:00Z").unwrap(),
        payload,
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    })
    .unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn appends_owner_only_lines_and_creates_private_directories() {
    let root = Root::new("ok");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    assert_eq!(
        spool.path(),
        root.0.join(".receipts/inbox/host-events.jsonl")
    );
    spool.append(&event(1)).unwrap();
    spool.append(&event(2)).unwrap();
    assert_eq!(mode(&spool.path()), 0o600);
    assert_eq!(mode(&root.0.join(".receipts")), 0o700);
    assert_eq!(mode(&root.0.join(".receipts/inbox")), 0o700);
    let text = fs::read_to_string(spool.path()).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(text.ends_with('\n'));
    for (line, n) in lines.iter().zip([1, 2]) {
        assert_eq!(
            crate::wire::normalized_host_event_from_json(line).unwrap(),
            event(n)
        );
    }
    // Only the spool remains: the uid probe is removed, nothing else written.
    let entries: Vec<_> = fs::read_dir(root.0.join(".receipts/inbox"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries, vec!["host-events.jsonl"]);
}

#[test]
fn refuses_non_canonical_roots() {
    let root = Root::new("canon");
    assert_eq!(
        HostEventSpool::for_project(&root.0.join("..").join(root.0.file_name().unwrap())),
        Err(HostEventSpoolError::RootNotCanonical)
    );
    std::os::unix::fs::symlink(&root.0, root.0.with_extension("link")).unwrap();
    assert_eq!(
        HostEventSpool::for_project(&root.0.with_extension("link")),
        Err(HostEventSpoolError::RootNotCanonical)
    );
    fs::remove_file(root.0.with_extension("link")).unwrap();
    assert!(HostEventSpool::for_project(Path::new("relative")).is_err());
}

#[test]
fn refuses_symlinked_fifo_linked_or_loose_entries_without_repair() {
    // Symlinked spool file pointing elsewhere: never followed.
    let root = Root::new("symlink");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    spool.append(&event(1)).unwrap();
    fs::remove_file(spool.path()).unwrap();
    let target = root.0.join("elsewhere");
    fs::write(&target, b"").unwrap();
    std::os::unix::fs::symlink(&target, spool.path()).unwrap();
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::UnsafeEntry)
    );
    assert_eq!(fs::read(&target).unwrap(), b"");

    // Hard link to another file.
    fs::remove_file(spool.path()).unwrap();
    fs::hard_link(&target, spool.path()).unwrap();
    fs::set_permissions(spool.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::UnsafeEntry)
    );
    assert_eq!(fs::read(&target).unwrap(), b"");

    // A loose file mode is refused and not chmod-repaired.
    fs::remove_file(spool.path()).unwrap();
    fs::write(spool.path(), b"").unwrap();
    fs::set_permissions(spool.path(), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::Permissions)
    );
    assert_eq!(mode(&spool.path()), 0o644);

    // A FIFO cannot block the open and is refused.
    fs::remove_file(spool.path()).unwrap();
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(spool.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::UnsafeEntry)
    );

    // Symlinked inbox directory.
    let root = Root::new("dirlink");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    fs::create_dir_all(root.0.join(".receipts")).unwrap();
    fs::set_permissions(root.0.join(".receipts"), fs::Permissions::from_mode(0o700)).unwrap();
    let other = root.0.join("other");
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&other, root.0.join(".receipts/inbox")).unwrap();
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::UnsafeEntry)
    );
    assert_eq!(fs::read_dir(&other).unwrap().count(), 0);

    // Group/other-writable directory.
    let root = Root::new("loose-dir");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    fs::create_dir_all(root.0.join(".receipts/inbox")).unwrap();
    fs::set_permissions(root.0.join(".receipts"), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        spool.append(&event(1)),
        Err(HostEventSpoolError::Permissions)
    );
    assert!(!spool.path().exists());
}

#[test]
fn bounds_line_and_total_size() {
    let root = Root::new("bounds");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    assert_eq!(
        spool.append(&event(HOST_EVENT_SPOOL_LINE_MAX_BYTES)),
        Err(HostEventSpoolError::LineTooLarge)
    );
    assert!(
        !spool.path().exists(),
        "refused before any filesystem write"
    );
    spool.append(&event(1)).unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(spool.path())
        .unwrap();
    file.set_len(HOST_EVENT_SPOOL_MAX_BYTES - 10).unwrap();
    assert_eq!(spool.append(&event(1)), Err(HostEventSpoolError::SpoolFull));
    assert_eq!(
        fs::metadata(spool.path()).unwrap().len(),
        HOST_EVENT_SPOOL_MAX_BYTES - 10
    );
}

#[test]
fn concurrent_appends_keep_whole_lines() {
    let root = Root::new("concurrent");
    let spool = HostEventSpool::for_project(&root.0).unwrap();
    std::thread::scope(|s| {
        for n in 0..8 {
            let spool = &spool;
            s.spawn(move || {
                for _ in 0..25 {
                    spool.append(&event(1000 + n)).unwrap();
                }
            });
        }
    });
    let text = fs::read_to_string(spool.path()).unwrap();
    assert_eq!(text.lines().count(), 200);
    for line in text.lines() {
        crate::wire::normalized_host_event_from_json(line).unwrap();
    }
}
