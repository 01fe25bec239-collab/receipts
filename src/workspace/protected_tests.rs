//! Platform-neutral protected-launch logic. The `unenforced_for_test` seam
//! exercises admission binding and channel semantics WITHOUT enforcement;
//! these tests prove no Linux confinement property.

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

#[test]
fn identities_are_noncolliding_and_never_reused() {
    let launcher = ProtectedLauncher::unenforced_for_test(
        ProtectedLaunchConfig::new(61001, 61001, 61002, 62000..=62001, "/c").unwrap(),
    );
    let repo = TestRepo::new("protected-ids");
    let handle = provisioned(&repo, Some("task-1"));
    let first = launcher.admit(&handle, repo.path()).unwrap();
    let second = launcher.admit(&handle, repo.path()).unwrap();
    assert_ne!(first.binding.worker_uid, second.binding.worker_uid);
    assert_ne!(first.attempt_id(), second.attempt_id());
    drop((first, second));
    assert!(matches!(
        launcher.admit(&handle, repo.path()),
        Err(ProtectedLaunchError::IdentityExhausted)
    ));
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
