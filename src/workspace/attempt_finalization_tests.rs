use super::*;
use crate::execution::{ProcessRunRequest, ProcessTimeoutPolicy, start_live_process_attempt};
use crate::test_support::{TestRepo, git, stdout_trimmed};
use crate::{WorkspaceCheckpointKind, WorkspaceProvisionRequest, WriteScopeGitOperation};
use std::{fs, time::Duration};

struct Fixture {
    repo: TestRepo,
    handle: WorkspaceHandle,
    checkpoint: WorkspaceCheckpointCaptureCore,
    terminal: LiveProcessOutcome,
    final_sha: String,
    allowed: Vec<String>,
    forbidden: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let repo = TestRepo::new_nested("attempt-finalization", "repo");
        repo.commit_file("src/file", "start");
        repo.commit_file("outside", "start");
        let handle = WorkspaceProvisionRequest::new(
            repo.path(),
            "workspace",
            Some("task"),
            "task-branch",
            fs::canonicalize(repo.root.path()).unwrap().join("target"),
            &repo.head_sha(),
        )
        .unwrap()
        .provision()
        .unwrap();
        let checkpoint = checkpoint(&handle, "workspace", Some("task"), Some("attempt"));
        let terminal = terminal(
            handle.worktree_path(),
            "/usr/bin/true",
            &[],
            false,
            Duration::from_secs(10),
        );
        fs::write(handle.worktree_path().join("src/file"), "final").unwrap();
        commit(handle.worktree_path());
        let final_sha = stdout_trimmed(&git(handle.worktree_path(), &["rev-parse", "HEAD"]));
        Self {
            repo,
            handle,
            checkpoint,
            terminal,
            final_sha,
            allowed: vec!["src/**".into()],
            forbidden: vec!["src/private/**".into()],
        }
    }
    fn root(&self) -> &Path {
        self.handle.worktree_path()
    }
    fn request(&self) -> WorkspaceAttemptFinalizationRequest<'_> {
        WorkspaceAttemptFinalizationRequest {
            handle: &self.handle,
            workspace_id: "workspace",
            task_id: Some("task"),
            attempt_id: Some("attempt"),
            expected_worktree: self.root(),
            expected_branch: "task-branch",
            start_sha: self.handle.base_sha().as_str(),
            final_sha: &self.final_sha,
            allowed_write_paths: &self.allowed,
            forbidden_write_paths: &self.forbidden,
            checkpoint: &self.checkpoint,
            terminal: &self.terminal,
        }
    }
    fn recommit(&mut self) {
        commit(self.root());
        self.final_sha = stdout_trimmed(&git(self.root(), &["rev-parse", "HEAD"]));
    }
}
fn checkpoint(
    handle: &WorkspaceHandle,
    workspace: &str,
    task: Option<&str>,
    attempt: Option<&str>,
) -> WorkspaceCheckpointCaptureCore {
    WorkspaceCheckpointCaptureCore::new(
        "preceding",
        workspace,
        task.map(str::to_owned),
        attempt.map(str::to_owned),
        WorkspaceCheckpointKind::Progress,
        handle.base_sha().clone(),
        None,
        None,
        vec![],
        vec![],
        vec![],
        None,
        None,
    )
    .unwrap()
}
fn commit(root: &Path) {
    git(root, &["add", "--all"]);
    git(
        root,
        &["commit", "--quiet", "--allow-empty", "-m", "fixture"],
    );
}
fn terminal(
    root: &Path,
    executable: &str,
    args: &[&str],
    cancel: bool,
    timeout: Duration,
) -> LiveProcessOutcome {
    let request = ProcessRunRequest::new(executable, args.iter().copied(), root, root).unwrap();
    let attempt = start_live_process_attempt(
        &request,
        &ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap(),
    )
    .unwrap();
    if cancel {
        attempt.cancel();
    }
    attempt.wait_collect().unwrap()
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for item in fs::read_dir(path).unwrap() {
            let item = item.unwrap();
            if item.file_type().unwrap().is_dir() {
                visit(root, &item.path(), out);
            } else if item.file_type().unwrap().is_symlink() {
                out.push((
                    item.path().strip_prefix(root).unwrap().into(),
                    fs::read_link(item.path())
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                ));
            } else {
                out.push((
                    item.path().strip_prefix(root).unwrap().into(),
                    fs::read(item.path()).unwrap(),
                ));
            }
        }
    }
    let mut out = vec![];
    visit(root, root, &mut out);
    out.sort();
    out
}
fn rejected(f: &Fixture, request: WorkspaceAttemptFinalizationRequest<'_>) -> E {
    let before = snapshot(f.repo.root.path());
    let error = finalize_workspace_attempt(request).unwrap_err();
    assert_eq!(
        snapshot(f.repo.root.path()),
        before,
        "finalization must not repair any fixture state"
    );
    error
}

#[test]
fn attempt_finalization_clean_exact_anchor_preserves_older_checkpoint_and_all_evidence() {
    let f = Fixture::new();
    let before = snapshot(f.repo.root.path());
    let result = finalize_workspace_attempt(f.request()).unwrap();
    assert_eq!(snapshot(f.repo.root.path()), before);
    assert_eq!(result.handle(), &f.handle);
    assert_eq!(result.workspace_id(), "workspace");
    assert_eq!(result.task_id(), Some("task"));
    assert_eq!(result.attempt_id(), Some("attempt"));
    assert_eq!(result.canonical_root(), f.root());
    assert_eq!(result.branch(), "task-branch");
    assert_eq!(result.start_sha(), f.handle.base_sha());
    assert_eq!(result.final_sha().as_str(), f.final_sha);
    assert_eq!(result.checkpoint(), &f.checkpoint);
    assert_ne!(result.checkpoint().head_sha(), result.final_sha());
    assert_eq!(result.changed_paths(), ["src/file"]);
    assert_eq!(
        result.write_scope().verification(),
        WriteScopeVerificationStatus::Pass
    );
    assert!(result.modified_files().is_empty());
    assert!(result.untracked_files().is_empty());
    assert_eq!(result.terminal_cause(), LiveProcessTerminalCause::Completed);
    assert_eq!(result.exit_code(), Some(0));
}

#[test]
fn attempt_finalization_committed_scope_matrix() {
    for case in ["modification", "deletion", "nested-forbidden", "mixed"] {
        let mut f = Fixture::new();
        match case {
            "deletion" => fs::remove_file(f.root().join("outside")).unwrap(),
            "nested-forbidden" => {
                fs::create_dir_all(f.root().join("src/private/nested")).unwrap();
                fs::write(f.root().join("src/private/nested/file"), "forbidden").unwrap();
            }
            _ => {
                fs::write(f.root().join("outside"), "unauthorized").unwrap();
            }
        }
        f.recommit();
        let E::WriteScopeFailed(evidence) = rejected(&f, f.request()) else {
            panic!("{case}")
        };
        assert_eq!(evidence.verification(), WriteScopeVerificationStatus::Fail);
        assert!(evidence.changed_paths().contains(&"src/file".into()));
    }
}

#[test]
fn attempt_finalization_dirty_residuals_are_rejected_even_inside_scope() {
    for (name, staged, tracked) in [
        ("outside", false, true),
        ("src/file", false, true),
        ("src/file", true, true),
        ("loose", false, false),
        ("src/loose", false, false),
    ] {
        let f = Fixture::new();
        fs::write(f.root().join(name), "residual").unwrap();
        if staged {
            git(f.root(), &["add", name]);
        }
        let error = rejected(&f, f.request());
        match error {
            E::DirtyTracked(paths) if tracked => assert_eq!(paths, [name]),
            E::DirtyUntracked(paths) if !tracked => assert_eq!(paths, [name]),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn attempt_finalization_exact_sha_syntax_and_handle_authority() {
    let f = Fixture::new();
    for sha in [
        "",
        "bad",
        "1234567",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "task-branch",
        "HEAD",
        "gggggggggggggggggggggggggggggggggggggggg",
    ] {
        let mut r = f.request();
        r.start_sha = sha;
        assert!(matches!(rejected(&f, r), E::InvalidStartSha));
        let mut r = f.request();
        r.final_sha = sha;
        assert!(matches!(rejected(&f, r), E::InvalidFinalSha));
    }
    let mut r = f.request();
    r.start_sha = &f.final_sha;
    assert!(matches!(rejected(&f, r), E::StartAuthorityMismatch));
    let mut r = f.request();
    r.final_sha = f.handle.base_sha().as_str();
    assert!(matches!(rejected(&f, r), E::CurrentHeadMismatch));
}

#[test]
fn attempt_finalization_missing_and_tag_objects_fail_locally() {
    let f = Fixture::new();
    git(f.root(), &["tag", "-a", "tag-object", "-m", "tag"]);
    let tag = stdout_trimmed(&git(f.root(), &["rev-parse", "tag-object"]));
    for sha in [&"0".repeat(40), &tag] {
        let mut r = f.request();
        r.final_sha = sha;
        assert!(matches!(
            rejected(&f, r),
            E::WriteScopeTechnical(WriteScopeVerificationError::CommitUnavailable {
                operation: WriteScopeGitOperation::CandidateCommit,
                ..
            })
        ));
        // A retained provisioning commit can disappear; constructor is private
        // fixture authority, allowing the missing/noncommit base to be isolated.
        let handle = WorkspaceHandle::provisioned(
            "workspace".into(),
            Some("task".into()),
            "task-branch".into(),
            f.root().into(),
            CommitSha::parse(sha).unwrap(),
            None,
        );
        let mut r = f.request();
        r.handle = &handle;
        r.start_sha = sha;
        assert!(matches!(
            rejected(&f, r),
            E::WriteScopeTechnical(WriteScopeVerificationError::CommitUnavailable {
                operation: WriteScopeGitOperation::BaselineCommit,
                ..
            })
        ));
    }
}

#[test]
fn attempt_finalization_request_and_checkpoint_identity_matrix() {
    let f = Fixture::new();
    let mut r = f.request();
    r.workspace_id = "other";
    assert!(matches!(rejected(&f, r), E::WorkspaceMismatch));
    for task in [None, Some("other")] {
        let mut r = f.request();
        r.task_id = task;
        assert!(matches!(rejected(&f, r), E::TaskMismatch));
    }
    let no_task = WorkspaceHandle::provisioned(
        "workspace".into(),
        None,
        "task-branch".into(),
        f.root().into(),
        f.handle.base_sha().clone(),
        None,
    );
    let mut r = f.request();
    r.handle = &no_task;
    assert!(matches!(rejected(&f, r), E::TaskMismatch));
    for (workspace, task, attempt) in [
        ("other", Some("task"), Some("attempt")),
        ("workspace", None, Some("attempt")),
        ("workspace", Some("other"), Some("attempt")),
        ("workspace", Some("task"), None),
        ("workspace", Some("task"), Some("other")),
    ] {
        let core = checkpoint(&f.handle, workspace, task, attempt);
        let mut r = f.request();
        r.checkpoint = &core;
        assert!(matches!(
            rejected(&f, r),
            E::CheckpointWorkspaceMismatch
                | E::CheckpointTaskMismatch
                | E::CheckpointAttemptMismatch
        ));
    }
}

#[test]
fn attempt_finalization_branch_and_target_matrix() {
    let f = Fixture::new();
    let mut r = f.request();
    r.expected_branch = "other";
    assert!(matches!(rejected(&f, r), E::BranchMismatch));
    let mut r = f.request();
    r.expected_worktree = f.repo.path();
    assert!(matches!(rejected(&f, r), E::WorktreeRootMismatch));
    git(f.root(), &["checkout", "--quiet", "-b", "other"]);
    assert!(matches!(rejected(&f, f.request()), E::BranchMismatch));
    git(f.root(), &["checkout", "--quiet", "--detach"]);
    assert!(matches!(rejected(&f, f.request()), E::DetachedHead));
}

#[test]
fn attempt_finalization_sibling_gitfile_and_symlink_substitution_fail() {
    for symlink in [false, true] {
        let f = Fixture::new();
        let sibling = fs::canonicalize(f.repo.root.path())
            .unwrap()
            .join("sibling");
        git(
            f.repo.path(),
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "sibling",
                sibling.to_str().unwrap(),
            ],
        );
        if symlink {
            fs::rename(f.root(), f.root().with_file_name("retained")).unwrap();
            std::os::unix::fs::symlink(&sibling, f.root()).unwrap();
        } else {
            fs::copy(sibling.join(".git"), f.root().join(".git")).unwrap();
        }
        assert!(matches!(
            rejected(&f, f.request()),
            E::WorktreeRootMismatch | E::BranchMismatch
        ));
    }
}

#[test]
fn attempt_finalization_terminal_outcomes_are_real_collected_execution_evidence() {
    let f = Fixture::new();
    for (executable, args, cancel, timeout, cause) in [
        (
            "/bin/sleep",
            vec!["60"],
            true,
            Duration::from_secs(30),
            LiveProcessTerminalCause::Cancelled,
        ),
        (
            "/bin/sleep",
            vec!["60"],
            false,
            Duration::from_millis(50),
            LiveProcessTerminalCause::TimedOut,
        ),
        (
            "/usr/bin/false",
            vec![],
            false,
            Duration::from_secs(10),
            LiveProcessTerminalCause::Completed,
        ),
    ] {
        let outcome = terminal(f.root(), executable, &args, cancel, timeout);
        assert_eq!(outcome.terminal_cause(), cause);
        let mut r = f.request();
        r.terminal = &outcome;
        assert!(
            matches!(rejected(&f, r), E::TerminalUnacceptable { cause: actual, .. } if actual == cause)
        );
    }
}

#[test]
fn attempt_finalization_deterministic_changes_between_observations_fail() {
    for change in ["head", "branch", "tracked", "untracked", "root"] {
        let f = Fixture::new();
        let root = f.root().to_owned();
        CAPTURE_GATE.with(|gate| {
            *gate.borrow_mut() = Some(Box::new(move || match change {
                "head" => {
                    git(&root, &["commit", "--quiet", "--allow-empty", "-m", "race"]);
                }
                "branch" => {
                    git(&root, &["checkout", "--quiet", "-b", "race"]);
                }
                "tracked" => fs::write(root.join("src/file"), "race").unwrap(),
                "untracked" => fs::write(root.join("src/new"), "race").unwrap(),
                _ => {
                    fs::rename(&root, root.with_file_name("moved")).unwrap();
                }
            }))
        });
        let E::RepositoryChangedDuringCapture(error) =
            finalize_workspace_attempt(f.request()).unwrap_err()
        else {
            panic!("{change}")
        };
        assert!(matches!(
            *error,
            E::CurrentHeadMismatch
                | E::BranchMismatch
                | E::DirtyTracked(_)
                | E::DirtyUntracked(_)
                | E::TargetIo(_)
        ));
    }
}

#[cfg(unix)]
#[test]
fn attempt_finalization_preserves_index_metadata_and_every_repository_byte() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    fs::File::options()
        .write(true)
        .open(f.root().join("src/file"))
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1)),
        )
        .unwrap();
    let index = f.root().join(stdout_trimmed(&git(
        f.root(),
        &["rev-parse", "--git-path", "index"],
    )));
    let metadata = || {
        let m = fs::metadata(&index).unwrap();
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    let before = snapshot(f.repo.root.path());
    let meta = metadata();
    finalize_workspace_attempt(f.request()).unwrap();
    assert_eq!(metadata(), meta);
    assert_eq!(snapshot(f.repo.root.path()), before);
}

#[test]
fn attempt_finalization_malformed_policy_fails_technically() {
    let mut f = Fixture::new();
    f.allowed.push("src/**broken".into());
    assert!(matches!(
        rejected(&f, f.request()),
        E::WriteScopeTechnical(WriteScopeVerificationError::InvalidPattern { .. })
    ));
}

#[test]
fn attempt_finalization_missing_provisioned_base_and_promisor_final_stay_local() {
    let f = Fixture::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/repo", listener.local_addr().unwrap());
    for (key, value) in [
        ("remote.origin.url", url.as_str()),
        ("remote.origin.promisor", "true"),
        ("remote.origin.partialclonefilter", "blob:none"),
        ("protocol.http.allow", "always"),
    ] {
        git(f.root(), &["config", key, value]);
    }
    let missing = "0".repeat(40);
    let mut request = f.request();
    request.final_sha = &missing;
    assert!(matches!(
        rejected(&f, request),
        E::WriteScopeTechnical(WriteScopeVerificationError::CommitUnavailable {
            operation: WriteScopeGitOperation::CandidateCommit,
            ..
        })
    ));
    let sha = f.handle.base_sha().as_str();
    let object = f
        .repo
        .path()
        .join(".git/objects")
        .join(&sha[..2])
        .join(&sha[2..]);
    fs::remove_file(&object).unwrap();
    assert!(matches!(
        rejected(&f, f.request()),
        E::WriteScopeTechnical(WriteScopeVerificationError::CommitUnavailable {
            operation: WriteScopeGitOperation::BaselineCommit,
            ..
        })
    ));
    assert!(!object.exists());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn attempt_finalization_checkpoint_checks_and_references_remain_inert() {
    use crate::{
        WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
        WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
    };
    let f = Fixture::new();
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::Url,
        "https://never-resolve.invalid/evidence",
        Some("do not recompute".into()),
        None,
    )
    .unwrap();
    let checks = vec![
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec![
                "/usr/bin/touch".into(),
                f.root().join("must-not-exist").to_str().unwrap().into(),
            ],
            -7,
            f.handle.base_sha().clone(),
            None,
            Some(reference.clone()),
        )
        .unwrap(),
    ];
    let core = WorkspaceCheckpointCaptureCore::new(
        "preceding",
        "workspace",
        Some("task".into()),
        Some("attempt".into()),
        WorkspaceCheckpointKind::Progress,
        f.handle.base_sha().clone(),
        None,
        Some(reference),
        vec!["old/dirty".into()],
        vec!["old/untracked".into()],
        checks,
        None,
        None,
    )
    .unwrap();
    let before = snapshot(f.repo.root.path());
    let mut request = f.request();
    request.checkpoint = &core;
    let evidence = finalize_workspace_attempt(request).unwrap();
    assert_eq!(evidence.checkpoint(), &core);
    assert_eq!(snapshot(f.repo.root.path()), before);
}

#[test]
fn attempt_finalization_current_state_bound_is_enforced_and_ignored_files_are_excluded() {
    let mut f = Fixture::new();
    fs::write(f.root().join("src/.gitignore"), "*.ignored\n").unwrap();
    f.recommit();
    fs::write(f.root().join("src/secret.ignored"), "ignored").unwrap();
    assert!(finalize_workspace_attempt(f.request()).is_ok());
    for i in 0..(1_048_576 / 245 + 1) {
        fs::write(
            f.root()
                .join("src")
                .join(format!("{i:08}{}", "x".repeat(232))),
            "x",
        )
        .unwrap();
    }
    assert!(matches!(
        rejected(&f, f.request()),
        E::Evidence(
            WorkspaceCheckpointEvidenceCaptureError::GitEvidenceTooLarge {
                observation: Observation::UntrackedPaths,
                ..
            }
        )
    ));
}
