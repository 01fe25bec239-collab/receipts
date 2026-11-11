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
    assert_eq!(publish(&request), PublicationOutcome::LocalOnly);
}
