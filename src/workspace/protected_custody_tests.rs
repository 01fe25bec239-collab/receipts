//! Real filesystem controls for the descriptor-relative custody engine and the
//! custody reservation. They use the invoking user as every identity (no
//! privilege), so ownership changes are no-ops but modes, links, devices and
//! external sentinels are observed for real. A real chown to other users and
//! Linux enforcement stay with the hosted diagnostic.

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use super::*;
use crate::test_support::TempDir;

const SECRET: &str = "outside secret\n";

struct Layout {
    _dir: TempDir,
    common: PathBuf,
    worktree: PathBuf,
    outside: PathBuf,
}

fn write_mode(path: &Path, text: &str, mode: u32) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A Git-shaped common directory and worktree plus an unrelated outside area.
fn layout() -> Layout {
    let dir = TempDir::new("custody");
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let (common, worktree, outside) = (base.join("common"), base.join("wt"), base.join("outside"));
    for path in [
        &common,
        &common.join("objects/ab"),
        &common.join("hooks"),
        &worktree,
        &worktree.join("src"),
        &worktree.join("src/deep"),
        &outside,
        &outside.join("tree"),
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
    write_mode(&common.join("config"), "[core]\n", 0o664);
    write_mode(&common.join("HEAD"), "ref: refs/heads/main\n", 0o644);
    write_mode(&common.join("objects/ab/cdef"), "object", 0o444);
    write_mode(&worktree.join(".git"), "gitdir: x\n", 0o664);
    write_mode(&worktree.join("src/a.txt"), "payload", 0o600);
    write_mode(&worktree.join("src/deep/b.txt"), "deep", 0o644);
    write_mode(&outside.join("secret"), SECRET, 0o600);
    write_mode(&outside.join("tree/inner"), "inner", 0o600);
    Layout {
        _dir: dir,
        common,
        worktree,
        outside,
    }
}

type Snapshot = BTreeMap<PathBuf, (bool, u32, u32, u32, u64, u64, Vec<u8>)>;

/// Every object under `roots` (no link following): kind, mode, owner, inode,
/// link count and file bytes (or link target bytes).
fn snapshot(roots: &[&Path]) -> Snapshot {
    let mut out = Snapshot::new();
    let mut pending: Vec<PathBuf> = roots.iter().map(|p| p.to_path_buf()).collect();
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        let content = if metadata.is_file() {
            std::fs::read(&path).unwrap()
        } else if metadata.file_type().is_symlink() {
            std::fs::read_link(&path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            Vec::new()
        };
        out.insert(
            path.clone(),
            (
                metadata.is_dir(),
                metadata.mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
                metadata.ino(),
                metadata.nlink(),
                content,
            ),
        );
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        }
    }
    out
}

fn me(layout: &Layout) -> (u32, u32) {
    let metadata = std::fs::metadata(&layout.common).unwrap();
    (metadata.uid(), metadata.gid())
}

fn ownership(layout: &Layout, worker_gid: u32) -> Ownership {
    let (uid, gid) = me(layout);
    Ownership {
        controller_uid: uid,
        controller_gid: gid,
        worker_uid: uid,
        worker_gid,
    }
}

/// A group the invoking user also belongs to, other than the primary one, so a
/// gid change on a verified object is observable without privilege.
fn other_group(primary: u32) -> Option<u32> {
    let output = std::process::Command::new("id").arg("-G").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|group| group.parse::<u32>().ok())
        .find(|group| *group != primary)
}

fn open(layout: &Layout) -> Roots {
    Roots::open(&layout.common, &layout.worktree, None).unwrap()
}

fn preflight(layout: &Layout) -> Result<(), String> {
    preflight_roots(&open(layout), &layout.common, &layout.worktree)
}

fn assert_outside_untouched(layout: &Layout, before: &Snapshot) {
    assert_eq!(
        &snapshot(&[&layout.outside]),
        before,
        "outside objects changed"
    );
    assert_eq!(
        std::fs::read_to_string(layout.outside.join("secret")).unwrap(),
        SECRET
    );
    assert_eq!(
        std::fs::metadata(layout.outside.join("secret"))
            .unwrap()
            .mode()
            & 0o7777,
        0o600
    );
}

/// Preflight refuses, and a full snapshot of inside AND outside is unchanged.
fn assert_refused_atomically(layout: &Layout, needle: &str) {
    let all = [&layout.common, &layout.worktree, &layout.outside];
    let before = snapshot(&all.map(PathBuf::as_path));
    let error = preflight(layout).unwrap_err();
    assert!(error.contains(needle), "{error}");
    assert_eq!(
        snapshot(&all.map(PathBuf::as_path)),
        before,
        "mutated before refusal"
    );
}

#[test]
fn valid_layout_transfers_and_restores_through_verified_objects() {
    let layout = layout();
    let (uid, gid) = me(&layout);
    let outside_before = snapshot(&[&layout.outside]);
    let roots = open(&layout);
    preflight_roots(&roots, &layout.common, &layout.worktree).unwrap();
    let worker_gid = other_group(gid);
    acquire(&roots, &ownership(&layout, worker_gid.unwrap_or(gid))).unwrap();

    let mode = |path: &Path| std::fs::symlink_metadata(path).unwrap().mode() & 0o7777;
    assert_eq!(
        mode(&layout.common.join("config")),
        0o644,
        "group/other write stripped"
    );
    assert_eq!(mode(&layout.common.join("objects/ab/cdef")), 0o444);
    assert_eq!(mode(&layout.worktree), 0o1770);
    assert_eq!(mode(&layout.worktree.join(".git")), 0o644);
    assert_eq!(
        mode(&layout.worktree.join("src/a.txt")),
        0o600,
        "payload mode untouched"
    );
    let root = std::fs::metadata(&layout.worktree).unwrap();
    assert_eq!((root.uid(), root.mode() & 0o7777), (uid, 0o1770));
    if let Some(worker_gid) = worker_gid {
        for path in [layout.worktree.clone(), layout.worktree.join("src/a.txt")] {
            assert_eq!(std::fs::metadata(path).unwrap().gid(), worker_gid);
        }
    }

    restore(&open(&layout), uid, gid, 0o755).unwrap();
    assert_eq!(mode(&layout.worktree), 0o755);
    for path in [&layout.common, &layout.worktree] {
        let now = std::fs::metadata(path).unwrap();
        assert_eq!((now.uid(), now.gid()), (uid, gid));
    }
    assert_outside_untouched(&layout, &outside_before);
}

#[test]
fn external_pointer_symlink_is_refused_before_any_mutation() {
    let layout = layout();
    // Git itself would accept this pointer: the external file has the same
    // `gitdir:` content, so Git prechecks pass while chmod would follow it.
    write_mode(&layout.outside.join("pointer"), "gitdir: x\n", 0o600);
    std::fs::remove_file(layout.worktree.join(".git")).unwrap();
    symlink(layout.outside.join("pointer"), layout.worktree.join(".git")).unwrap();
    assert_refused_atomically(&layout, ".git pointer is a symlink");
    // The engine's own backstop refuses even when preflight was skipped.
    let all = [&layout.common, &layout.worktree, &layout.outside];
    let before = snapshot(&all.map(PathBuf::as_path));
    let (uid, gid) = me(&layout);
    let own = Ownership {
        controller_uid: uid,
        controller_gid: gid,
        worker_uid: uid,
        worker_gid: gid,
    };
    assert!(acquire(&open(&layout), &own).is_err());
    assert_eq!(snapshot(&all.map(PathBuf::as_path)), before);
    assert_eq!(
        std::fs::metadata(layout.outside.join("pointer"))
            .unwrap()
            .mode()
            & 0o7777,
        0o600,
        "external target mode untouched"
    );
}

/// Positive control for the F4 fixtures: the pathname-based mutation the
/// repair replaced really does follow a symlink and a hard link and change the
/// external object, so the refusal tests above would have caught it.
#[test]
fn legacy_pathname_mutation_would_have_changed_the_external_objects() {
    let layout = layout();
    write_mode(&layout.outside.join("pointer"), "gitdir: x\n", 0o600);
    std::fs::remove_file(layout.worktree.join(".git")).unwrap();
    symlink(layout.outside.join("pointer"), layout.worktree.join(".git")).unwrap();
    std::fs::set_permissions(
        layout.worktree.join(".git"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let mode = |path: PathBuf| std::fs::metadata(path).unwrap().mode() & 0o7777;
    assert_eq!(
        mode(layout.outside.join("pointer")),
        0o644,
        "symlink followed"
    );
    std::fs::hard_link(
        layout.outside.join("secret"),
        layout.common.join("objects/ab/alias"),
    )
    .unwrap();
    std::fs::set_permissions(
        layout.common.join("objects/ab/alias"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        mode(layout.outside.join("secret")),
        0o644,
        "hard link shared"
    );
}

#[test]
fn hardlink_aliases_in_metadata_and_payload_refuse_without_touching_the_alias() {
    for (inside, needle) in [
        ("common/objects/ab/alias", "hard links"),
        ("wt/src/alias", "hard links"),
    ] {
        let layout = layout();
        let base = layout.common.parent().unwrap();
        std::fs::hard_link(layout.outside.join("secret"), base.join(inside)).unwrap();
        assert_refused_atomically(&layout, needle);
        // The shared inode keeps its external owner, mode and bytes.
        let alias = std::fs::metadata(layout.outside.join("secret")).unwrap();
        assert_eq!((alias.mode() & 0o7777, alias.nlink()), (0o600, 2));
    }
    // The pointer itself may not be an alias either.
    let layout = layout();
    std::fs::hard_link(
        layout.worktree.join(".git"),
        layout.outside.join("pointer-alias"),
    )
    .unwrap();
    assert_refused_atomically(&layout, ".git pointer is a hard-linked file");
}

#[test]
fn a_hardlink_created_after_preflight_is_refused_at_the_object_and_never_modified() {
    let layout = layout();
    let (uid, gid) = me(&layout);
    let roots = open(&layout);
    preflight_roots(&roots, &layout.common, &layout.worktree).unwrap();
    // Replacement between preflight and the mutation phase.
    std::fs::hard_link(
        layout.outside.join("secret"),
        layout.common.join("objects/ab/late-alias"),
    )
    .unwrap();
    let own = Ownership {
        controller_uid: uid,
        controller_gid: gid,
        worker_uid: uid,
        worker_gid: gid,
    };
    let error = acquire(&roots, &own).unwrap_err();
    assert!(error.contains("hard links"), "{error}");
    let alias = std::fs::metadata(layout.outside.join("secret")).unwrap();
    assert_eq!(alias.mode() & 0o7777, 0o600, "alias was never chmod'ed");
}

#[test]
fn payload_symlinks_are_reowned_in_place_and_never_followed() {
    let layout = layout();
    let (_, gid) = me(&layout);
    symlink(
        layout.outside.join("secret"),
        layout.worktree.join("to-file"),
    )
    .unwrap();
    symlink(layout.outside.join("tree"), layout.worktree.join("to-dir")).unwrap();
    symlink("loop", layout.worktree.join("loop")).unwrap();
    symlink("missing", layout.worktree.join("dangling")).unwrap();
    let outside_before = snapshot(&[&layout.outside]);
    let roots = open(&layout);
    preflight_roots(&roots, &layout.common, &layout.worktree).unwrap();
    let worker_gid = other_group(gid);
    acquire(&roots, &ownership(&layout, worker_gid.unwrap_or(gid))).unwrap();
    for name in ["to-file", "to-dir", "loop", "dangling"] {
        let link = std::fs::symlink_metadata(layout.worktree.join(name)).unwrap();
        assert!(link.file_type().is_symlink(), "{name} stays a symlink");
        if let Some(worker_gid) = worker_gid {
            assert_eq!(link.gid(), worker_gid, "{name} itself was re-owned");
        }
    }
    assert_outside_untouched(&layout, &outside_before);
    restore(&open(&layout), me(&layout).0, gid, 0o755).unwrap();
    assert_outside_untouched(&layout, &outside_before);
}

#[test]
fn metadata_symlinks_special_files_and_overlaps_are_unsupported() {
    // Metadata symlink, even to an in-tree target.
    let layout = layout();
    symlink("config", layout.common.join("hooks/linked")).unwrap();
    assert_refused_atomically(&layout, "symlink in Git metadata is unsupported");

    // Special file in the payload.
    let layout = self::layout();
    let fifo = layout.worktree.join("src/pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert_refused_atomically(&layout, "special file is unsupported");

    // Overlapping roots.
    let layout = self::layout();
    let inner = layout.common.join("wt");
    std::fs::create_dir(&inner).unwrap();
    write_mode(&inner.join(".git"), "gitdir: x\n", 0o644);
    let error = preflight_layout(&layout.common, &inner).unwrap_err();
    assert!(error.contains("overlap"), "{error}");
    let nested_common = layout.worktree.join("meta");
    std::fs::create_dir(&nested_common).unwrap();
    let error = preflight_layout(&nested_common, &layout.worktree).unwrap_err();
    assert!(error.contains("overlap"), "{error}");
}

#[test]
fn replaced_or_redirected_roots_are_refused_and_nothing_outside_changes() {
    let layout = layout();
    let ids = preflight_layout(&layout.common, &layout.worktree).unwrap();
    let outside_before = snapshot(&[&layout.outside]);

    // A symlink in place of the worktree (redirected traversal).
    let moved = layout.worktree.with_file_name("wt-moved");
    std::fs::rename(&layout.worktree, &moved).unwrap();
    symlink(&layout.outside, &layout.worktree).unwrap();
    assert!(Roots::open(&layout.common, &layout.worktree, Some(ids)).is_err());
    assert!(
        Dir::open_absolute(&layout.worktree).is_err(),
        "final symlink is not followed"
    );
    assert_outside_untouched(&layout, &outside_before);

    // A different directory at the same path (replacement).
    std::fs::remove_file(&layout.worktree).unwrap();
    std::fs::create_dir(&layout.worktree).unwrap();
    write_mode(&layout.worktree.join(".git"), "gitdir: x\n", 0o644);
    let error = Roots::open(&layout.common, &layout.worktree, Some(ids)).unwrap_err();
    assert!(error.contains("replaced"), "{error}");
    // The original is still accepted when it is the same object.
    std::fs::remove_dir_all(&layout.worktree).unwrap();
    std::fs::rename(&moved, &layout.worktree).unwrap();
    Roots::open(&layout.common, &layout.worktree, Some(ids)).unwrap();
}

#[test]
fn deep_nesting_beyond_the_bound_is_refused() {
    let layout = layout();
    let mut deep = layout.worktree.join("src");
    for _ in 0..(MAX_DEPTH + 4) {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let error = preflight(&layout).unwrap_err();
    assert!(error.contains("nesting exceeds"), "{error}");
}

#[test]
fn mount_topology_refuses_any_mount_strictly_inside_a_root() {
    let roots = [Path::new("/srv/repo/.git"), Path::new("/srv/wt")];
    let line = |mount_point: &str| {
        format!("36 25 8:1 /a {mount_point} rw,relatime shared:1 - ext4 /dev/sda1 rw\n")
    };
    // Mounts at a root, beside it or above it are outside the check.
    for fine in ["/srv/wt", "/srv", "/srv/other", "/srv/wt2", "/"] {
        refuse_nested_mounts(&line(fine), &roots).unwrap_or_else(|e| panic!("{fine}: {e}"));
    }
    // Strictly inside: bind mounts, other filesystems, escaped spaces.
    for bad in [
        "/srv/wt/data",
        "/srv/repo/.git/objects",
        "/srv/wt/with\\040space",
    ] {
        let error = refuse_nested_mounts(&line(bad), &roots).unwrap_err();
        assert!(error.contains("lies inside custody root"), "{bad}: {error}");
    }
    let error = refuse_nested_mounts(&line("/srv/wt/with\\040space"), &roots).unwrap_err();
    assert!(error.contains("with space"), "{error}");
    // A different filesystem under the root also fails at the descriptor.
    let (root, dev) = (Path::new("/"), Path::new("/dev"));
    if std::fs::metadata(root).unwrap().dev() != std::fs::metadata(dev).unwrap().dev() {
        let error = Dir::open_absolute(root)
            .unwrap()
            .open_child(c"dev")
            .unwrap_err();
        assert!(error.to_string().contains("filesystem boundary"), "{error}");
    }
}

// ---------------------------------------------------------------- reservation

fn begin(layout: &Layout, attempt: &str) -> Result<Reservation, String> {
    Reservation::begin(&layout.common, attempt, &layout.worktree)
}

fn marker(layout: &Layout) -> String {
    std::fs::read_to_string(layout.common.join("receipts-custody.state")).unwrap()
}

#[test]
fn one_reservation_per_common_directory_and_a_loser_changes_nothing() {
    let layout = layout();
    let first = begin(&layout, "attempt-a").unwrap();
    first
        .record_identity(
            62000,
            OriginalOwner {
                uid: 7,
                gid: 8,
                worktree_mode: 0o755,
            },
            RootIds {
                common: (1, 2),
                worktree: (3, 4),
            },
        )
        .unwrap();
    first.set_phase(Phase::Custody).unwrap();
    let all = [&layout.common, &layout.worktree, &layout.outside];
    let before = snapshot(&all.map(PathBuf::as_path));
    let held = marker(&layout);
    assert!(
        held.contains("attempt=attempt-a") && held.contains("phase=custody"),
        "{held}"
    );
    assert!(held.contains("original_uid=7") && held.contains("original_worktree_mode=755"));

    let error = begin(&layout, "attempt-b").err().unwrap();
    assert!(
        error.contains("attempt attempt-a") && error.contains("owner alive"),
        "{error}"
    );
    assert_eq!(
        snapshot(&all.map(PathBuf::as_path)),
        before,
        "loser mutated"
    );
    assert_eq!(marker(&layout), held, "loser changed the record");
}

#[test]
fn process_exit_releases_only_the_lock_never_the_retained_state() {
    let layout = layout();
    let crashed = begin(&layout, "attempt-crash").unwrap();
    crashed.set_phase(Phase::Incomplete).unwrap();
    drop(crashed); // the fd closes: the flock is gone, the record is not
    let error = begin(&layout, "attempt-next").err().unwrap();
    assert!(
        error.contains("attempt attempt-crash")
            && error.contains("phase incomplete")
            && error.contains("no live owner: retained state"),
        "{error}"
    );
    assert!(marker(&layout).contains("attempt=attempt-crash"));
}

#[test]
fn complete_finish_frees_the_directory_for_safe_readmission() {
    let layout = layout();
    let first = begin(&layout, "attempt-1").unwrap();
    first.finish().unwrap();
    assert!(!layout.common.join("receipts-custody.state").exists());
    // The finished holder's still-open descriptor does not block the next one.
    let second = begin(&layout, "attempt-2").unwrap();
    assert!(marker(&layout).contains("attempt=attempt-2"));
    drop(first);
    assert!(marker(&layout).contains("attempt=attempt-2"));
    second.finish().unwrap();
}

#[test]
fn a_marker_symlink_is_never_followed_or_modified() {
    let layout = layout();
    symlink(
        layout.outside.join("secret"),
        layout.common.join("receipts-custody.state"),
    )
    .unwrap();
    let outside_before = snapshot(&[&layout.outside]);
    let error = begin(&layout, "attempt-link").err().unwrap();
    assert!(error.contains("unreadable"), "{error}");
    assert_outside_untouched(&layout, &outside_before);
}

#[test]
fn persisted_original_owner_evidence_is_verified_not_assumed() {
    let layout = layout();
    let reservation = begin(&layout, "attempt-v").unwrap();
    let original = OriginalOwner {
        uid: 11,
        gid: 12,
        worktree_mode: 0o750,
    };
    reservation
        .record_identity(
            62001,
            original,
            RootIds {
                common: (1, 1),
                worktree: (2, 2),
            },
        )
        .unwrap();
    reservation.verify_original(original).unwrap();
    assert!(
        reservation
            .verify_original(OriginalOwner { uid: 0, ..original })
            .is_err()
    );
    let tampered = marker(&layout).replace("original_uid=11", "original_uid=0");
    std::fs::write(layout.common.join("receipts-custody.state"), tampered).unwrap();
    assert!(reservation.verify_original(original).is_err());
}

#[test]
fn concurrent_contenders_have_exactly_one_winner() {
    let layout = Arc::new(layout());
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let (layout, barrier) = (layout.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                begin(&layout, &format!("contender-{index}"))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let winner = results
        .iter()
        .position(|r| r.is_ok())
        .map(|index| format!("attempt=contender-{index}"))
        .unwrap();
    assert!(marker(&layout).contains(&winner));
}

/// Child half of the process-boundary test: reports whether this separate
/// process can reserve the directory named by the environment.
#[test]
#[ignore = "child process of reservation_excludes_a_separate_process"]
fn reservation_child_probe() {
    let Some(common) = std::env::var_os("RECEIPTS_CUSTODY_PROBE_COMMON") else {
        return;
    };
    match Reservation::begin(Path::new(&common), "child-process", Path::new("/unused")) {
        Ok(reservation) => {
            reservation.finish().unwrap();
            println!("CHILD ACQUIRED");
        }
        Err(error) => println!("CHILD REFUSED {error}"),
    }
}

#[test]
fn reservation_excludes_a_separate_process() {
    let layout = layout();
    let probe = || {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "protected_custody::tests::reservation_child_probe",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("RECEIPTS_CUSTODY_PROBE_COMMON", &layout.common)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let held = begin(&layout, "parent").unwrap();
    let during = probe();
    assert!(during.contains("CHILD REFUSED"), "{during}");
    assert!(during.contains("owner alive"), "{during}");
    // Parent exit/drop without finish: the other process still refuses.
    drop(held);
    let after_crash = probe();
    assert!(
        after_crash.contains("no live owner: retained state"),
        "{after_crash}"
    );
    // Only a verified finish frees the directory for the other process.
    std::fs::remove_file(layout.common.join("receipts-custody.state")).unwrap();
    let finished = begin(&layout, "parent-2").unwrap();
    finished.finish().unwrap();
    let after = probe();
    assert!(after.contains("CHILD ACQUIRED"), "{after}");
}
