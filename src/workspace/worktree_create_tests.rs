use std::time::{Duration, Instant};

use super::*;
use crate::WorkspaceState;
use crate::test_support::{TestRepo, branch_exists, git, worktree_registered};

fn task() -> Option<WorktreeCreateTask> {
    Some(WorktreeCreateTask {
        workspace_id: "ws-1".into(),
        task_id: "task-1".into(),
    })
}

fn request(
    repo: &TestRepo,
    name: &str,
    base: &str,
    task: Option<WorktreeCreateTask>,
) -> WorktreeCreateRequest {
    WorktreeCreateRequest::new(
        repo.path(),
        repo.path().join(name),
        &format!("task/{name}"),
        base,
        task,
    )
    .unwrap()
}

#[test]
fn idempotent_second_call_same_path_without_reset_or_new_handle() {
    let repo = TestRepo::new("wc-idem");
    let base = repo.head_sha();
    let first = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(first.mode(), WorktreeCreateMode::TaskProvisioned);
    assert!(first.path().is_absolute());
    assert_eq!(
        first.path(),
        std::fs::canonicalize(repo.path().join("wt")).unwrap()
    );
    assert_eq!(first.handle().unwrap().state(), WorkspaceState::Provisioned);

    // Edited and committed work is preserved; HEAD is reported as it is.
    std::fs::write(first.path().join("work.txt"), "w").unwrap();
    git(first.path(), &["add", "work.txt"]);
    git(first.path(), &["commit", "-m", "work"]);
    std::fs::write(first.path().join("dirty.txt"), "d").unwrap();
    let head = crate::test_support::stdout_trimmed(&git(first.path(), &["rev-parse", "HEAD"]));

    let second = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(second.mode(), WorktreeCreateMode::TaskAttached);
    assert_eq!(second.path(), first.path());
    assert_eq!(second.head_sha().as_str(), head);
    assert!(!second.clean());
    assert!(
        second.handle().is_none(),
        "no fabricated PROVISIONED handle"
    );
    assert!(first.path().join("dirty.txt").exists());
}

#[test]
fn mismatched_branch_base_or_unregistered_target_refuses() {
    let repo = TestRepo::new("wc-mismatch");
    let base = repo.head_sha();
    request(&repo, "wt", &base, task()).create().unwrap();
    // Same path, different branch.
    let other_branch = WorktreeCreateRequest::new(
        repo.path(),
        repo.path().join("wt"),
        "task/other",
        &base,
        task(),
    )
    .unwrap();
    assert!(matches!(
        other_branch.create(),
        Err(WorkspaceError::WorktreeCreateConflict { .. })
    ));
    assert!(!branch_exists(&repo, "task/other"));
    // Same branch and path, base not in its history.
    let unrelated = repo.commit_file("elsewhere", "x");
    assert!(matches!(
        request(&repo, "wt", &unrelated, task()).create(),
        Err(WorkspaceError::WorktreeCreateConflict { .. })
    ));
    // Same branch requested at a different path refuses (branch exists).
    assert!(matches!(
        WorktreeCreateRequest::new(
            repo.path(),
            repo.path().join("elsewhere-wt"),
            "task/wt",
            &base,
            task()
        )
        .unwrap()
        .create(),
        Err(WorkspaceError::BranchAlreadyExists { .. })
    ));
    // Existing unregistered content is never adopted.
    std::fs::create_dir(repo.path().join("occupied")).unwrap();
    std::fs::write(repo.path().join("occupied/file"), "keep").unwrap();
    assert!(request(&repo, "occupied", &base, task()).create().is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("occupied/file")).unwrap(),
        "keep"
    );
}

#[test]
fn no_task_context_falls_back_to_plain_worktree() {
    let repo = TestRepo::new("wc-plain");
    let base = repo.head_sha();
    let created = request(&repo, "plain", &base, None).create().unwrap();
    assert_eq!(created.mode(), WorktreeCreateMode::PlainCreated);
    assert!(
        created.handle().is_none(),
        "no task admission without context"
    );
    assert!(worktree_registered(&repo, created.path()));
    assert_eq!(created.head_sha().as_str(), base);
    let again = request(&repo, "plain", &base, None).create().unwrap();
    assert_eq!(again.mode(), WorktreeCreateMode::PlainAttached);
    assert_eq!(again.path(), created.path());
}

#[test]
fn create_never_fetches_from_a_configured_remote() {
    let repo = TestRepo::new("wc-nofetch");
    // A remote whose URL would fail loudly if contacted.
    git(
        repo.path(),
        &["remote", "add", "origin", "/nonexistent/remote.git"],
    );
    let base = repo.head_sha();
    request(&repo, "wt", &base, task()).create().unwrap();
    let refs =
        crate::test_support::stdout_trimmed(&git(repo.path(), &["for-each-ref", "refs/remotes"]));
    assert!(refs.is_empty());
}

#[test]
fn completes_under_500ms_on_small_repo() {
    let repo = TestRepo::new("wc-fast");
    let base = repo.head_sha();
    let started = Instant::now();
    request(&repo, "wt", &base, task()).create().unwrap();
    let elapsed = started.elapsed();
    // Recorded for evidence; the bound applies to a quiet host.
    eprintln!("worktree_create_elapsed_ms={}", elapsed.as_millis());
    assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
}
