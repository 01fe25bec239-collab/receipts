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

#[test]
fn repository_receive_pack_override_is_refused_before_push() {
    let source = TestRepo::new("publish-receivepack-refusal");
    let remote = TestRepo::new("publish-receivepack-remote");
    git(source.path(), &["branch", "checkpoint/refusal"]);
    git(
        source.path(),
        &[
            "config",
            "remote.origin.url",
            remote.path().to_str().unwrap(),
        ],
    );
    // Inert: no executable exists at this path, even before the repair.
    git(
        source.path(),
        &[
            "config",
            "remote.origin.receivepack",
            "/nonexistent/receipts-unapproved-receive-pack",
        ],
    );
    let sha = source.head_sha();
    let request = PublicationRequest {
        repository: source.path(),
        remote: "origin",
        branch: "checkpoint/refusal",
        commit: &sha,
        wire: W::PushAlways,
        mode: ProjectRemoteMode::PushAllCheckpoints,
        class: PublicationBranchClass::Checkpoint,
        accepted_commit: None,
    };
    let mut reached_push = false;
    let outcome = publish_locked(&request, || reached_push = true);
    assert_eq!(outcome, PublicationOutcome::Refused);
    assert!(
        !reached_push,
        "configuration must refuse before locking/pushing"
    );
    assert!(!crate::test_support::branch_exists(
        &remote,
        "checkpoint/refusal"
    ));
    git(
        source.path(),
        &["config", "--unset", "remote.origin.receivepack"],
    );
    assert_eq!(publish(&request), PublicationOutcome::Published);
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            remote.path(),
            &["rev-parse", "refs/heads/checkpoint/refusal"]
        )),
        sha
    );
}

#[test]
fn push_all_checkpoints_refuses_other_classes_and_keeps_acceptance_gating() {
    let source = TestRepo::new("publish-classes");
    let remote = TestRepo::new("publish-classes-remote");
    let sha = source.head_sha();
    for (branch, class) in [
        ("workstream/policy", PublicationBranchClass::Workstream),
        ("task/policy", PublicationBranchClass::Task),
        ("checkpoint/policy", PublicationBranchClass::Checkpoint),
    ] {
        git(source.path(), &["branch", branch]);
        let mut request = PublicationRequest {
            repository: source.path(),
            remote: remote.path().to_str().unwrap(),
            branch,
            commit: &sha,
            wire: W::PushAlways,
            mode: ProjectRemoteMode::PushAllCheckpoints,
            class,
            accepted_commit: None,
        };
        if class == PublicationBranchClass::Checkpoint {
            assert_eq!(publish(&request), PublicationOutcome::Published);
        } else {
            assert_eq!(publish(&request), PublicationOutcome::Refused);
            assert!(!crate::test_support::branch_exists(&remote, branch));
            request.accepted_commit = Some(&sha);
            assert_eq!(publish(&request), PublicationOutcome::Refused);
            request.wire = W::PushOnAccept;
            request.mode = ProjectRemoteMode::PushAcceptedA3;
            request.accepted_commit = None;
            assert_eq!(publish(&request), PublicationOutcome::Refused);
            request.accepted_commit = Some(&sha);
            assert_eq!(publish(&request), PublicationOutcome::Published);
        }
    }
}

#[test]
fn publication_overrides_are_refused_without_reaching_push() {
    let source = TestRepo::new("publish-config-overrides");
    let remote = TestRepo::new("publish-config-overrides-remote");
    git(source.path(), &["branch", "checkpoint/config"]);
    git(
        source.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let sha = source.head_sha();
    let request = PublicationRequest {
        repository: source.path(),
        remote: "origin",
        branch: "checkpoint/config",
        commit: &sha,
        wire: W::PushAlways,
        mode: ProjectRemoteMode::PushAllCheckpoints,
        class: PublicationBranchClass::Checkpoint,
        accepted_commit: None,
    };
    for (key, value) in [
        ("remote.origin.receivepack", "/nonexistent/unapproved"),
        ("remote.origin.uploadpack", "/nonexistent/unapproved"),
        ("remote.origin.vcs", "unapproved"),
        ("remote.origin.pushurl", "unapproved::inert"),
        ("remote.origin.proxy", "/nonexistent/unapproved"),
        ("remote.origin.mirror", "true"),
        ("url.unapproved.insteadOf", "inert:"),
        ("url.unapproved.pushInsteadOf", "inert:"),
        ("protocol.ext.allow", "always"),
        ("protocol.file.allow", "always"),
        ("core.sshCommand", "/nonexistent/unapproved"),
        ("core.gitProxy", "/nonexistent/unapproved"),
        ("core.alternateRefsCommand", "/nonexistent/unapproved"),
        ("core.askPass", "/nonexistent/unapproved"),
        ("credential.helper", "/nonexistent/unapproved"),
        ("http.proxy", "inert://unapproved"),
        ("gpg.program", "/nonexistent/unapproved"),
        ("push.gpgSign", "true"),
        ("push.recurseSubmodules", "on-demand"),
        ("submodule.recurse", "true"),
    ] {
        git(source.path(), &["config", key, value]);
        let mut reached_push = false;
        assert_eq!(
            publish_locked(&request, || reached_push = true),
            PublicationOutcome::Refused,
            "{key}"
        );
        assert!(!reached_push, "{key}");
        assert!(!crate::test_support::branch_exists(
            &remote,
            "checkpoint/config"
        ));
        git(source.path(), &["config", "--unset", key]);
    }
    git(
        source.path(),
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        source.path(),
        &[
            "config",
            "--worktree",
            "remote.origin.receivepack",
            "/nonexistent/unapproved",
        ],
    );
    let mut reached_push = false;
    assert_eq!(
        publish_locked(&request, || reached_push = true),
        PublicationOutcome::Refused
    );
    assert!(!reached_push);
    git(
        source.path(),
        &[
            "config",
            "--worktree",
            "--unset",
            "remote.origin.receivepack",
        ],
    );
    assert_eq!(publish(&request), PublicationOutcome::Published);
}
