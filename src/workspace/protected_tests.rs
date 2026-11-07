//! Platform-neutral protected-launch logic. The `unenforced_for_test` seam
//! exercises admission binding and channel semantics WITHOUT enforcement;
//! these tests prove no Linux confinement property.

use std::ffi::OsStr;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use super::*;
use crate::execution::{LiveProcessTerminalCause, ProcessRunRequest, ProcessTimeoutPolicy};
use crate::test_support::{TestRepo, git, stdout_trimmed};
use crate::{WorkspaceProvisionRequest, WorkspaceState};

fn config() -> ProtectedLaunchConfig {
    ProtectedLaunchConfig::new(
        61001,
        61001,
        61002,
        62000..=62009,
        "/sys/fs/cgroup/receipts",
    )
    .unwrap()
}

fn provisioned(repo: &TestRepo, task: Option<&str>) -> WorkspaceHandle {
    WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-protected",
        task,
        "task/protected",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap()
}

fn request(worktree: &Path) -> ProcessRunRequest {
    let git = crate::git::resolved_git_executable().unwrap();
    ProcessRunRequest::new(git, ["--version"], worktree, worktree).unwrap()
}

fn policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_millis(200)).unwrap()
}

fn send(
    channel: &ControllerChannel,
    sequence: u64,
    operation: ControllerOperation,
) -> Result<ControllerResponse, ProtectedLaunchError> {
    let credentials = channel.credentials();
    channel.handle(ControllerRequest {
        attempt_id: credentials.attempt_id,
        task_id: credentials.task_id,
        token: credentials.token,
        sequence,
        operation,
    })
}

#[cfg(not(target_os = "linux"))]
#[test]
fn native_non_linux_platforms_refuse_protected_launch() {
    let error = ProtectedLauncher::new(config()).unwrap_err();
    assert!(
        matches!(error, ProtectedLaunchError::Unsupported(_)),
        "{error:?}"
    );
}

/// Hosted CI runs unprivileged: admission must refuse, never degrade.
#[cfg(target_os = "linux")]
#[test]
fn unprivileged_linux_refuses_protected_launch() {
    if std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .any(|line| line.starts_with("Uid:") && line.split_whitespace().nth(2) == Some("0"))
    {
        return; // Privileged hosts are exercised only by the hosted diagnostic.
    }
    let error = ProtectedLauncher::new(config()).unwrap_err();
    assert!(
        matches!(error, ProtectedLaunchError::Unsupported(_)),
        "{error:?}"
    );
}

#[test]
fn config_requires_separated_non_root_identities() {
    let bad = [
        ProtectedLaunchConfig::new(0, 1, 2, 10..=11, "/c"),
        ProtectedLaunchConfig::new(5, 5, 6, 5..=9, "/c"),
        ProtectedLaunchConfig::new(5, 5, 5, 10..=11, "/c"),
        ProtectedLaunchConfig::new(5, 5, 6, 0..=3, "/c"),
        ProtectedLaunchConfig::new(5, 5, 6, 10..=11, "relative"),
    ];
    for result in bad {
        assert!(matches!(
            result,
            Err(ProtectedLaunchError::InvalidConfig(_))
        ));
    }
}

#[test]
fn seccomp_program_refuses_qualified_syscalls_and_namespace_clone() {
    let program = seccomp_program_x86_64();
    assert!(program.len() < u16::MAX as usize);
    // Architecture check first, x32 kill second.
    assert_eq!(
        program[0],
        SockFilter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 4
        }
    );
    assert_eq!(program[1].k, 0xC000_003E);
    assert_eq!(program[2].k, 0x8000_0000);
    for number in SECCOMP_DENIED_X86_64 {
        let index = program
            .iter()
            .position(|insn| insn.code == 0x15 && insn.k == number)
            .unwrap_or_else(|| panic!("syscall {number} not filtered"));
        assert_eq!(program[index + 1].k, 0x0005_0001, "EPERM for {number}");
    }
    let clone3 = program
        .iter()
        .position(|i| i.code == 0x15 && i.k == 435)
        .unwrap();
    assert_eq!(program[clone3 + 1].k, 0x0005_0026, "ENOSYS for clone3");
    assert!(
        program
            .iter()
            .any(|i| i.code == 0x45 && i.k == CLONE_NAMESPACE_FLAGS)
    );
    assert_eq!(program.last().unwrap().k, 0x7FFF_0000);
    // Every jump stays inside the program.
    for (index, insn) in program.iter().enumerate() {
        if insn.code & 0x07 == 0x05 {
            assert!(index + 1 + (insn.jt.max(insn.jf) as usize) < program.len());
        }
    }
}

#[cfg(unix)]
#[test]
fn custody_verification_refuses_unprotected_layouts() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let repo = TestRepo::new("protected-custody");
    let handle = provisioned(&repo, Some("task-1"));
    let me = std::fs::metadata(repo.path()).unwrap().uid();
    let common = std::fs::canonicalize(repo.path().join(".git")).unwrap();
    let worktree = std::fs::canonicalize(handle.worktree_path()).unwrap();
    // Temp ancestors are not controller custody here, and the worktree root
    // is not 01770: both refuse rather than pass.
    assert!(verify_custody(&common, &worktree, me, 1).is_err());
    std::fs::set_permissions(&worktree, std::fs::Permissions::from_mode(0o1770)).unwrap();
    assert!(verify_custody(&common, &worktree, me.wrapping_add(1), 1).is_err());
    std::fs::set_permissions(&worktree, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn controller_landlock_plan_is_bounded_to_task_paths() {
    let repo = TestRepo::new("protected-plan");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = ProtectedLauncher::unenforced_for_test(config())
        .admit(&handle, repo.path())
        .unwrap();
    let plan = controller_landlock_plan(&admission.binding);
    let full: Vec<_> = plan
        .iter()
        .filter(|(_, access)| *access == LANDLOCK_ACCESS_FS_ABI3)
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(
        full,
        vec![
            admission.binding.common_dir.clone(),
            admission.binding.worktree.clone()
        ]
    );
    assert!(
        plan.iter()
            .all(|(_, access)| access & !LANDLOCK_ACCESS_FS_ABI3 == 0)
    );
    assert!(!plan.iter().any(|(path, _)| path == Path::new("/etc")));
}

#[test]
fn admission_requires_task_binding_matching_head_branch_and_repository() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-admit");
    let untasked = provisioned(&repo, None);
    assert!(matches!(
        launcher.admit(&untasked, repo.path()),
        Err(ProtectedLaunchError::Custody(_))
    ));

    let other = TestRepo::new("protected-admit-other");
    let repo2 = TestRepo::new("protected-admit-2");
    let handle = provisioned(&repo2, Some("task-1"));
    assert!(matches!(
        launcher.admit(&handle, other.path()),
        Err(ProtectedLaunchError::Custody(_))
    ));

    std::fs::write(handle.worktree_path().join("f"), "x").unwrap();
    git(handle.worktree_path(), &["add", "f"]);
    git(handle.worktree_path(), &["commit", "-m", "drift"]);
    assert!(matches!(
        launcher.admit(&handle, repo2.path()),
        Err(ProtectedLaunchError::Custody(_))
    ));
}

/// Identities stay noncolliding and are never reused. Admissions here use
/// nonoverlapping repositories: one repository has one custody owner (F2).
#[test]
fn identities_are_noncolliding_and_never_reused() {
    let launcher = ProtectedLauncher::unenforced_for_test(
        ProtectedLaunchConfig::new(61001, 61001, 61002, 62000..=62001, "/c").unwrap(),
    );
    let (repo_a, repo_b, repo_c) = (
        TestRepo::new("protected-ids-a"),
        TestRepo::new("protected-ids-b"),
        TestRepo::new("protected-ids-c"),
    );
    let (handle_a, handle_b, handle_c) = (
        provisioned(&repo_a, Some("task-1")),
        provisioned(&repo_b, Some("task-1")),
        provisioned(&repo_c, Some("task-1")),
    );
    let first = launcher.admit(&handle_a, repo_a.path()).unwrap();
    let second = launcher.admit(&handle_b, repo_b.path()).unwrap();
    assert_ne!(first.binding.worker_uid, second.binding.worker_uid);
    assert_ne!(first.attempt_id(), second.attempt_id());
    drop((first, second));
    // Both identities stay retained after their custody was given back.
    assert!(matches!(
        launcher.admit(&handle_c, repo_c.path()),
        Err(ProtectedLaunchError::IdentityExhausted)
    ));
    // Exhaustion freed its own reservation again.
    assert!(!marker_path(&repo_c).exists());
    assert!(!marker_path(&repo_a).exists() && !marker_path(&repo_b).exists());
}

fn marker_path(repo: &TestRepo) -> PathBuf {
    repo.path().join(".git/receipts-custody.state")
}

fn marker_text(repo: &TestRepo) -> String {
    std::fs::read_to_string(marker_path(repo)).unwrap()
}

/// Every entry of the repository's Git directory with mode and size.
fn git_dir_listing(repo: &TestRepo) -> Vec<(PathBuf, u32, u64)> {
    use std::os::unix::fs::MetadataExt;
    let mut out = Vec::new();
    let mut pending = vec![repo.path().join(".git")];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        // The reservation record is the only thing admission may add.
        if path
            .file_name()
            .is_some_and(|name| name == "receipts-custody.state")
        {
            continue;
        }
        out.push((path.clone(), metadata.mode(), metadata.len()));
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        }
    }
    out.sort();
    out
}

fn unenforced(uids: std::ops::RangeInclusive<u32>) -> ProtectedLauncher {
    ProtectedLauncher::unenforced_for_test(
        ProtectedLaunchConfig::new(61001, 61001, 61002, uids, "/c").unwrap(),
    )
}

fn custody_refusal(result: Result<ProtectedAdmission, ProtectedLaunchError>) -> String {
    match result {
        Err(ProtectedLaunchError::Custody(detail)) => detail,
        other => panic!("expected a custody refusal, got {other:?}"),
    }
}

/// F2: one owner per actual common directory/worktree. A duplicate admission
/// of the same handle refuses before any change and burns no identity.
#[test]
fn duplicate_admission_of_one_handle_refuses_before_any_change() {
    let launcher = unenforced(62000..=62001);
    let repo = TestRepo::new("protected-dup");
    let handle = provisioned(&repo, Some("task-1"));
    let first = launcher.admit(&handle, repo.path()).unwrap();
    let (record, listing) = (marker_text(&repo), git_dir_listing(&repo));
    assert!(record.contains(&format!("attempt={}", first.attempt_id())));
    assert!(record.contains("phase=custody") && record.contains("worker_uid=62000"));
    assert!(record.contains("original_uid=") && record.contains("worktree_identity="));

    let detail = custody_refusal(launcher.admit(&handle, repo.path()));
    assert!(detail.contains("owner alive"), "{detail}");
    // Separate launcher instance with its own allocator: same resource.
    let detail = custody_refusal(unenforced(62000..=62001).admit(&handle, repo.path()));
    assert!(detail.contains(first.attempt_id()), "{detail}");
    assert_eq!(
        marker_text(&repo),
        record,
        "losing admissions changed the record"
    );
    assert_eq!(
        git_dir_listing(&repo),
        listing,
        "losing admissions mutated Git metadata"
    );

    // The refusals consumed no worker identity: after the owner gives custody
    // back, the next admission gets the next unused UID, not IdentityExhausted.
    drop(first);
    assert!(!marker_path(&repo).exists());
    let again = launcher.admit(&handle, repo.path()).unwrap();
    assert_eq!(again.binding.worker_uid, 62001);
}

/// F2: another linked worktree of the same repository, an alias spelling of
/// the repository, the worktree given as the repository root and a different
/// launcher all meet at the same common directory.
#[test]
fn linked_worktrees_aliases_and_launchers_share_one_custody_owner() {
    let repo = TestRepo::new_nested("protected-linked", "repo");
    let first_handle = provisioned(&repo, Some("task-1"));
    let second_handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-second",
        Some("task-2"),
        "task/second",
        repo.path().join("wt2"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let alias = repo.root.path().join("alias");
    std::os::unix::fs::symlink(repo.path(), &alias).unwrap();
    let dotdot = repo.path().join("wt").join("..");

    let (launcher_a, launcher_b) = (unenforced(62000..=62009), unenforced(62000..=62009));
    let first = launcher_a.admit(&first_handle, repo.path()).unwrap();
    let (record, listing) = (marker_text(&repo), git_dir_listing(&repo));
    for (launcher, handle, root) in [
        (&launcher_a, &second_handle, repo.path().to_path_buf()),
        (&launcher_b, &second_handle, repo.path().to_path_buf()),
        (&launcher_b, &second_handle, alias.clone()),
        (&launcher_b, &second_handle, dotdot.clone()),
        (
            &launcher_b,
            &second_handle,
            first_handle.worktree_path().to_path_buf(),
        ),
        (&launcher_b, &first_handle, alias.clone()),
        (&launcher_b, &first_handle, dotdot.clone()),
    ] {
        let detail = custody_refusal(launcher.admit(handle, &root));
        assert!(
            detail.contains(first.attempt_id()),
            "{}: {detail}",
            root.display()
        );
    }
    assert_eq!(marker_text(&repo), record);
    assert_eq!(git_dir_listing(&repo), listing);

    // After the first owner's complete release, the other worktree is free to
    // be admitted by a different launcher (safe re-admission).
    drop(first);
    assert!(!marker_path(&repo).exists());
    let second = launcher_b.admit(&second_handle, &alias).unwrap();
    assert!(marker_text(&repo).contains(second.attempt_id()));
}

/// Unrelated, nonoverlapping repositories are unaffected by each other.
#[test]
fn nonoverlapping_repositories_admit_independently() {
    let launcher = unenforced(62000..=62009);
    let (repo_a, repo_b) = (
        TestRepo::new("protected-n-a"),
        TestRepo::new("protected-n-b"),
    );
    let (handle_a, handle_b) = (
        provisioned(&repo_a, Some("task-1")),
        provisioned(&repo_b, Some("task-1")),
    );
    let first = launcher.admit(&handle_a, repo_a.path()).unwrap();
    let second = unenforced(62000..=62009)
        .admit(&handle_b, repo_b.path())
        .unwrap();
    assert!(marker_text(&repo_a).contains(first.attempt_id()));
    assert!(marker_text(&repo_b).contains(second.attempt_id()));
}

/// The test registry directory an admission used, from its phase trace.
fn traced_registry(trace: &[String]) -> Option<PathBuf> {
    trace
        .iter()
        .find_map(|event| event.strip_prefix("registry "))
        .map(PathBuf::from)
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Concurrent contenders with independent launchers: exactly one wins. Every
/// typed outcome is kept with its admission phase trace, the contender's test
/// registry entries and the marker it observed before its thread exited, so a
/// failure reports all six results instead of only a count.
#[test]
fn concurrent_admissions_have_exactly_one_winner() {
    let repo = TestRepo::new("protected-race");
    let handle = provisioned(&repo, Some("task-1"));
    let barrier = Barrier::new(6);
    let contenders: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    let launcher = unenforced(62000..=62009);
                    barrier.wait();
                    let result = launcher.admit(&handle, repo.path());
                    let trace = take_custody_trace_for_test();
                    let registry = traced_registry(&trace).map(|dir| (listing(&dir), dir));
                    let marker = std::fs::read_to_string(marker_path(&repo));
                    (result, trace, registry, marker)
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    let report: Vec<String> = contenders
        .iter()
        .enumerate()
        .map(|(index, (result, trace, registry, marker))| {
            let outcome = match result {
                Ok(admission) => format!("Ok(attempt {})", admission.attempt_id()),
                Err(error) => format!("Err({error:?})"),
            };
            let registry_after_join = registry.as_ref().map(|(_, dir)| dir.exists());
            format!(
                "contender {index}: {outcome}\n  trace: {trace:?}\n  registry before exit: \
                 {registry:?}\n  registry exists after join: {registry_after_join:?}\n  \
                 marker before exit: {marker:?}"
            )
        })
        .collect();
    let report = report.join("\n");
    eprintln!(
        "{report}\nmarker after join: {:?}",
        std::fs::read_to_string(marker_path(&repo))
    );
    let mut winners = Vec::new();
    for (result, ..) in contenders {
        match result {
            Ok(admission) => winners.push(admission),
            Err(ProtectedLaunchError::Custody(_)) => {}
            Err(other) => panic!("non-custody refusal {other:?}\n{report}"),
        }
    }
    assert_eq!(winners.len(), 1, "{report}");
    let winner = winners.pop().unwrap();
    assert!(marker_text(&repo).contains(winner.attempt_id()), "{report}");
    // The winner's reservation outlives its contender thread: its registry
    // must too, or giving custody back cannot be verified.
    drop(winner);
    assert!(
        !marker_path(&repo).exists(),
        "the winner could not release its reservation\n{report}"
    );
}

/// A losing contender's liveness probe runs while the owner has created the
/// marker but not yet locked it. The probe must not take that lock and turn
/// the only valid admission into a refusal (zero winners).
#[test]
fn a_losing_probe_inside_the_owner_create_lock_window_never_defeats_the_owner() {
    let repo = TestRepo::new("protected-probe-window");
    let handle = provisioned(&repo, Some("task-1"));
    let (created, probed, owner_done) = (
        Arc::new(Latch::default()),
        Arc::new(Latch::default()),
        Arc::new(Latch::default()),
    );
    let (owner, owner_trace, loser, loser_trace) = std::thread::scope(|scope| {
        let owner = scope.spawn(|| {
            let (created, probed) = (created.clone(), probed.clone());
            set_custody_hook_for_test(move |event| {
                if event == "marker created" {
                    created.open();
                    probed.wait();
                }
            });
            let result = unenforced(62000..=62009).admit(&handle, repo.path());
            (result, take_custody_trace_for_test())
        });
        let loser = scope.spawn(|| {
            created.wait();
            let (probed, owner_done) = (probed.clone(), owner_done.clone());
            set_custody_hook_for_test(move |event| {
                if event.starts_with("probe marker") {
                    probed.open();
                    owner_done.wait();
                }
            });
            let result = unenforced(62000..=62009).admit(&handle, repo.path());
            (result, take_custody_trace_for_test())
        });
        let (owner, owner_trace) = owner.join().unwrap();
        owner_done.open();
        let (loser, loser_trace) = loser.join().unwrap();
        (owner, owner_trace, loser, loser_trace)
    });
    let report = format!(
        "owner: {:?}\n  {owner_trace:?}\nloser: {loser:?}\n  {loser_trace:?}",
        owner.as_ref().map(ProtectedAdmission::attempt_id)
    );
    let owner = owner.unwrap_or_else(|error| panic!("owner refused: {error:?}\n{report}"));
    assert!(
        matches!(loser, Err(ProtectedLaunchError::Custody(_))),
        "{report}"
    );
    assert!(marker_text(&repo).contains(owner.attempt_id()), "{report}");
    drop(owner);
    assert!(!marker_path(&repo).exists(), "{report}");
}

/// A retained record from a crashed or incomplete owner is unknown prior
/// custody: it refuses, is never overwritten and is not recovered here.
#[test]
fn retained_reservation_state_refuses_new_admission_without_recovery() {
    let launcher = unenforced(62000..=62009);
    let repo = TestRepo::new("protected-retained");
    let handle = provisioned(&repo, Some("task-1"));
    let crashed = "receipts-custody-v1\nattempt=deadbeef\nphase=custody\nworker_uid=62000\n";
    std::fs::write(marker_path(&repo), crashed).unwrap();
    let detail = custody_refusal(launcher.admit(&handle, repo.path()));
    assert!(
        detail.contains("attempt deadbeef")
            && detail.contains("phase custody")
            && detail.contains("no live owner: retained state"),
        "{detail}"
    );
    assert_eq!(marker_text(&repo), crashed);
}

/// A repository already owned by the controller or in the worker group looks
/// like earlier custody; it refuses instead of being adopted as "original".
#[test]
fn ownership_that_already_looks_like_custody_is_unknown_prior_custody() {
    use std::os::unix::fs::MetadataExt;
    let repo = TestRepo::new("protected-prior");
    let handle = provisioned(&repo, Some("task-1"));
    let metadata = std::fs::metadata(repo.path().join(".git")).unwrap();
    let (uid, gid) = (metadata.uid(), metadata.gid());
    let as_controller = ProtectedLauncher::unenforced_for_test(
        ProtectedLaunchConfig::new(uid, uid, 62500, 62000..=62009, "/c").unwrap(),
    );
    let in_worker_group = ProtectedLauncher::unenforced_for_test(
        ProtectedLaunchConfig::new(61001, 61001, gid, 62000..=62009, "/c").unwrap(),
    );
    for launcher in [&as_controller, &in_worker_group] {
        let detail = custody_refusal(launcher.admit(&handle, repo.path()));
        assert!(detail.contains("unknown prior custody"), "{detail}");
        assert!(
            !marker_path(&repo).exists(),
            "refusal must free its own reservation"
        );
    }
}

/// An invalid layout is refused with the repository and Git metadata untouched
/// and the reservation freed: here an external `.git` pointer symlink that Git
/// itself accepts.
#[test]
fn admission_refuses_an_external_pointer_symlink_without_any_change() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let repo = TestRepo::new_nested("protected-pointer", "repo");
    let handle = provisioned(&repo, Some("task-1"));
    let external = repo.root.path().join("external-pointer");
    let pointer = handle.worktree_path().join(".git");
    std::fs::copy(&pointer, &external).unwrap();
    std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_file(&pointer).unwrap();
    std::os::unix::fs::symlink(&external, &pointer).unwrap();
    // Git's own prechecks accept this layout.
    git(handle.worktree_path(), &["rev-parse", "--git-common-dir"]);
    let listing = git_dir_listing(&repo);

    let detail = custody_refusal(unenforced(62000..=62009).admit(&handle, repo.path()));
    assert!(detail.contains(".git pointer is a symlink"), "{detail}");
    assert_eq!(std::fs::metadata(&external).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(git_dir_listing(&repo), listing);
    assert!(!marker_path(&repo).exists());
}

#[test]
fn channel_refuses_forgery_cross_task_replay_and_reviewer_writes() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-channel");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    assert_eq!(protected.handle().state(), WorkspaceState::Active);
    let worker = protected.worker_channel();
    let reviewer = protected.reviewer_channel();
    assert_ne!(worker.credentials().token, reviewer.credentials().token);
    assert!(!format!("{:?}", worker.credentials()).contains(&worker.credentials().token));

    let good = worker.credentials();
    let attempt = |change: &dyn Fn(&mut ControllerRequest)| {
        let mut request = ControllerRequest {
            attempt_id: good.attempt_id.clone(),
            task_id: good.task_id.clone(),
            token: good.token.clone(),
            sequence: 0,
            operation: ControllerOperation::Identity,
        };
        change(&mut request);
        worker.handle(request)
    };
    assert!(matches!(
        attempt(&|r| r.task_id = "task-2".into()),
        Err(ProtectedLaunchError::Binding("cross-task request"))
    ));
    assert!(matches!(
        attempt(&|r| r.token = "0".repeat(64)),
        Err(ProtectedLaunchError::Binding("forged request"))
    ));
    assert!(matches!(
        attempt(&|r| r.attempt_id = "other".into()),
        Err(ProtectedLaunchError::Binding("forged request"))
    ));
    // A reviewer token cannot drive the worker channel.
    let reviewer_token = reviewer.credentials().token;
    assert!(matches!(
        attempt(&|r| r.token = reviewer_token.clone()),
        Err(ProtectedLaunchError::Binding("forged request"))
    ));

    assert!(matches!(
        send(worker, 0, ControllerOperation::Identity),
        Ok(ControllerResponse::Identity { .. })
    ));
    assert!(matches!(
        send(worker, 0, ControllerOperation::Identity),
        Err(ProtectedLaunchError::Binding(
            "replayed or out-of-order request"
        ))
    ));
    assert!(matches!(
        send(worker, 5, ControllerOperation::Status),
        Err(ProtectedLaunchError::Binding(_))
    ));

    assert!(matches!(
        send(reviewer, 0, ControllerOperation::Status),
        Ok(ControllerResponse::Status { .. })
    ));
    assert!(matches!(
        send(
            reviewer,
            1,
            ControllerOperation::Commit {
                message: "x".into()
            }
        ),
        Err(ProtectedLaunchError::Binding(
            "reviewer channel is query-only"
        ))
    ));
    let outcome = protected.attempt().wait_collect().unwrap();
    assert_eq!(
        outcome.terminal_cause(),
        LiveProcessTerminalCause::Completed
    );
}

#[test]
fn controller_stages_commits_and_checkpoints_the_actual_task_branch() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-commit");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    protected.attempt().wait_collect().unwrap();
    let worker = protected.worker_channel();
    std::fs::write(handle.worktree_path().join("payload.txt"), "edit").unwrap();
    std::fs::write(handle.worktree_path().join("*"), "literal").unwrap();

    // Operation-level refusal still consumes its sequence number.
    assert!(matches!(
        send(
            worker,
            0,
            ControllerOperation::Stage {
                paths: vec!["../x".into()]
            }
        ),
        Err(ProtectedLaunchError::Binding("invalid stage path"))
    ));
    let staged = send(
        worker,
        1,
        ControllerOperation::Stage {
            paths: vec!["payload.txt".into()],
        },
    )
    .unwrap();
    assert_eq!(
        staged,
        ControllerResponse::Staged {
            paths: vec!["payload.txt".into()]
        }
    );
    let ControllerResponse::Committed { commit } = send(
        worker,
        2,
        ControllerOperation::Commit {
            message: "worker edit".into(),
        },
    )
    .unwrap() else {
        panic!("commit response")
    };
    let branch_tip = stdout_trimmed(&git(
        repo.path(),
        &["rev-parse", "refs/heads/task/protected"],
    ));
    assert_eq!(commit.as_str(), branch_tip);
    // Literal pathspec: "*" was not staged by the earlier exact request.
    let tree = stdout_trimmed(&git(repo.path(), &["ls-tree", "--name-only", &branch_tip]));
    assert!(tree.lines().any(|l| l == "payload.txt") && !tree.lines().any(|l| l == "*"));

    let ControllerResponse::Checkpointed {
        handle: checkpointed,
        checkpoint,
    } = send(
        worker,
        3,
        ControllerOperation::Checkpoint {
            checkpoint_id: "cp".into(),
            kind: crate::WorkspaceCheckpointKind::Progress,
            captured_at: crate::WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
        },
    )
    .unwrap()
    else {
        panic!("checkpoint response")
    };
    assert_eq!(checkpoint.core().head_sha(), &commit);
    assert_eq!(checkpointed.state(), WorkspaceState::Checkpointed);
    assert_eq!(checkpointed.head_sha(), Some(&commit));
    assert_eq!(checkpoint.core().task_id(), Some("task-1"));
    assert_eq!(checkpoint.core().untracked_files(), &["*".to_string()]);

    // Switching the worktree off its task branch refuses further work.
    git(handle.worktree_path(), &["checkout", "--detach"]);
    assert!(matches!(
        send(worker, 4, ControllerOperation::Status),
        Err(ProtectedLaunchError::Custody(_))
    ));
}

#[test]
fn launch_refuses_requests_outside_the_admitted_worktree_or_launcher() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-launch-root");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    assert!(matches!(
        launcher.launch(admission, &request(repo.path()), &policy()),
        Err(ProtectedLaunchError::Binding(_))
    ));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let other = ProtectedLauncher::unenforced_for_test(config());
    assert!(matches!(
        other.launch(admission, &request(handle.worktree_path()), &policy()),
        Err(ProtectedLaunchError::Binding(
            "admission belongs to another launcher"
        ))
    ));
}

/// Bearer values never reach ordinary, pretty, nested or error diagnostics,
/// while the explicit protocol accessors keep returning them.
#[test]
fn controller_tokens_are_redacted_from_every_diagnostic_but_stay_accessible() {
    const MARKER: &str = "SYNTHETIC-BEARER-MARKER-7f3a";
    let marker_request = ControllerRequest {
        attempt_id: "attempt-1".into(),
        task_id: "task-1".into(),
        token: MARKER.into(),
        sequence: 3,
        operation: ControllerOperation::Identity,
    };
    let wrapped = (Some(marker_request.clone()), vec![marker_request.clone()]);
    for text in [
        format!("{marker_request:?}"),
        format!("{marker_request:#?}"),
        format!("{wrapped:?}"),
        format!("{wrapped:#?}"),
    ] {
        assert!(!text.contains(MARKER), "{text}");
        assert!(text.contains("<redacted>") && text.contains("attempt-1"));
    }
    // Explicit protocol access is unchanged.
    assert_eq!(marker_request.token, MARKER);
    assert_eq!(marker_request.clone(), marker_request);

    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-redact");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    let tokens = [
        protected.worker_channel().credentials().token,
        protected.reviewer_channel().credentials().token,
    ];
    assert_eq!(tokens[0].len(), 64);
    for text in [
        format!("{:?}", protected.worker_channel()),
        format!("{:#?}", protected.reviewer_channel()),
        format!("{protected:?}"),
        format!("{protected:#?}"),
        format!("{:?}", protected.worker_channel().credentials()),
        format!("{:#?}", protected.reviewer_channel().credentials()),
    ] {
        for token in &tokens {
            assert!(!text.contains(token.as_str()), "{text}");
        }
    }
    // Refusals triggered by a marker-bearing forged request echo nothing.
    let forged = ControllerRequest {
        token: MARKER.into(),
        ..marker_request
    };
    let refused = protected.worker_channel().handle(forged).unwrap_err();
    for text in [format!("{refused}"), format!("{refused:?}")] {
        assert!(!text.contains(MARKER), "{text}");
    }
    // The genuine credentials still drive the channel.
    assert!(matches!(
        send(protected.worker_channel(), 0, ControllerOperation::Identity),
        Ok(ControllerResponse::Identity { .. })
    ));
    protected.attempt().wait_collect().unwrap();
}

fn nested_repository(worktree: &Path, name: &str) {
    let nested = worktree.join(name);
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    git(
        &nested,
        &[
            "-c",
            "user.name=n",
            "-c",
            "user.email=n@n.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "nested",
        ],
    );
}

/// A tracked embedded repository would let worker-controlled config (for
/// example a clean filter) run as the controller on a later status/diff.
/// Staging one is refused, the index is restored and later work is unaffected.
#[test]
fn stage_refuses_embedded_repositories_and_restores_the_index() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-embedded");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    protected.attempt().wait_collect().unwrap();
    let worker = protected.worker_channel();
    let worktree = handle.worktree_path();
    std::fs::write(worktree.join("ok.txt"), "fine").unwrap();
    nested_repository(worktree, "evil");
    std::fs::create_dir(worktree.join("sub")).unwrap();
    nested_repository(worktree, "sub/inner");

    let staged_index = || stdout_trimmed(&git(worktree, &["ls-files", "--stage"]));
    assert!(matches!(
        send(
            worker,
            0,
            ControllerOperation::Stage {
                paths: vec!["ok.txt".into(), "evil".into()]
            }
        ),
        Err(ProtectedLaunchError::Binding(
            "embedded repositories cannot be staged"
        ))
    ));
    // Nothing from the refused request (not even ok.txt) stays staged.
    assert_eq!(staged_index(), "");
    // A request that reaches an embedded repository through its parent is refused too.
    assert!(matches!(
        send(
            worker,
            1,
            ControllerOperation::Stage {
                paths: vec!["sub".into()]
            }
        ),
        Err(ProtectedLaunchError::Binding(
            "embedded repositories cannot be staged"
        ))
    ));
    assert_eq!(staged_index(), "");
    assert!(
        !git(worktree, &["ls-files", "--stage"])
            .stdout
            .windows(6)
            .any(|window| window == b"160000")
    );
    // Ordinary staging and committing still work afterwards.
    send(
        worker,
        2,
        ControllerOperation::Stage {
            paths: vec!["ok.txt".into()],
        },
    )
    .unwrap();
    assert!(staged_index().contains("ok.txt"));
    let ControllerResponse::Committed { commit } = send(
        worker,
        3,
        ControllerOperation::Commit {
            message: "ok".into(),
        },
    )
    .unwrap() else {
        panic!("commit response")
    };
    let tree = stdout_trimmed(&git(worktree, &["ls-tree", "-r", commit.as_str()]));
    assert!(tree.contains("ok.txt") && !tree.contains("evil") && !tree.contains("160000"));
}

/// Commit, checkpoint, custody release and non-force teardown form one
/// verified sequence; release is refused until collection proved cleanup.
#[test]
fn release_requires_proven_cleanup_then_checkpointed_handle_tears_down_without_force() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-release");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    let worker = protected.worker_channel();
    let worktree = handle.worktree_path();

    // Cleanup not proven yet: custody stays.
    assert!(matches!(
        protected.release_custody(),
        Err(ProtectedLaunchError::CleanupUnknown(_))
    ));
    protected.attempt().wait_collect().unwrap();

    std::fs::write(worktree.join("work.txt"), "work").unwrap();
    send(
        worker,
        0,
        ControllerOperation::Stage {
            paths: vec!["work.txt".into()],
        },
    )
    .unwrap();
    let ControllerResponse::Committed { commit } = send(
        worker,
        1,
        ControllerOperation::Commit {
            message: "work".into(),
        },
    )
    .unwrap() else {
        panic!("commit response")
    };
    let ControllerResponse::Checkpointed {
        handle: checkpointed,
        ..
    } = send(
        worker,
        2,
        ControllerOperation::Checkpoint {
            checkpoint_id: "cp".into(),
            kind: crate::WorkspaceCheckpointKind::Progress,
            captured_at: crate::WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
        },
    )
    .unwrap()
    else {
        panic!("checkpoint response")
    };

    protected.release_custody().unwrap();
    // Single use, and both channels refuse afterwards.
    assert!(matches!(
        protected.release_custody(),
        Err(ProtectedLaunchError::Custody(_))
    ));
    for channel in [worker, protected.reviewer_channel()] {
        assert!(matches!(
            send(channel, 3, ControllerOperation::Status),
            Err(ProtectedLaunchError::Binding("custody was released"))
        ));
    }

    // Dirty payload refuses ordinary non-force teardown and survives.
    std::fs::write(worktree.join("late.txt"), "late").unwrap();
    let teardown = crate::WorkspaceTeardownRequest::new(repo.path());
    assert!(matches!(
        teardown.teardown(&checkpointed),
        Err(WorkspaceError::TeardownWorktreeDirty { .. })
    ));
    assert!(worktree.join("late.txt").exists());
    std::fs::remove_file(worktree.join("late.txt")).unwrap();
    let torn = teardown.teardown(&checkpointed).unwrap();
    assert_eq!(torn.state(), WorkspaceState::TornDown);
    assert!(!worktree.exists());
    assert_eq!(
        stdout_trimmed(&git(
            repo.path(),
            &["rev-parse", "refs/heads/task/protected"]
        )),
        commit.as_str(),
        "task branch and its commit are retained"
    );
}

// ------------------------------------------------------------ F1: release barrier

fn started(tag: &str) -> (TestRepo, WorkspaceHandle, ProtectedAttempt) {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new(tag);
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    protected.attempt().wait_collect().unwrap();
    (repo, handle, protected)
}

fn set_hooks(
    protected: &ProtectedAttempt,
    before: impl Fn(ControllerRole) + Send + Sync + 'static,
    after: impl Fn(ControllerRole) + Send + Sync + 'static,
) {
    let hooks = &protected.worker_channel().binding.hooks;
    *hooks.before.lock().unwrap() = Some(Arc::new(before));
    *hooks.after.lock().unwrap() = Some(Arc::new(after));
}

/// One-way latch with a bounded wait: a regression fails the test instead of
/// hanging it. Coordination is by this latch and the gate's phase condition,
/// never by sleeping for a guessed duration.
#[derive(Default)]
struct Latch(Mutex<bool>, std::sync::Condvar);

impl Latch {
    fn open(&self) {
        *self.0.lock().unwrap() = true;
        self.1.notify_all();
    }
    fn wait(&self) {
        let (open, timeout) = self
            .1
            .wait_timeout_while(self.0.lock().unwrap(), Duration::from_secs(20), |open| {
                !*open
            })
            .unwrap();
        assert!(*open && !timeout.timed_out(), "latch never opened");
    }
}

/// Parks the first `count` operations that reach the "before" point until
/// `proceed` opens, announcing each on `parked`.
fn park_first_operations(
    protected: &ProtectedAttempt,
    count: usize,
    parked: std::sync::mpsc::Sender<ControllerRole>,
    proceed: Arc<Latch>,
    after: impl Fn(ControllerRole) + Send + Sync + 'static,
) {
    let seen = AtomicUsize::new(0);
    let parked = Mutex::new(parked);
    set_hooks(
        protected,
        move |role| {
            if seen.fetch_add(1, Ordering::SeqCst) < count {
                parked.lock().unwrap().send(role).unwrap();
                proceed.wait();
            }
        },
        after,
    );
}

/// An operation already in flight on EACH channel holds the release back; the
/// release closes admission first (new requests on both channels refuse while
/// it waits) and restores custody only after both operations finished.
#[test]
fn release_waits_for_in_flight_operations_on_both_channels() {
    let (_repo, _handle, protected) = started("protected-barrier");
    let (worker, reviewer) = (protected.worker_channel(), protected.reviewer_channel());
    let (parked_tx, parked_rx) = std::sync::mpsc::channel();
    let proceed = Arc::new(Latch::default());
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    {
        let log = log.clone();
        park_first_operations(&protected, 2, parked_tx, proceed.clone(), move |_| {
            log.lock().unwrap().push("operation-complete");
        });
    }
    let gate = protected.worker_channel().binding.gate.clone();
    std::thread::scope(|scope| {
        let worker_op = scope.spawn(|| send(worker, 0, ControllerOperation::Status));
        let reviewer_op = scope.spawn(|| send(reviewer, 0, ControllerOperation::Status));
        let mut roles = [
            parked_rx.recv_timeout(Duration::from_secs(20)).unwrap(),
            parked_rx.recv_timeout(Duration::from_secs(20)).unwrap(),
        ];
        roles.sort_by_key(|role| *role == ControllerRole::Reviewer);
        assert_eq!(roles, [ControllerRole::Worker, ControllerRole::Reviewer]);
        let release = scope.spawn(|| {
            let result = protected.release_custody();
            log.lock().unwrap().push("release-returned");
            result
        });
        gate.wait_for_phase_for_test(Phase::Releasing);
        // New work on either channel refuses before executing.
        for channel in [worker, reviewer] {
            assert!(matches!(
                send(channel, 1, ControllerOperation::Status),
                Err(ProtectedLaunchError::Binding("custody release in progress"))
            ));
        }
        assert!(
            !release.is_finished(),
            "release overtook an in-flight operation"
        );
        assert_eq!(gate.phase_for_test(), Phase::Releasing);
        proceed.open();
        assert!(matches!(
            worker_op.join().unwrap(),
            Ok(ControllerResponse::Status { .. })
        ));
        assert!(matches!(
            reviewer_op.join().unwrap(),
            Ok(ControllerResponse::Status { .. })
        ));
        release.join().unwrap().unwrap();
    });
    assert_eq!(
        *log.lock().unwrap(),
        [
            "operation-complete",
            "operation-complete",
            "release-returned"
        ],
        "custody was restored before both operations completed"
    );
    assert_eq!(gate.phase_for_test(), Phase::Released);
    // Release first, request second: refuses and consumes nothing.
    for channel in [worker, reviewer] {
        assert!(matches!(
            send(channel, 1, ControllerOperation::Status),
            Err(ProtectedLaunchError::Binding("custody was released"))
        ));
    }
}

/// A mutating operation (worker stage) in flight is also drained, and a
/// second release attempt during the first is refused as such.
#[test]
fn release_drains_a_mutating_operation_and_refuses_a_concurrent_second_release() {
    let (_repo, handle, protected) = started("protected-barrier-stage");
    std::fs::write(handle.worktree_path().join("late.txt"), "late").unwrap();
    let (parked_tx, parked_rx) = std::sync::mpsc::channel();
    let proceed = Arc::new(Latch::default());
    park_first_operations(&protected, 1, parked_tx, proceed.clone(), |_| {});
    let gate = protected.worker_channel().binding.gate.clone();
    std::thread::scope(|scope| {
        let stage = scope.spawn(|| {
            send(
                protected.worker_channel(),
                0,
                ControllerOperation::Stage {
                    paths: vec!["late.txt".into()],
                },
            )
        });
        parked_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        let release = scope.spawn(|| protected.release_custody());
        gate.wait_for_phase_for_test(Phase::Releasing);
        assert!(matches!(
            protected.release_custody(),
            Err(ProtectedLaunchError::Custody(detail)) if detail.contains("already in progress")
        ));
        proceed.open();
        assert!(matches!(
            stage.join().unwrap(),
            Ok(ControllerResponse::Staged { .. })
        ));
        release.join().unwrap().unwrap();
    });
    // The staged work completed before custody moved.
    let staged = stdout_trimmed(&git(
        handle.worktree_path(),
        &["diff", "--cached", "--name-only"],
    ));
    assert_eq!(staged, "late.txt");
}

/// Failed or partial release: custody stays quarantined, the reservation record
/// survives (nothing looks free), channels refuse, a retry and a new admission
/// refuse; dropping the attempt does not free the repository either.
#[test]
fn failed_release_retains_custody_and_the_reservation_with_no_new_work() {
    let (repo, handle, protected) = started("protected-failed-release");
    protected
        .worker_channel()
        .binding
        .hooks
        .fail_restore
        .store(true, Ordering::SeqCst);
    let error = protected.release_custody().unwrap_err();
    assert!(
        matches!(&error, ProtectedLaunchError::CleanupUnknown(d) if d.contains("injected restoration failure")),
        "{error:?}"
    );
    let record = marker_text(&repo);
    assert!(record.contains("phase=incomplete"), "{record}");
    for channel in [protected.worker_channel(), protected.reviewer_channel()] {
        assert!(matches!(
            send(channel, 0, ControllerOperation::Status),
            Err(ProtectedLaunchError::Binding("custody release incomplete"))
        ));
    }
    assert!(matches!(
        protected.release_custody(),
        Err(ProtectedLaunchError::CleanupUnknown(_))
    ));
    let detail = custody_refusal(unenforced(62000..=62009).admit(&handle, repo.path()));
    assert!(detail.contains("phase incomplete"), "{detail}");
    drop(protected);
    assert_eq!(
        marker_text(&repo),
        record,
        "dropping the attempt freed nothing"
    );
    assert!(matches!(
        unenforced(62000..=62009).admit(&handle, repo.path()),
        Err(ProtectedLaunchError::Custody(_))
    ));
}

/// A confined controller run whose containment was not proven quarantines the
/// release; nothing is restored. Exercised through the real confined-run path
/// with a launch whose containment fails.
#[test]
fn unproven_controller_cleanup_quarantines_release() {
    struct Failing(Arc<Gate>);
    struct Unprovable;
    impl crate::execution::AttemptContainment for Unprovable {
        fn kill_and_verify_empty(&self) -> Result<(), String> {
            Err("injected: group not proven empty".into())
        }
    }
    impl git::ControllerLaunch for Failing {
        fn customization(&self) -> Result<crate::execution::LaunchCustomization, String> {
            Ok(crate::execution::LaunchCustomization {
                pre_exec: None,
                containment: Some(Arc::new(Unprovable)),
            })
        }
        fn cleanup_unproven(&self) {
            self.0.mark_controller_unproven();
        }
    }
    let (repo, _handle, protected) = started("protected-unproven");
    let binding = protected.worker_channel().binding.clone();
    {
        let _scope = git::ControllerScope::enter(Arc::new(Failing(binding.gate.clone())));
        assert!(git::capture(&binding.worktree, "unproven", &[OsStr::new("--version")]).is_err());
    }
    assert!(binding.gate.controller_unproven());
    let error = protected.release_custody().unwrap_err();
    assert!(
        matches!(&error, ProtectedLaunchError::CleanupUnknown(d) if d.contains("not proven")),
        "{error:?}"
    );
    assert!(marker_text(&repo).contains("phase=incomplete"));
    assert!(matches!(
        send(protected.worker_channel(), 0, ControllerOperation::Status),
        Err(ProtectedLaunchError::Binding("custody release incomplete"))
    ));
}

/// The valid flow is deadlock free: both channels are driven continuously while
/// release runs; every request is either completed or refused by the closed
/// barrier, release succeeds and nothing runs after it returned.
#[test]
fn concurrent_traffic_and_release_never_deadlock() {
    let (_repo, _handle, protected) = started("protected-traffic");
    let start = Barrier::new(3);
    let completed = AtomicUsize::new(0);
    let after_release = AtomicBool::new(false);
    let ran_after_release = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for channel in [protected.worker_channel(), protected.reviewer_channel()] {
            let (start, completed, after_release, ran_after) =
                (&start, &completed, &after_release, &ran_after_release);
            scope.spawn(move || {
                start.wait();
                let mut sequence = 0;
                loop {
                    match send(channel, sequence, ControllerOperation::Status) {
                        Ok(_) => {
                            if after_release.load(Ordering::SeqCst) {
                                ran_after.store(true, Ordering::SeqCst);
                            }
                            completed.fetch_add(1, Ordering::SeqCst);
                            sequence += 1;
                        }
                        Err(ProtectedLaunchError::Binding(reason)) => {
                            assert!(reason.starts_with("custody"), "unexpected refusal {reason}");
                            break;
                        }
                        Err(other) => panic!("{other:?}"),
                    }
                }
            });
        }
        start.wait();
        // Let at least one operation complete before closing the barrier.
        let waiting = std::time::Instant::now();
        while completed.load(Ordering::SeqCst) == 0 {
            assert!(
                waiting.elapsed() < Duration::from_secs(20),
                "no operation completed"
            );
            std::thread::yield_now();
        }
        protected.release_custody().unwrap();
        after_release.store(true, Ordering::SeqCst);
    });
    assert!(completed.load(Ordering::SeqCst) >= 1);
    assert!(
        !ran_after_release.load(Ordering::SeqCst),
        "an operation completed after custody was returned"
    );
}

/// Dropping an unused admission, and failed launch checks, give custody back;
/// a failure after a worker may exist keeps the record.
#[test]
fn unused_and_misdirected_admissions_release_while_unknown_ones_stay_retained() {
    let launcher = unenforced(62000..=62009);
    let repo = TestRepo::new("protected-unused");
    let handle = provisioned(&repo, Some("task-1"));
    drop(launcher.admit(&handle, repo.path()).unwrap());
    assert!(
        !marker_path(&repo).exists(),
        "unused admission was not released"
    );

    let admission = launcher.admit(&handle, repo.path()).unwrap();
    assert!(matches!(
        launcher.launch(admission, &request(repo.path()), &policy()),
        Err(ProtectedLaunchError::Binding(_))
    ));
    assert!(
        !marker_path(&repo).exists(),
        "misdirected launch kept custody"
    );

    // If giving custody back fails, the refusal says so and the record stays.
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    admission
        .binding
        .hooks
        .fail_restore
        .store(true, Ordering::SeqCst);
    let error = launcher
        .launch(admission, &request(repo.path()), &policy())
        .unwrap_err();
    assert!(
        matches!(&error, ProtectedLaunchError::CleanupUnknown(d) if d.contains("also failed")),
        "{error:?}"
    );
    assert!(marker_text(&repo).contains("phase=incomplete"));
}

// -------------------------------------------- F5: filter marker attribution

fn arm_clean_filter(nested: &Path, marker: &Path) {
    std::fs::write(nested.join(".gitattributes"), "* filter=x\n").unwrap();
    git(
        nested,
        &[
            "config",
            "filter.x.clean",
            &format!("sh -c 'touch {}; cat'", marker.display()),
        ],
    );
}

fn commit_all(nested: &Path) {
    git(nested, &["add", "-A"]);
    git(
        nested,
        &[
            "-c",
            "user.name=w",
            "-c",
            "user.email=w@w.invalid",
            "commit",
            "-q",
            "-m",
            "w",
        ],
    );
}

/// The retained finding: a fixture that installs the filter before preparing
/// the embedded repository runs the filter itself, so a marker at the
/// controller baseline proves nothing about the controller.
#[test]
fn filter_installed_before_preparation_creates_a_worker_marker_before_any_controller() {
    let repo = TestRepo::new("protected-filter-old-order");
    let handle = provisioned(&repo, Some("task-1"));
    let nested = handle.worktree_path().join("evil");
    let marker = handle.worktree_path().join("FILTER_RAN");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    arm_clean_filter(&nested, &marker);
    std::fs::write(nested.join("f"), "data").unwrap();
    commit_all(&nested);
    assert!(marker.exists(), "worker preparation ran its own filter");
}

/// F5: worker preparation (init/commit before the filter exists), arming, a
/// positive control (the unsafe Git path runs the marker as the worker) and a
/// reset establish a baseline; afterwards every controller operation leaves the
/// marker absent, so a marker could only have come from controller execution.
#[test]
fn filter_marker_distinguishes_worker_preparation_from_controller_execution() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("protected-filter-phases");
    let handle = provisioned(&repo, Some("task-1"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    protected.attempt().wait_collect().unwrap();
    let worker = protected.worker_channel();
    let worktree = handle.worktree_path();
    let marker = worktree.join("FILTER_RAN");
    let nested = worktree.join("evil");

    // 1. Worker preparation: no filter exists yet, so nothing can run it.
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    std::fs::write(nested.join("f"), "data").unwrap();
    commit_all(&nested);
    assert!(!marker.exists(), "preparation must not run the filter");
    // 2. Arm the malicious filter.
    arm_clean_filter(&nested, &marker);
    assert!(!marker.exists(), "arming must not run the filter");
    // 3. Positive control: the attempted unsafe Git path really runs it.
    std::fs::write(nested.join("f"), "changed").unwrap();
    git(&nested, &["add", "f"]);
    assert!(
        marker.exists(),
        "control: Git inside the repo runs the filter"
    );
    std::fs::remove_file(&marker).unwrap();

    // Controller phase: the protected path refuses without running anything.
    std::fs::write(worktree.join("ok.txt"), "fine").unwrap();
    assert!(matches!(
        send(
            worker,
            0,
            ControllerOperation::Stage {
                paths: vec!["ok.txt".into(), "evil".into()]
            }
        ),
        Err(ProtectedLaunchError::Binding(
            "embedded repositories cannot be staged"
        ))
    ));
    assert!(!marker.exists(), "refusal executed the worker filter");
    assert!(matches!(
        send(worker, 1, ControllerOperation::Status),
        Ok(ControllerResponse::Status { .. })
    ));
    assert!(!marker.exists(), "status executed the worker filter");
    // Ordinary stage, commit and checkpoint keep working.
    send(
        worker,
        2,
        ControllerOperation::Stage {
            paths: vec!["ok.txt".into()],
        },
    )
    .unwrap();
    assert!(matches!(
        send(
            worker,
            3,
            ControllerOperation::Commit {
                message: "ok".into()
            }
        ),
        Ok(ControllerResponse::Committed { .. })
    ));
    assert!(matches!(
        send(
            worker,
            4,
            ControllerOperation::Checkpoint {
                checkpoint_id: "cp".into(),
                kind: crate::WorkspaceCheckpointKind::Progress,
                captured_at: crate::WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
            }
        ),
        Ok(ControllerResponse::Checkpointed { .. })
    ));
    assert!(
        !marker.exists(),
        "a controller operation ran the worker filter"
    );
    let tree = stdout_trimmed(&git(worktree, &["ls-tree", "-r", "HEAD"]));
    assert!(tree.contains("ok.txt") && !tree.contains("evil"), "{tree}");
}

#[test]
fn controller_refuses_effective_target_configuration_for_every_content_operation() {
    let launcher = ProtectedLauncher::unenforced_for_test(config());
    let repo = TestRepo::new("controller-target-config");
    repo.commit_file("f", "original\n");
    let handle = provisioned(&repo, Some("target-config"));
    let admission = launcher.admit(&handle, repo.path()).unwrap();
    let protected = launcher
        .launch(admission, &request(handle.worktree_path()), &policy())
        .unwrap();
    protected.attempt().wait_collect().unwrap();
    // This test seam models a preexisting target-only configuration. Production
    // metadata remains in custody through each confined child completion.
    git(
        repo.path(),
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        handle.worktree_path(),
        &[
            "config",
            "--worktree",
            "filter.x.clean",
            "/nonexistent/filter",
        ],
    );
    for (sequence, operation) in [
        ControllerOperation::Status,
        ControllerOperation::Stage {
            paths: vec!["f".into()],
        },
        ControllerOperation::Commit {
            message: "refused".into(),
        },
        ControllerOperation::Checkpoint {
            checkpoint_id: "refused".into(),
            kind: crate::WorkspaceCheckpointKind::Progress,
            captured_at: crate::WorkspaceDateTimeV1::try_new("2026-11-03T00:00:00Z").unwrap(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            send(protected.worker_channel(), sequence as u64, operation),
            Err(ProtectedLaunchError::Workspace(
                WorkspaceError::RepositoryConfigRefused { .. }
            ))
        ));
    }
    protected.release_custody().unwrap();
}

// ------------------------------------------- FRESH-1: controller cgroup names

fn ctl_index(dir: &Path, attempt: &str) -> u64 {
    dir.file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix(&format!("receipts-{attempt}-ctl-"))
        .unwrap()
        .parse()
        .unwrap()
}

/// FRESH-1: worker and reviewer operations run on separate channels, each
/// creating its own controller launch per request. The first Git call of each
/// channel is held until the other channel's first reservation exists, so two
/// reservations are live at once; every name stays distinct and indices keep
/// increasing across later requests. The seam reserves names in a temporary
/// directory through the production allocator only: no kernel cgroup exists.
#[test]
fn overlapping_channel_operations_reserve_distinct_controller_cgroups() {
    let (_repo, _handle, protected) = started("protected-cgroup-names");
    let (worker, reviewer) = (protected.worker_channel(), protected.reviewer_channel());
    let attempt = worker.credentials().attempt_id;
    let root = crate::test_support::TempDir::new("cgroup-seam");
    let names: Arc<Mutex<Vec<(ControllerRole, PathBuf)>>> = Arc::default();
    let overlap: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let ready = Arc::new([Latch::default(), Latch::default()]);
    let slot = |role: ControllerRole| usize::from(role == ControllerRole::Reviewer);
    let seam = CgroupNameSeam {
        root: root.path().to_path_buf(),
        reserved: Arc::new({
            let (names, overlap, ready) = (names.clone(), overlap.clone(), ready.clone());
            let root = root.path().to_path_buf();
            move |role, dir| {
                let first = {
                    let mut names = names.lock().unwrap();
                    let first = !names.iter().any(|(seen, _)| *seen == role);
                    names.push((role, dir.to_path_buf()));
                    first
                };
                if first {
                    ready[slot(role)].open();
                    ready[1 - slot(role)].wait();
                    overlap.lock().unwrap().push(listing(&root));
                }
            }
        }),
    };
    // An operation that ends without reserving releases the other channel.
    {
        let ready = ready.clone();
        set_hooks(&protected, |_| {}, move |role| ready[slot(role)].open());
    }
    *worker.binding.hooks.controller_cgroups.lock().unwrap() = Some(seam);

    let (worked, reviewed) = std::thread::scope(|scope| {
        let worked = scope.spawn(|| send(worker, 0, ControllerOperation::Status));
        let reviewed = scope.spawn(|| send(reviewer, 0, ControllerOperation::Status));
        (worked.join().unwrap(), reviewed.join().unwrap())
    });
    let report = format!("{worked:?}\n{reviewed:?}\n{:?}", names.lock().unwrap());
    assert!(
        matches!(worked, Ok(ControllerResponse::Status { .. })),
        "{report}"
    );
    assert!(
        matches!(reviewed, Ok(ControllerResponse::Status { .. })),
        "{report}"
    );
    let first = |role| {
        names
            .lock()
            .unwrap()
            .iter()
            .find(|(seen, _)| *seen == role)
            .map(|(_, dir)| dir.file_name().unwrap().to_string_lossy().into_owned())
            .unwrap()
    };
    let (worker_first, reviewer_first) = (
        first(ControllerRole::Worker),
        first(ControllerRole::Reviewer),
    );
    assert_ne!(worker_first, reviewer_first);
    for snapshot in overlap.lock().unwrap().iter() {
        assert!(
            snapshot.contains(&worker_first) && snapshot.contains(&reviewer_first),
            "both reservations were not live at once: {snapshot:?}"
        );
    }
    assert_eq!(overlap.lock().unwrap().len(), 2);
    let first_round = names.lock().unwrap().len();

    // Later requests (several Git calls each) continue the same sequence.
    send(worker, 1, ControllerOperation::Status).unwrap();
    send(reviewer, 1, ControllerOperation::Status).unwrap();
    let names = names.lock().unwrap();
    assert!(names.len() > first_round);
    let mut indices: Vec<u64> = names
        .iter()
        .map(|(_, dir)| ctl_index(dir, &attempt))
        .collect();
    let in_order = indices.clone();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(
        indices,
        (0..names.len() as u64).collect::<Vec<_>>(),
        "{names:?}"
    );
    assert!(
        in_order[first_round..]
            .iter()
            .all(|index| *index >= first_round as u64),
        "a later request reused an earlier index: {in_order:?}"
    );
    // Every reservation's containment removed exactly its own directory.
    assert_eq!(listing(root.path()), Vec::<String>::new());
    drop(names);
    protected.release_custody().unwrap();
}

/// The allocator consumes an index even when creation fails, never adopts or
/// cleans another directory, never reuses a retained or removed name and
/// refuses at exhaustion instead of wrapping.
#[test]
fn controller_cgroup_indices_are_consumed_on_failure_never_reused_and_never_wrap() {
    let root = crate::test_support::TempDir::new("cgroup-alloc");
    let name = |index: u64| root.path().join(format!("receipts-a1-ctl-{index}"));
    let gate = Gate::new();
    // Another live or retained cgroup already holds index 1.
    std::fs::create_dir(name(1)).unwrap();
    std::fs::write(name(1).join("owned-elsewhere"), "x").unwrap();

    assert_eq!(
        gate.create_controller_cgroup(root.path(), "a1").unwrap(),
        name(0)
    );
    let refused = gate
        .create_controller_cgroup(root.path(), "a1")
        .unwrap_err();
    assert!(refused.contains("receipts-a1-ctl-1"), "{refused}");
    assert!(
        name(1).join("owned-elsewhere").exists(),
        "collision cleaned a directory"
    );
    // ctl-0 is retained (unproven cleanup) and ctl-2 is removed: neither returns.
    assert_eq!(
        gate.create_controller_cgroup(root.path(), "a1").unwrap(),
        name(2)
    );
    std::fs::remove_dir(name(2)).unwrap();
    assert_eq!(
        gate.create_controller_cgroup(root.path(), "a1").unwrap(),
        name(3)
    );

    gate.controller_cgroups
        .store(u64::MAX - 1, Ordering::SeqCst);
    assert_eq!(
        gate.create_controller_cgroup(root.path(), "a1").unwrap(),
        name(u64::MAX - 1)
    );
    let before = listing(root.path());
    for _ in 0..2 {
        let exhausted = gate
            .create_controller_cgroup(root.path(), "a1")
            .unwrap_err();
        assert!(exhausted.contains("exhausted"), "{exhausted}");
    }
    assert_eq!(
        listing(root.path()),
        before,
        "exhaustion created or reused a name"
    );
    assert_eq!(gate.controller_cgroups.load(Ordering::SeqCst), u64::MAX);
}
