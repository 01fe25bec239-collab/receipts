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

fn refused_config(
    result: Result<ReviewWorkspaceIntegrityV1, crate::error::WorkspaceError>,
) -> String {
    match result {
        Err(crate::error::WorkspaceError::RepositoryConfigRefused { key }) => key,
        other => panic!("expected a configuration refusal, got {other:?}"),
    }
}

#[test]
fn verify_refuses_post_provision_filters_and_includes_before_they_run() {
    let repo = TestRepo::new("review-verify-filter");
    std::fs::write(repo.path().join(".gitattributes"), "src/a.rs filter=x\n").unwrap();
    let reviewed = repo.commit_file("src/a.rs", "original");
    let place = TempDir::new("review-verify-filter-wt");
    let marker = place.path().join("filter-ran");
    let ws = provision_review_workspace(&request(&repo, &place, "f", &reviewed)).unwrap();
    // Valid clean review succeeds.
    verify_review_workspace(&ws).unwrap();

    // Configured after provisioning; equal-length tracked edit so status
    // must hash content (and would run the clean filter).
    git(
        repo.path(),
        &[
            "config",
            "filter.x.clean",
            &format!("touch {}; cat", marker.display()),
        ],
    );
    let file = ws.handle().worktree_path().join("src/a.rs");
    std::fs::write(&file, "changed!").unwrap();
    assert_eq!(
        refused_config(verify_review_workspace(&ws)),
        "filter.x.clean"
    );
    assert!(!marker.exists(), "the filter must not run before refusal");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"changed!",
        "evidence untouched"
    );
    git(repo.path(), &["config", "--unset", "filter.x.clean"]);

    // Worktree-scoped configuration of the actual worktree is refused too.
    git(
        repo.path(),
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        ws.handle().worktree_path(),
        &["config", "--worktree", "include.path", "/dev/null"],
    );
    assert_eq!(refused_config(verify_review_workspace(&ws)), "include.path");
    assert!(!marker.exists());
}

#[test]
fn configured_submodule_ignores_cannot_hide_dirt() {
    let sub = TestRepo::new("review-sub-origin");
    let recorded = sub.head_sha();
    let repo = TestRepo::new("review-sub-super");
    git(
        repo.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{recorded},sub"),
        ],
    );
    git(repo.path(), &["commit", "--quiet", "-m", "gitlink"]);
    let reviewed = repo.head_sha();
    let place = TempDir::new("review-sub-wt");
    let ws = provision_review_workspace(&request(&repo, &place, "s", &reviewed)).unwrap();
    verify_review_workspace(&ws).unwrap();

    // Populate the gitlink in the review worktree and move it.
    let checkout = ws.handle().worktree_path().join("sub");
    std::fs::remove_dir_all(&checkout).ok();
    git(
        ws.handle().worktree_path(),
        &["clone", "--quiet", sub.path().to_str().unwrap(), "sub"],
    );
    git(
        &checkout,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "moved",
        ],
    );
    git(repo.path(), &["config", "diff.ignoreSubmodules", "all"]);
    git(repo.path(), &["config", "submodule.sub.ignore", "all"]);
    // The configuration hides it from a default status ...
    assert!(
        stdout_trimmed(&git(
            ws.handle().worktree_path(),
            &["status", "--porcelain"]
        ))
        .is_empty()
    );
    // ... but not from verification.
    assert!(verify_review_workspace(&ws).is_err());
}

#[test]
fn stale_handles_never_remove_a_replacement_workspace() {
    use crate::provision::WorkspaceProvisionRequest;
    let repo = TestRepo::new("review-own");
    let reviewed = repo.commit_file("src/a.rs", "a");
    let place = TempDir::new("review-own-wt");

    // Valid owning cleanup.
    let ws = provision_review_workspace(&request(&repo, &place, "valid", &reviewed)).unwrap();
    remove_review_workspace(repo.path(), &ws).unwrap();

    // Stale handle after removal, then a different task/workspace is
    // provisioned at the identical path, branch and SHA.
    let ws = provision_review_workspace(&request(&repo, &place, "stale", &reviewed)).unwrap();
    let path = ws.handle().worktree_path().to_path_buf();
    remove_review_workspace(repo.path(), &ws).unwrap();
    let reprovision = |workspace: &str, task: &str| {
        WorkspaceProvisionRequest::new(
            repo.path(),
            workspace,
            Some(task),
            ws.review_branch(),
            &path,
            &reviewed,
        )
        .unwrap()
        .provision()
        .unwrap()
    };
    let foreign = reprovision("foreign-workspace", "foreign-task");
    assert!(remove_review_workspace(repo.path(), &ws).is_err());
    assert!(
        path.exists() && worktree_registered(&repo, &path),
        "replacement retained"
    );
    assert!(
        branch_exists(&repo, ws.review_branch()),
        "replacement branch retained"
    );
    crate::teardown::WorkspaceTeardownRequest::new(repo.path())
        .teardown(&foreign)
        .unwrap();
    git(
        repo.path(),
        &["branch", "--quiet", "-D", ws.review_branch()],
    );

    // Identical workspace/task fields, but a new administrative record:
    // matching SHA and fields are not ownership.
    let _same_fields = reprovision(ws.handle().workspace_id(), "task-reviewed");
    assert!(remove_review_workspace(repo.path(), &ws).is_err());
    assert!(path.exists() && branch_exists(&repo, ws.review_branch()));

    // Altered, replaced or missing binding on a live workspace refuses.
    let live = provision_review_workspace(&request(&repo, &place, "live", &reviewed)).unwrap();
    let live_path = live.handle().worktree_path().to_path_buf();
    let binding = crate::test_support::worktree_meta_dir(&repo, &live_path)
        .unwrap()
        .join(crate::worktree_binding::BINDING_FILE);
    let original = std::fs::read(&binding).unwrap();
    // Same bytes, new object: the administrative identity was replaced.
    std::fs::remove_file(&binding).unwrap();
    std::fs::write(&binding, &original).unwrap();
    assert!(remove_review_workspace(repo.path(), &live).is_err());
    // Altered content.
    std::fs::write(&binding, b"receipts-workspace-binding/v1\n").unwrap();
    assert!(remove_review_workspace(repo.path(), &live).is_err());
    // Missing.
    std::fs::remove_file(&binding).unwrap();
    assert!(remove_review_workspace(repo.path(), &live).is_err());
    assert!(live_path.exists() && branch_exists(&repo, live.review_branch()));

    // A different repository root is refused before anything is inspected.
    let other = TestRepo::new("review-own-other");
    let owned = provision_review_workspace(&request(&repo, &place, "owned", &reviewed)).unwrap();
    assert!(remove_review_workspace(other.path(), &owned).is_err());
    remove_review_workspace(repo.path(), &owned).unwrap();
}
