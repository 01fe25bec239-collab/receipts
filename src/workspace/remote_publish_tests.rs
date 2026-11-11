use crate::{
    WorkspaceRemotePublishPolicy as W,
    remote_publish::*,
    test_support::{TestRepo, git},
};
#[test]
fn owned_remote_exact_publication_and_acceptance_refusal() {
    let source = TestRepo::new("publish-source");
    let remote = TestRepo::new("publish-remote");
    git(
        remote.path(),
        &["config", "receive.denyCurrentBranch", "ignore"],
    );
    git(source.path(), &["branch", "workstream"]);
    let sha = source.head_sha();
    let endpoint = remote.path().to_str().unwrap();
    let mut request = PublicationRequest {
        repository: source.path(),
        remote: endpoint,
        branch: "workstream",
        commit: &sha,
        wire: W::PushOnAccept,
        mode: ProjectRemoteMode::PushA2Branches,
        class: PublicationBranchClass::Workstream,
        accepted_commit: None,
    };
    assert_eq!(publish(&request), PublicationOutcome::Refused);
    request.accepted_commit = Some(&sha);
    assert_eq!(publish(&request), PublicationOutcome::Published);
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            remote.path(),
            &["rev-parse", "refs/heads/workstream"]
        )),
        sha
    );
    request.wire = W::LocalOnly;
    request.mode = ProjectRemoteMode::LocalOnly;
    request.remote = "/does/not/exist";
    request.repository = std::path::Path::new("/does/not/exist");
    assert_eq!(publish(&request), PublicationOutcome::LocalOnly);
}

#[test]
fn concurrent_publish_and_branch_updates_refuse_while_anchor_is_locked() {
    let source = TestRepo::new("publish-race");
    let remote = TestRepo::new("publish-race-remote");
    git(source.path(), &["branch", "workstream"]);
    let sha = source.head_sha();
    let next = source.commit_file("next", "new commit");
    let request = PublicationRequest {
        repository: source.path(),
        remote: remote.path().to_str().unwrap(),
        branch: "workstream",
        commit: &sha,
        wire: W::PushOnAccept,
        mode: ProjectRemoteMode::PushA2Branches,
        class: PublicationBranchClass::Workstream,
        accepted_commit: Some(&sha),
    };
    let outcome = publish_locked(&request, || {
        std::thread::scope(|scope| {
            assert_eq!(
                scope.spawn(|| publish(&request)).join().unwrap(),
                PublicationOutcome::Refused
            );
        });
        let update = crate::test_support::git_raw(
            source.path(),
            &["update-ref", "refs/heads/workstream", &next, &sha],
        );
        assert!(!update.status.success());
    });
    assert_eq!(outcome, PublicationOutcome::Published);
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            remote.path(),
            &["rev-parse", "refs/heads/workstream"]
        )),
        sha
    );
    // Cleanup releases only our lock; a subsequent normal update succeeds.
    git(
        source.path(),
        &["update-ref", "refs/heads/workstream", &next, &sha],
    );
    assert_eq!(publish(&request), PublicationOutcome::Refused);
}

#[test]
fn rejected_checkpoint_is_retained_and_non_fast_forward_cannot_overwrite_it() {
    let source = TestRepo::new("publish-retention");
    let remote = TestRepo::new("publish-retention-remote");
    git(source.path(), &["branch", "checkpoint/rejected"]);
    let original = source.head_sha();
    let mut request = PublicationRequest {
        repository: source.path(),
        remote: remote.path().to_str().unwrap(),
        branch: "checkpoint/rejected",
        commit: &original,
        wire: W::PushAlways,
        mode: ProjectRemoteMode::PushAllCheckpoints,
        class: PublicationBranchClass::Checkpoint,
        accepted_commit: None,
    };
    assert_eq!(publish(&request), PublicationOutcome::Published);
    // Independent root commit is unrelated: an ordinary push must refuse it.
    git(source.path(), &["checkout", "--orphan", "unrelated"]);
    git(
        source.path(),
        &["commit", "--allow-empty", "-m", "unrelated"],
    );
    let unrelated = source.head_sha();
    git(
        source.path(),
        &["update-ref", "refs/heads/checkpoint/rejected", &unrelated],
    );
    request.commit = &unrelated;
    assert_eq!(publish(&request), PublicationOutcome::RemoteFailed);
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            remote.path(),
            &["rev-parse", "refs/heads/checkpoint/rejected"]
        )),
        original
    );
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            source.path(),
            &["rev-parse", "refs/heads/checkpoint/rejected"]
        )),
        unrelated
    );
}
