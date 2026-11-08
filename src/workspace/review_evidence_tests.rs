//! Review workspace/evidence controls on real temporary Git repositories.
//! Linux protected admission of these handles is NOT RUN here; the portable
//! test checks the exact preconditions `ProtectedLauncher::admit` enforces.

use std::path::PathBuf;

use crate::handle::{CommitSha, WorkspaceState};
use crate::review_evidence::*;
use crate::test_support::{
    TempDir, TestRepo, branch_exists, git, stdout_trimmed, worktree_registered,
};

fn sha(value: &str) -> CommitSha {
    CommitSha::parse(value).unwrap()
}

fn request(repo: &TestRepo, place: &TempDir, name: &str, reviewed: &str) -> ReviewWorkspaceRequest {
    ReviewWorkspaceRequest {
        repository_root: repo.path().to_path_buf(),
        workspace_id: format!("ws-{name}"),
        task_id: "task-reviewed".into(),
        reviewed_sha: sha(reviewed),
        worktree_path: place.path().join(name),
    }
}

#[test]
fn review_workspace_is_task_bound_on_a_fresh_pinned_local_branch() {
    let repo = TestRepo::new("review-provision");
    let reviewed = repo.commit_file("src/a.rs", "a");
    let place = TempDir::new("review-provision-wt");
    let ws = provision_review_workspace(&request(&repo, &place, "r1", &reviewed)).unwrap();
    let handle = ws.handle();
    // The exact preconditions ProtectedLauncher::admit checks.
    assert_eq!(handle.state(), WorkspaceState::Provisioned);
    assert!(handle.state().may_transition_to(WorkspaceState::Active));
    assert_eq!(handle.task_id(), Some("task-reviewed"));
    assert_eq!(
        handle.head_sha().map(CommitSha::as_str),
        Some(reviewed.as_str())
    );
    assert_eq!(handle.base_sha().as_str(), reviewed);
    assert!(ws.review_branch().starts_with(REVIEW_BRANCH_PREFIX));
    assert_eq!(ws.review_branch().len(), REVIEW_BRANCH_PREFIX.len() + 32);
    let wt = handle.worktree_path();
    assert_eq!(stdout_trimmed(&git(wt, &["rev-parse", "HEAD"])), reviewed);
    assert_eq!(
        stdout_trimmed(&git(wt, &["symbolic-ref", "--quiet", "HEAD"])),
        format!("refs/heads/{}", ws.review_branch())
    );
    let integrity = verify_review_workspace(&ws).unwrap();
    assert_eq!(integrity.head_sha.as_str(), reviewed);

    // A second review of the same SHA gets a different branch.
    let ws2 = provision_review_workspace(&request(&repo, &place, "r2", &reviewed)).unwrap();
    assert_ne!(ws.review_branch(), ws2.review_branch());
}

#[test]
fn drift_dirt_and_branch_moves_are_detected() {
    let repo = TestRepo::new("review-drift");
    let reviewed = repo.commit_file("src/a.rs", "a");
    let place = TempDir::new("review-drift-wt");

    let ws = provision_review_workspace(&request(&repo, &place, "dirty", &reviewed)).unwrap();
    std::fs::write(ws.handle().worktree_path().join("new.txt"), "x").unwrap();
    assert!(verify_review_workspace(&ws).is_err(), "untracked file");

    let ws = provision_review_workspace(&request(&repo, &place, "commit", &reviewed)).unwrap();
    let wt = ws.handle().worktree_path().to_path_buf();
    std::fs::write(wt.join("src/a.rs"), "changed").unwrap();
    git(&wt, &["commit", "--quiet", "-am", "post-review commit"]);
    assert!(
        verify_review_workspace(&ws).is_err(),
        "new commit after review"
    );

    let ws = provision_review_workspace(&request(&repo, &place, "switch", &reviewed)).unwrap();
    git(
        ws.handle().worktree_path(),
        &["switch", "--quiet", "--detach"],
    );
    assert!(
        verify_review_workspace(&ws).is_err(),
        "left the review branch"
    );
}

#[test]
fn cleanup_removes_only_owned_unmoved_state() {
    let repo = TestRepo::new("review-cleanup");
    let reviewed = repo.commit_file("src/a.rs", "a");
    let place = TempDir::new("review-cleanup-wt");

    let ws = provision_review_workspace(&request(&repo, &place, "ok", &reviewed)).unwrap();
    let path: PathBuf = ws.handle().worktree_path().to_path_buf();
    remove_review_workspace(repo.path(), &ws).unwrap();
    assert!(!worktree_registered(&repo, &path));
    assert!(!branch_exists(&repo, ws.review_branch()));

    // A moved review ref is never deleted or reset.
    let ws = provision_review_workspace(&request(&repo, &place, "moved", &reviewed)).unwrap();
    let other = repo.commit_file("src/b.rs", "b");
    let path: PathBuf = ws.handle().worktree_path().to_path_buf();
    // Detach the worktree's view first so teardown would see the moved tip.
    git(
        repo.path(),
        &[
            "update-ref",
            &format!("refs/heads/{}", ws.review_branch()),
            &other,
        ],
    );
    assert!(remove_review_workspace(repo.path(), &ws).is_err());
    assert!(
        branch_exists(&repo, ws.review_branch()),
        "moved ref retained"
    );
    assert_eq!(
        stdout_trimmed(&git(
            repo.path(),
            &["rev-parse", &format!("refs/heads/{}", ws.review_branch())]
        )),
        other
    );
    assert!(path.exists(), "worktree retained for recovery");

    // A dirty review worktree is retained.
    let ws = provision_review_workspace(&request(&repo, &place, "dirty", &reviewed)).unwrap();
    std::fs::write(ws.handle().worktree_path().join("scratch"), "x").unwrap();
    assert!(remove_review_workspace(repo.path(), &ws).is_err());
    assert!(branch_exists(&repo, ws.review_branch()));
}

#[test]
fn commit_range_and_diff_are_exact_and_refuse_non_ancestors() {
    let repo = TestRepo::new("review-range");
    let baseline = repo.head_sha();
    let first = repo.commit_file("src/a.rs", "a");
    let second = repo.commit_file("docs/b.md", "b");
    let range = review_commit_range(repo.path(), &sha(&baseline), &sha(&second)).unwrap();
    assert_eq!(range.commits, vec![sha(&first), sha(&second)]);

    let diff = review_diff(repo.path(), &sha(&baseline), &sha(&second)).unwrap();
    assert_eq!(
        diff.changed_paths,
        vec!["docs/b.md".to_string(), "src/a.rs".to_string()]
    );
    assert_eq!(diff.patch_sha256.len(), 64);
    let again = review_diff(repo.path(), &sha(&baseline), &sha(&second)).unwrap();
    assert_eq!(diff, again, "deterministic");
    let narrower = review_diff(repo.path(), &sha(&first), &sha(&second)).unwrap();
    assert_ne!(diff.patch_sha256, narrower.patch_sha256);

    // Reversed order: the implementation is not a descendant of the baseline.
    assert!(review_commit_range(repo.path(), &sha(&second), &sha(&baseline)).is_err());
    // An unknown commit is refused.
    assert!(review_commit_range(repo.path(), &sha(&baseline), &sha(&"e".repeat(40))).is_err());
}

#[test]
fn filter_configuration_is_refused_before_content_reads() {
    let repo = TestRepo::new("review-filter");
    let baseline = repo.head_sha();
    let next = repo.commit_file("src/a.rs", "a");
    git(
        repo.path(),
        &[
            "config",
            "filter.evil.smudge",
            "touch /tmp/receipts-review-pwned",
        ],
    );
    assert!(review_diff(repo.path(), &sha(&baseline), &sha(&next)).is_err());
    assert!(review_commit_range(repo.path(), &sha(&baseline), &sha(&next)).is_err());
}

#[test]
fn temporary_merge_checks_out_the_merged_tree_and_refuses_conflicts() {
    let repo = TestRepo::new("review-merge");
    let base = repo.commit_file("shared.txt", "base\n");
    git(repo.path(), &["switch", "--quiet", "-c", "candidate"]);
    let candidate = repo.commit_file("feature.txt", "feature\n");
    git(repo.path(), &["switch", "--quiet", "main"]);
    let target = repo.commit_file("other.txt", "other\n");
    let place = TempDir::new("review-merge-wt");

    let merge =
        provision_temporary_merge(&request(&repo, &place, "merge", &candidate), &sha(&target))
            .unwrap();
    let wt = merge.workspace.handle().worktree_path();
    assert!(wt.join("feature.txt").exists() && wt.join("other.txt").exists());
    let parents = stdout_trimmed(&git(
        repo.path(),
        &["rev-list", "--parents", "-n", "1", merge.merge_sha.as_str()],
    ));
    assert_eq!(
        parents,
        format!("{} {target} {candidate}", merge.merge_sha.as_str())
    );
    assert_eq!(
        stdout_trimmed(&git(
            repo.path(),
            &[
                "rev-parse",
                &format!("{}^{{tree}}", merge.merge_sha.as_str())
            ]
        )),
        merge.merged_tree
    );
    verify_review_workspace(&merge.workspace).unwrap();
    assert_eq!(
        repo.head_sha(),
        target,
        "no existing branch or worktree moved"
    );
    remove_review_workspace(repo.path(), &merge.workspace).unwrap();

    // Conflicting edits are refused and create no branch or worktree.
    git(repo.path(), &["switch", "--quiet", "-c", "conflict", &base]);
    let left = repo.commit_file("shared.txt", "left\n");
    git(repo.path(), &["switch", "--quiet", "main"]);
    let right = repo.commit_file("shared.txt", "right\n");
    let before = stdout_trimmed(&git(repo.path(), &["for-each-ref", "--format=%(refname)"]));
    assert!(
        provision_temporary_merge(&request(&repo, &place, "conflict", &left), &sha(&right))
            .is_err()
    );
    assert_eq!(
        stdout_trimmed(&git(repo.path(), &["for-each-ref", "--format=%(refname)"])),
        before
    );
    assert!(!place.path().join("conflict").exists());
}
