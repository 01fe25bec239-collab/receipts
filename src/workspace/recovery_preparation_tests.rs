use crate::test_support::{TestRepo, git};
use crate::*;
use WorkspaceRecoveryPreparationError as E;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Fixture {
    repo: TestRepo,
    handle: WorkspaceHandle,
    checkpoint: WorkspaceCheckpointCaptureCore,
}
impl Fixture {
    fn new(task: Option<&str>) -> Self {
        let repo = TestRepo::new_nested("recovery-preparation", "repo");
        repo.commit_file("tracked", "original");
        repo.commit_file("unstaged-only", "original");
        let root = fs::canonicalize(repo.root.path()).unwrap();
        let handle = WorkspaceProvisionRequest::new(
            repo.path(),
            "workspace",
            task,
            "task-branch",
            root.join("target"),
            &repo.head_sha(),
        )
        .unwrap()
        .provision()
        .unwrap();
        let checkpoint = checkpoint(&handle, "workspace", task, handle.base_sha().clone(), None);
        Self {
            repo,
            handle,
            checkpoint,
        }
    }
    fn request(
        &self,
        decision: WorkspaceRecoveryDecision,
    ) -> WorkspaceRecoveryPreparationRequest<'_> {
        WorkspaceRecoveryPreparationRequest {
            handle: &self.handle,
            checkpoint: &self.checkpoint,
            decision,
            pre_recovery_checkpoint_id: "fresh".into(),
            attempt_id: Some("replacement-attempt".into()),
            last_accepted_sha: None,
        }
    }
    fn root(&self) -> &Path {
        self.handle.worktree_path()
    }
    fn dirty(&self) {
        fs::write(self.root().join("tracked"), b"staged\0bytes").unwrap();
        git(self.root(), &["add", "tracked"]);
        fs::write(self.root().join("tracked"), b"unstaged\0bytes").unwrap();
        fs::write(self.root().join("untracked"), b"new\xffbytes").unwrap();
        fs::write(self.root().join("unstaged-only"), "unstaged only").unwrap();
        fs::write(self.root().join("staged-only"), "staged only").unwrap();
        git(self.root(), &["add", "staged-only"]);
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        snapshot(self.repo.root.path())
    }
}
fn checkpoint(
    handle: &WorkspaceHandle,
    workspace: &str,
    task: Option<&str>,
    head: CommitSha,
    base: Option<CommitSha>,
) -> WorkspaceCheckpointCaptureCore {
    WorkspaceCheckpointCaptureCore::new(
        "selected",
        workspace,
        task.map(str::to_owned),
        Some("crashed-attempt".into()),
        WorkspaceCheckpointKind::Progress,
        head,
        base,
        None,
        vec![],
        vec![],
        vec![],
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("{}: {e}", handle.workspace_id()))
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let bytes = if kind.is_symlink() {
                fs::read_link(&path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec()
            } else if kind.is_dir() {
                visit(root, &path, entries);
                vec![]
            } else {
                fs::read(&path).unwrap()
            };
            entries.push((path.strip_prefix(root).unwrap().into(), bytes));
        }
    }
    let mut entries = vec![];
    visit(root, root, &mut entries);
    entries.sort();
    entries
}
fn rejected(f: &Fixture, request: WorkspaceRecoveryPreparationRequest<'_>) -> E {
    let before = f.snapshot();
    let error = prepare_workspace_checkpoint_recovery(request).unwrap_err();
    assert_eq!(f.snapshot(), before);
    error
}

#[test]
fn recovery_preparation_all_decisions_preserve_dirty_index_refs_and_untracked_bytes() {
    let f = Fixture::new(Some("task"));
    f.dirty();
    for decision in WorkspaceRecoveryDecision::ALL {
        let before = f.snapshot();
        let mut request = f.request(decision);
        if decision == WorkspaceRecoveryDecision::ResetToLastAccepted {
            request.last_accepted_sha = Some(f.handle.base_sha().as_str());
        }
        let result = prepare_workspace_checkpoint_recovery(request).unwrap();
        assert_eq!(result.handle(), &f.handle);
        assert_eq!(result.checkpoint(), &f.checkpoint);
        assert_eq!(result.canonical_target(), f.root());
        assert_eq!(result.decision(), decision);
        assert_eq!(result.current_head(), f.handle.base_sha());
        assert_eq!(
            result.last_accepted_sha().is_some(),
            decision == WorkspaceRecoveryDecision::ResetToLastAccepted
        );
        let fresh = result.pre_recovery_capture();
        assert_eq!(fresh.kind(), WorkspaceCheckpointKind::RecoveryCapture);
        assert_eq!(fresh.checkpoint_id(), "fresh");
        assert_eq!(fresh.workspace_id(), "workspace");
        assert_eq!(fresh.task_id(), Some("task"));
        assert_eq!(fresh.attempt_id(), Some("replacement-attempt"));
        assert_eq!(
            fresh.modified_files(),
            &["staged-only", "tracked", "unstaged-only"]
        );
        assert_eq!(fresh.untracked_files(), &["untracked"]);
        assert_eq!(fresh.recovery_decision(), None);
        assert!(fresh.dirty_diff_ref().is_none());
        assert!(fresh.executed_checks().is_empty());
        assert_eq!(
            f.snapshot(),
            before,
            "decision {decision:?}: includes index, all refs, files and Git admin bytes"
        );
        assert_eq!(
            git(f.root(), &["show", ":tracked"]).stdout,
            b"staged\0bytes"
        );
    }
}

#[test]
fn recovery_preparation_exact_workspace_and_task_option_identity() {
    for task in [None, Some("task")] {
        let f = Fixture::new(task);
        for (workspace, checkpoint_task) in [
            ("wrong", task),
            ("workspace", Some("wrong")),
            (
                "workspace",
                if task.is_some() { None } else { Some("task") },
            ),
        ] {
            let core = checkpoint(
                &f.handle,
                workspace,
                checkpoint_task,
                f.handle.base_sha().clone(),
                None,
            );
            let mut request = f.request(WorkspaceRecoveryDecision::InspectAndSalvage);
            request.checkpoint = &core;
            let error = rejected(&f, request);
            assert!(matches!(error, E::WorkspaceMismatch | E::TaskMismatch));
        }
        assert!(
            prepare_workspace_checkpoint_recovery(
                f.request(WorkspaceRecoveryDecision::InspectAndSalvage)
            )
            .is_ok()
        );
    }
}

#[test]
fn recovery_preparation_wrong_branch_and_detached_head_fail_closed() {
    for detached in [false, true] {
        let f = Fixture::new(Some("task"));
        if detached {
            git(f.root(), &["checkout", "--detach", "--quiet"]);
        } else {
            git(f.root(), &["checkout", "--quiet", "-b", "wrong"]);
        }
        assert!(matches!(
            rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
            E::BranchMismatch
                | E::Evidence(WorkspaceCheckpointEvidenceCaptureError::GitCommandFailed { .. })
        ));
    }
}

#[test]
fn recovery_preparation_missing_checkpoint_head_and_base_fail_closed() {
    for missing_head in [true, false] {
        let f = Fixture::new(Some("task"));
        let missing = CommitSha::parse(&"0".repeat(40)).unwrap();
        let core = checkpoint(
            &f.handle,
            "workspace",
            Some("task"),
            if missing_head {
                missing.clone()
            } else {
                f.handle.base_sha().clone()
            },
            if missing_head { None } else { Some(missing) },
        );
        let mut request = f.request(WorkspaceRecoveryDecision::ContinueFromCheckpoint);
        request.checkpoint = &core;
        assert!(matches!(
            rejected(&f, request),
            E::Evidence(WorkspaceCheckpointEvidenceCaptureError::GitCommandFailed { .. })
        ));
    }
}

#[test]
fn recovery_preparation_reset_requires_exact_exclusive_local_authority() {
    let f = Fixture::new(None);
    assert!(matches!(
        rejected(
            &f,
            f.request(WorkspaceRecoveryDecision::ResetToLastAccepted)
        ),
        E::MissingResetTarget
    ));
    for sha in [
        "",
        "HEAD",
        "abc123",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "0000000000000000000000000000000000000000\n",
    ] {
        let mut request = f.request(WorkspaceRecoveryDecision::ResetToLastAccepted);
        request.last_accepted_sha = Some(sha);
        assert!(matches!(rejected(&f, request), E::InvalidResetTarget));
    }
    let mut request = f.request(WorkspaceRecoveryDecision::ResetToLastAccepted);
    request.last_accepted_sha = Some("0000000000000000000000000000000000000000");
    assert!(matches!(rejected(&f, request), E::Evidence(_)));
    for decision in [
        WorkspaceRecoveryDecision::ContinueFromCheckpoint,
        WorkspaceRecoveryDecision::InspectAndSalvage,
    ] {
        let mut request = f.request(decision);
        request.last_accepted_sha = Some(f.handle.base_sha().as_str());
        assert!(matches!(rejected(&f, request), E::ExtraneousResetTarget));
    }
}

#[test]
fn recovery_preparation_older_checkpoint_and_unrelated_reset_commit_need_no_ancestry() {
    let f = Fixture::new(None);
    git(
        f.root(),
        &["commit", "--quiet", "--allow-empty", "-m", "later"],
    );
    // A root commit with the same tree is valid exact caller evidence without ancestry.
    let tree = String::from_utf8(git(f.root(), &["rev-parse", "HEAD^{tree}"]).stdout).unwrap();
    let unrelated =
        String::from_utf8(git(f.root(), &["commit-tree", tree.trim(), "-m", "unrelated"]).stdout)
            .unwrap();
    let unrelated = unrelated.trim();
    f.dirty();
    let mut request = f.request(WorkspaceRecoveryDecision::ResetToLastAccepted);
    request.last_accepted_sha = Some(unrelated);
    let before = f.snapshot();
    let result = prepare_workspace_checkpoint_recovery(request).unwrap();
    assert_ne!(result.current_head(), f.checkpoint.head_sha());
    assert_eq!(result.last_accepted_sha().unwrap().as_str(), unrelated);
    assert_eq!(f.snapshot(), before);
}

#[test]
fn recovery_preparation_tag_object_is_not_a_commit() {
    let f = Fixture::new(None);
    git(f.root(), &["tag", "-a", "evidence-tag", "-m", "tag"]);
    let tag = String::from_utf8(git(f.root(), &["rev-parse", "evidence-tag"]).stdout).unwrap();
    let core = checkpoint(
        &f.handle,
        "workspace",
        None,
        CommitSha::parse(tag.trim()).unwrap(),
        None,
    );
    let mut request = f.request(WorkspaceRecoveryDecision::InspectAndSalvage);
    request.checkpoint = &core;
    assert!(matches!(rejected(&f, request), E::CommitNotCommit));
}

#[test]
fn recovery_preparation_paths_symlinks_refs_and_commands_are_inert_without_network() {
    let f = Fixture::new(Some("task"));
    f.dirty();
    let outside = f.repo.root.path().join("outside");
    fs::write(&outside, "outside secret").unwrap();
    std::os::unix::fs::symlink(&outside, f.root().join("outside-link")).unwrap();
    let marker = f.repo.root.path().join("must-not-exist");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/must-not-read", listener.local_addr().unwrap());
    for kind in WorkspaceCheckpointRefType::ALL {
        let reference = WorkspaceCheckpointRef::new(kind, &url, None, None).unwrap();
        let checks = vec![
            WorkspaceCheckpointExecutedCheckCore::new(
                WorkspaceCheckpointCheckSource::WorkerExecution,
                vec!["/usr/bin/touch".into(), marker.to_str().unwrap().into()],
                0,
                f.handle.base_sha().clone(),
                None,
                Some(reference.clone()),
            )
            .unwrap(),
        ];
        let paths = vec![
            "../outside".into(),
            "../../secret".into(),
            outside.to_str().unwrap().into(),
            "/absolute/path".into(),
            "subdir/../../outside".into(),
            "outside-link".into(),
        ];
        let core = WorkspaceCheckpointCaptureCore::new(
            "selected",
            "workspace",
            Some("task".into()),
            None,
            WorkspaceCheckpointKind::PreTermination,
            f.handle.base_sha().clone(),
            Some(f.handle.base_sha().clone()),
            Some(reference),
            paths.clone(),
            paths,
            checks,
            Some(WorkspaceRecoveryDecision::ResetToLastAccepted),
            Some("old choice".into()),
        )
        .unwrap();
        for decision in WorkspaceRecoveryDecision::ALL {
            let before = f.snapshot();
            let mut request = f.request(decision);
            request.checkpoint = &core;
            if decision == WorkspaceRecoveryDecision::ResetToLastAccepted {
                request.last_accepted_sha = Some(f.handle.base_sha().as_str());
            }
            let result = prepare_workspace_checkpoint_recovery(request).unwrap();
            assert_eq!(result.checkpoint(), &core);
            assert_eq!(result.decision(), decision);
            assert!(!marker.exists());
            assert_eq!(f.snapshot(), before);
        }
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn recovery_preparation_sibling_symlink_substitution_and_missing_target_fail_closed() {
    for symlink in [true, false] {
        let f = Fixture::new(None);
        let sibling = fs::canonicalize(f.repo.root.path())
            .unwrap()
            .join("sibling");
        git(
            f.repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "sibling-branch",
                sibling.to_str().unwrap(),
            ],
        );
        let old = f.root().with_file_name("retained-target");
        fs::rename(f.root(), old).unwrap();
        if symlink {
            std::os::unix::fs::symlink(sibling, f.root()).unwrap();
        }
        assert!(matches!(
            rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
            E::InvalidTargetPath | E::TargetIo(_)
        ));
    }
}

#[test]
fn recovery_preparation_sibling_gitfile_and_nested_root_confusion_fail_closed() {
    let f = Fixture::new(None);
    let sibling = fs::canonicalize(f.repo.root.path())
        .unwrap()
        .join("sibling");
    git(
        f.repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "sibling-branch",
            sibling.to_str().unwrap(),
        ],
    );
    fs::copy(sibling.join(".git"), f.root().join(".git")).unwrap();
    assert!(matches!(
        rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
        E::BranchMismatch
    ));

    let f = Fixture::new(None);
    // A retained target now inside a parent repository must not be admitted
    // merely because Git discovery still finds a repository and valid HEAD.
    fs::rename(
        f.root().join(".git"),
        f.repo.root.path().join("retained-gitfile"),
    )
    .unwrap();
    git(
        f.repo.root.path(),
        &["init", "--quiet", "--initial-branch=task-branch"],
    );
    assert!(matches!(
        rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
        E::WorktreeRootMismatch
    ));
}

#[test]
fn recovery_preparation_lexical_parent_and_symlink_alias_handles_fail_closed() {
    let f = Fixture::new(None);
    fs::create_dir(f.root().join("subdir")).unwrap();
    let alias = f.root().with_file_name("alias");
    std::os::unix::fs::symlink(f.root(), &alias).unwrap();
    for path in [alias, f.root().join("subdir/..")] {
        let handle = WorkspaceHandle::provisioned(
            "workspace".into(),
            None,
            "task-branch".into(),
            path.into_boxed_path(),
            f.handle.base_sha().clone(),
            None,
        );
        let mut request = f.request(WorkspaceRecoveryDecision::InspectAndSalvage);
        request.handle = &handle;
        assert!(matches!(rejected(&f, request), E::InvalidTargetPath));
    }
}

#[test]
fn recovery_preparation_identical_reconstruction_is_current_observation_only_and_read_only() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new(None);
    f.dirty();
    let old = f.root().with_file_name("original-retained");
    let inode = fs::metadata(f.root()).unwrap().ino();
    fs::rename(f.root(), &old).unwrap();
    fs::create_dir(f.root()).unwrap();
    for entry in fs::read_dir(&old).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), f.root().join(entry.file_name())).unwrap();
    }
    assert_ne!(fs::metadata(f.root()).unwrap().ino(), inode);
    for decision in WorkspaceRecoveryDecision::ALL {
        let before = f.snapshot();
        let mut request = f.request(decision);
        if decision == WorkspaceRecoveryDecision::ResetToLastAccepted {
            request.last_accepted_sha = Some(f.handle.base_sha().as_str());
        }
        let result = prepare_workspace_checkpoint_recovery(request).unwrap();
        assert_eq!(result.canonical_target(), f.root());
        assert_eq!(result.current_head(), f.handle.base_sha());
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn recovery_preparation_capture_path_overflow_propagates_without_partial_result() {
    let f = Fixture::new(None);
    let count = execution::STREAM_CAPTURE_LIMIT_BYTES as usize / 241 + 1;
    for i in 0..count {
        fs::write(f.root().join(format!("{i:08}{}", "x".repeat(232))), "bytes").unwrap();
    }
    let error = rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage));
    assert!(
        matches!(error, E::Evidence(WorkspaceCheckpointEvidenceCaptureError::GitEvidenceTooLarge {
        observation: WorkspaceCheckpointGitObservation::UntrackedPaths, limit_bytes, ..
    }) if limit_bytes == execution::STREAM_CAPTURE_LIMIT_BYTES)
    );
}

#[test]
fn recovery_preparation_invalid_utf8_path_and_capture_metadata_fail_closed() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new(None);
    // Put raw bytes directly in Git's index: APFS rejects such filesystem
    // names, but Git can still carry them as staged evidence.
    let path = std::ffi::OsString::from_vec(vec![0xff]);
    let blob = String::from_utf8(git(f.root(), &["rev-parse", "HEAD:tracked"]).stdout).unwrap();
    let mut command = crate::git::prepared_command(f.root(), "invalid path fixture", &[]).unwrap();
    assert!(
        command
            .args([
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                blob.trim()
            ])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(matches!(
        rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
        E::Evidence(WorkspaceCheckpointEvidenceCaptureError::InvalidUtf8 { .. })
    ));
    let mut command =
        crate::git::prepared_command(f.root(), "remove invalid fixture path", &[]).unwrap();
    assert!(
        command
            .args(["update-index", "--force-remove"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut request = f.request(WorkspaceRecoveryDecision::InspectAndSalvage);
    request.pre_recovery_checkpoint_id.clear();
    assert!(matches!(
        rejected(&f, request),
        E::Evidence(WorkspaceCheckpointEvidenceCaptureError::Core(_))
    ));
}

#[test]
fn recovery_preparation_missing_promisor_commit_never_fetches_or_mutates() {
    let f = Fixture::new(None);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/repo", listener.local_addr().unwrap());
    git(f.root(), &["config", "remote.origin.url", &url]);
    git(f.root(), &["config", "remote.origin.promisor", "true"]);
    git(
        f.root(),
        &["config", "remote.origin.partialclonefilter", "blob:none"],
    );
    // A protocol-specific allow overrides protocol.allow=never. Lazy object
    // fetching must be disabled independently of transport policy.
    git(f.root(), &["config", "protocol.http.allow", "always"]);
    let core = checkpoint(
        &f.handle,
        "workspace",
        None,
        CommitSha::parse(&"0".repeat(40)).unwrap(),
        None,
    );
    let mut request = f.request(WorkspaceRecoveryDecision::InspectAndSalvage);
    request.checkpoint = &core;
    assert!(matches!(rejected(&f, request), E::Evidence(_)));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn recovery_preparation_missing_or_noncommit_current_head_fails_closed() {
    for missing in [true, false] {
        let f = Fixture::new(None);
        let sha = if missing {
            "0".repeat(40)
        } else {
            String::from_utf8(git(f.root(), &["rev-parse", "HEAD:tracked"]).stdout)
                .unwrap()
                .trim()
                .into()
        };
        fs::write(
            f.repo.path().join(".git/refs/heads/task-branch"),
            format!("{sha}\n"),
        )
        .unwrap();
        assert!(matches!(
            rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
            E::Evidence(_) | E::CommitNotCommit
        ));
    }
}

#[test]
fn recovery_preparation_symbolic_branch_redirection_is_an_observable_mismatch() {
    let f = Fixture::new(None);
    git(f.root(), &["branch", "other-branch"]);
    git(
        f.root(),
        &[
            "symbolic-ref",
            "refs/heads/task-branch",
            "refs/heads/other-branch",
        ],
    );
    assert!(matches!(
        rejected(&f, f.request(WorkspaceRecoveryDecision::InspectAndSalvage)),
        E::BranchMismatch
    ));
}
