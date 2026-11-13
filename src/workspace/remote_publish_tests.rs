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

struct PublicationClock;
impl receipts_state::TrustedClockV1 for PublicationClock {
    fn sample(&self) -> Result<receipts_state::TrustedTimeSampleV1, receipts_state::StateError> {
        Ok(receipts_state::TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-11-12T00:00:00.000000000Z".into(),
            clock_source_id: "test".into(),
            clock_contract_version: "1".into(),
        })
    }
}
fn publication_admission(
    store: &receipts_state::TrustedCoreStore,
) -> receipts_state::StateWriteAdmission {
    store.admit(receipts_state::StateWriteSubjectV1 {
        operation: receipts_state::StateWriteOperationV1::RemotePublication,
        project_id: "p".into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
}
#[test]
fn durable_publication_reopens_reconciles_lost_response_and_retries_nonforce() {
    use receipts_state::{
        RemotePublicationIntentV1, RemotePublicationOutcomeV1 as O, TrustedCoreStore,
    };
    let source = TestRepo::new("d12-source");
    let remote = TestRepo::new("d12-remote");
    let state_dir = source.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let db = state_dir.join("publication.sqlite");
    git(source.path(), &["branch", "checkpoint/d12"]);
    let sha = source.head_sha();
    let endpoint = remote.path().to_str().unwrap();
    let request = PublicationRequest {
        repository: source.path(),
        remote: endpoint,
        branch: "checkpoint/d12",
        commit: &sha,
        wire: W::PushAlways,
        mode: ProjectRemoteMode::PushAllCheckpoints,
        class: PublicationBranchClass::Checkpoint,
        accepted_commit: None,
    };
    let intent = RemotePublicationIntentV1 {
        project_id: "p".into(),
        operation_id: "operation".into(),
        repository: std::fs::canonicalize(source.path())
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        remote: std::fs::canonicalize(remote.path())
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        source_ref: "refs/heads/checkpoint/d12".into(),
        destination_ref: "refs/heads/checkpoint/d12".into(),
        commit: sha.clone(),
        wire: "PUSH_ALWAYS".into(),
        mode: "PUSH_ALL_CHECKPOINTS".into(),
        branch_class: "CHECKPOINT".into(),
        acceptance_ref: None,
        deletion_authorization: None,
    };
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let a = publication_admission(&store);
    store.reserve_remote_publication_v1(a, &intent).unwrap();
    // Simulate process interruption after durable intent, before any response.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    assert_eq!(
        store
            .reader()
            .read_remote_publication_v1("p", "operation")
            .unwrap()
            .unwrap()
            .outcome(),
        O::Uncertain
    );
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
        O::Published
    );
    // The remote accepted the push, but its response/outcome append was lost.
    let mut lost = intent.clone();
    lost.operation_id = "lost-response".into();
    let a = publication_admission(&store);
    store.reserve_remote_publication_v1(a, &lost).unwrap();
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &lost, &request, &PublicationClock).unwrap(),
        O::Published
    );
    let history = store
        .reader()
        .read_remote_publication_v1("p", "lost-response")
        .unwrap()
        .unwrap();
    assert_eq!(history.results.len(), 1);
    assert_eq!(history.results[0].evidence_ref, "d12:exact-remote-readback");
    let mut protected = intent.clone();
    protected.operation_id = "protected".into();
    protected.source_ref = "refs/heads/main".into();
    protected.destination_ref = protected.source_ref.clone();
    let protected_request = PublicationRequest {
        branch: "main",
        ..request
    };
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(
            &mut store,
            a,
            &protected,
            &protected_request,
            &PublicationClock
        )
        .unwrap(),
        O::Refused
    );
    let mut accepted = intent.clone();
    accepted.operation_id = "acceptance".into();
    accepted.wire = "PUSH_ON_ACCEPT".into();
    accepted.mode = "PUSH_ACCEPTED_A3".into();
    accepted.branch_class = "TASK".into();
    accepted.acceptance_ref = Some("acceptance:exact-sha".into());
    let mut accepted_request = PublicationRequest {
        wire: W::PushOnAccept,
        mode: ProjectRemoteMode::PushAcceptedA3,
        class: PublicationBranchClass::Task,
        ..request
    };
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(
            &mut store,
            a,
            &accepted,
            &accepted_request,
            &PublicationClock
        )
        .unwrap(),
        O::Refused
    );
    accepted_request.accepted_commit = Some(&sha);
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(
            &mut store,
            a,
            &accepted,
            &accepted_request,
            &PublicationClock
        )
        .unwrap(),
        O::Published
    );
    // Divergent existing destination cannot be overwritten by retry.
    git(
        remote.path(),
        &["update-ref", "refs/heads/checkpoint/d12", "HEAD"],
    );
    git(remote.path(), &["checkout", "-b", "diverge"]);
    std::fs::write(remote.path().join("divergent"), "remote").unwrap();
    git(remote.path(), &["add", "divergent"]);
    git(remote.path(), &["commit", "-m", "divergent"]);
    let divergent = remote.head_sha();
    git(
        remote.path(),
        &["update-ref", "refs/heads/checkpoint/d12", &divergent],
    );
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
        O::RemoteFailed
    );
    assert_eq!(
        crate::test_support::stdout_trimmed(&git(
            remote.path(),
            &["rev-parse", "refs/heads/checkpoint/d12"]
        )),
        divergent
    );
    // A read failure remains uncertain after reopen, even after a prior result.
    git(source.path(), &["config", "remote.fake.url", "/unused"]);
    let moved = remote.path().with_extension("unavailable");
    std::fs::rename(remote.path(), &moved).unwrap();
    std::fs::create_dir(remote.path()).unwrap();
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
        O::Uncertain
    );
    std::fs::remove_dir(remote.path()).unwrap();
    std::fs::rename(&moved, remote.path()).unwrap();
    git(
        source.path(),
        &["config", "remote.fake.receivepack", "blocked"],
    );
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
        O::Uncertain
    );
    drop(store);
    let store = TrustedCoreStore::open_exclusive(&db).unwrap();
    assert_eq!(
        store
            .reader()
            .read_remote_publication_v1("p", "operation")
            .unwrap()
            .unwrap()
            .outcome(),
        O::Uncertain
    );
}

fn transport_intent(
    source: &TestRepo,
    endpoint: &str,
    sha: &str,
    operation: &str,
) -> receipts_state::RemotePublicationIntentV1 {
    receipts_state::RemotePublicationIntentV1 {
        project_id: "p".into(),
        operation_id: operation.into(),
        repository: std::fs::canonicalize(source.path())
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        remote: if endpoint.starts_with('/') {
            std::fs::canonicalize(endpoint)
                .unwrap()
                .to_str()
                .unwrap()
                .into()
        } else {
            endpoint.into()
        },
        source_ref: "refs/heads/task/transport".into(),
        destination_ref: "refs/heads/task/transport".into(),
        commit: sha.into(),
        wire: "PUSH_ON_ACCEPT".into(),
        mode: "PUSH_ACCEPTED_A3".into(),
        branch_class: "TASK".into(),
        acceptance_ref: Some("acceptance:exact".into()),
        deletion_authorization: None,
    }
}
fn transport_store(source: &TestRepo) -> (std::path::PathBuf, receipts_state::TrustedCoreStore) {
    let dir = source.path().join("private-state");
    std::fs::create_dir(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let db = dir.join("state.sqlite");
    let store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
    (db, store)
}

#[test]
fn network_selection_durable_replay_refusal_and_user_deletion() {
    use receipts_state::RemotePublicationOutcomeV1 as O;
    use std::{cell::RefCell, rc::Rc};
    for endpoint in [
        "https://fixture.invalid/repo.git",
        "ssh://git@fixture.invalid/repo.git",
    ] {
        let source = TestRepo::new("network-publication");
        git(source.path(), &["branch", "task/transport"]);
        let sha = source.head_sha();
        let (db, mut store) = transport_store(&source);
        let config = source.path().join("trusted-config");
        std::fs::write(&config, "").unwrap();
        let settings = TrustedTransportSettings {
            git_config: &config,
            home: None,
            ssh_auth_sock: None,
        };
        let request = PublicationRequest {
            repository: source.path(),
            remote: endpoint,
            branch: "task/transport",
            commit: &sha,
            wire: W::PushOnAccept,
            mode: ProjectRemoteMode::PushAcceptedA3,
            class: PublicationBranchClass::Task,
            accepted_commit: Some(&sha),
        };
        let intent = transport_intent(&source, endpoint, &sha, "publish");
        let remote = Rc::new(RefCell::new(None::<String>));
        let calls = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let fixture_remote = remote.clone();
        let fixture_calls = calls.clone();
        let fixture_sha = sha.clone();
        let _fixture = crate::git::publication_fixture(Box::new(move |request| {
            let args: Vec<_> = request
                .arguments()
                .iter()
                .map(|v| v.to_str().unwrap().to_owned())
                .collect();
            if !args.iter().any(|v| v == "push" || v == "ls-remote") {
                return None;
            }
            assert!(
                request
                    .fixed_env()
                    .iter()
                    .any(|(key, _)| *key == "GIT_CONFIG_GLOBAL")
            );
            assert!(
                !request
                    .fixed_env()
                    .iter()
                    .any(|(key, _)| *key == "GIT_SSH_COMMAND")
            );
            fixture_calls.borrow_mut().push(args.clone());
            if args.iter().any(|v| v == "ls-remote") {
                let output = fixture_remote
                    .borrow()
                    .as_ref()
                    .map(|sha| format!("{sha}\trefs/heads/task/transport\n"))
                    .unwrap_or_default();
                return Some(Ok(crate::git::fixture_capture(true, output)));
            }
            if args.last().unwrap() == ":refs/heads/task/transport" {
                assert!(args.contains(&format!(
                    "--force-with-lease=refs/heads/task/transport:{fixture_sha}"
                )));
                *fixture_remote.borrow_mut() = None;
            } else {
                assert!(
                    !args
                        .iter()
                        .any(|v| v.starts_with("--force") || v.starts_with('+'))
                );
                *fixture_remote.borrow_mut() = Some(fixture_sha.clone());
            }
            Some(Ok(crate::git::fixture_capture(true, String::new())))
        }));
        // Network transports require explicit trusted settings.
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
            O::Refused
        );
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable_with_transport(
                &mut store,
                a,
                &intent,
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Published
        );
        let expected_protocol = if endpoint.starts_with("https") {
            "protocol.https.allow=always"
        } else {
            "protocol.ssh.allow=always"
        };
        assert!(
            calls
                .borrow()
                .iter()
                .all(|args| args.iter().any(|v| v == expected_protocol))
        );
        // Endpoint changes cannot replay an existing operation ID.
        let mut changed = intent.clone();
        changed.remote.push_str("/other");
        let a = publication_admission(&store);
        assert!(store.reserve_remote_publication_v1(a, &changed).is_err());
        // Repository overrides refuse before remote execution.
        let count = calls.borrow().len();
        git(source.path(), &["config", "core.sshCommand", "false"]);
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable_with_transport(
                &mut store,
                a,
                &intent,
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Refused
        );
        assert_eq!(calls.borrow().len(), count);
        git(source.path(), &["config", "--unset", "core.sshCommand"]);
        let shadow = format!("remote.{endpoint}.url");
        git(
            source.path(),
            &["config", &shadow, "https://other.invalid/repo.git"],
        );
        let count = calls.borrow().len();
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable_with_transport(
                &mut store,
                a,
                &intent,
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Refused
        );
        assert_eq!(
            calls.borrow().len(),
            count,
            "endpoint-name shadowing must refuse before remote access"
        );
        git(source.path(), &["config", "--unset", &shadow]);
        // Reconcile publication after the refusal; authorization checks complete history.
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable_with_transport(
                &mut store,
                a,
                &intent,
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Published
        );
        let mut deletion = intent.clone();
        deletion.operation_id = "user-delete".into();
        deletion.deletion_authorization = Some("user-delete".into());
        let a = publication_admission(&store);
        let authorization = store.authorize_remote_deletion_v1(a, &deletion).unwrap();
        // A pending deletion excludes even replay of a prior publication.
        let a = publication_admission(&store);
        assert!(
            publish_durable_with_transport(
                &mut store,
                a,
                &intent,
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .is_err()
        );
        assert_eq!(
            delete_remote_branch(
                &mut store,
                authorization,
                &request,
                &[],
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Deleted
        );
        let count = calls.borrow().len();
        drop(store);
        let mut store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
        let a = publication_admission(&store);
        assert!(store.authorize_remote_deletion_v1(a, &deletion).is_err());
        assert_eq!(
            reconcile_remote_deletion(
                &mut store,
                "p",
                "user-delete",
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            O::Deleted
        );
        assert_eq!(calls.borrow().len(), count);
        assert_eq!(source.head_sha(), sha);
    }
}

#[test]
fn deletion_retains_evidence_refuses_movement_and_reconciles_lost_response() {
    use receipts_state::RemotePublicationOutcomeV1 as O;
    for case in [
        "delete",
        "moved",
        "pending",
        "rejected",
        "protected",
        "lost",
        "unused",
        "lock",
        "symbolic",
    ] {
        let source = TestRepo::new(&format!("delete-{case}"));
        let remote = TestRepo::new(&format!("delete-remote-{case}"));
        git(source.path(), &["branch", "task/transport"]);
        let sha = source.head_sha();
        let endpoint = remote.path().to_str().unwrap();
        let (db, mut store) = transport_store(&source);
        let request = PublicationRequest {
            repository: source.path(),
            remote: endpoint,
            branch: "task/transport",
            commit: &sha,
            wire: W::PushOnAccept,
            mode: ProjectRemoteMode::PushAcceptedA3,
            class: PublicationBranchClass::Task,
            accepted_commit: Some(&sha),
        };
        let intent = transport_intent(&source, endpoint, &sha, "original");
        let a = publication_admission(&store);
        assert_eq!(
            publish_durable(&mut store, a, &intent, &request, &PublicationClock).unwrap(),
            O::Published
        );
        if matches!(case, "pending" | "rejected") {
            let mut extra = intent.clone();
            extra.operation_id = "older-evidence".into();
            if case == "rejected" {
                extra.wire = "PUSH_ALWAYS".into();
                extra.mode = "PUSH_ALL_CHECKPOINTS".into();
                extra.branch_class = "CHECKPOINT".into();
                extra.acceptance_ref = None;
            }
            let a = publication_admission(&store);
            store.reserve_remote_publication_v1(a, &extra).unwrap();
            if case == "rejected" {
                let a = publication_admission(&store);
                store
                    .append_remote_publication_result_v1(
                        a,
                        "p",
                        "older-evidence",
                        1,
                        O::Published,
                        Some(&sha),
                        "d12:rejected-checkpoint",
                        &PublicationClock,
                    )
                    .unwrap();
            }
        }
        let mut deletion = intent.clone();
        deletion.operation_id = "direction".into();
        deletion.deletion_authorization = Some("direction".into());
        let a = publication_admission(&store);
        let authorization = store.authorize_remote_deletion_v1(a, &deletion);
        if matches!(case, "pending" | "rejected") {
            assert!(authorization.is_err(), "{case}");
            continue;
        }
        let authorization = authorization.unwrap();
        // State's exclusive writer remains held while the remote is reconciled.
        assert!(receipts_state::TrustedCoreStore::open_exclusive(&db).is_err());
        if case == "symbolic" {
            git(remote.path(), &["update-ref", "refs/heads/main", &sha]);
            git(
                remote.path(),
                &[
                    "symbolic-ref",
                    "refs/heads/task/transport",
                    "refs/heads/main",
                ],
            );
        }
        if case == "moved" {
            let changed = remote.commit_file("moved", "new remote commit");
            git(
                remote.path(),
                &["update-ref", "refs/heads/task/transport", &changed],
            );
        }
        if case == "lock" {
            std::fs::write(
                source.path().join(".git/refs/heads/task/transport.lock"),
                "held",
            )
            .unwrap();
        }
        if matches!(case, "lost" | "unused") {
            if case == "lost" {
                let a = publication_admission(&store);
                store
                    .append_remote_publication_result_v1(
                        a,
                        "p",
                        "direction",
                        1,
                        O::Uncertain,
                        None,
                        "d12:delete-started",
                        &PublicationClock,
                    )
                    .unwrap();
            }
            git(
                remote.path(),
                &["update-ref", "-d", "refs/heads/task/transport"],
            );
            drop(authorization);
            drop(store);
            let mut store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
            assert_eq!(
                reconcile_remote_deletion(
                    &mut store,
                    "p",
                    "direction",
                    &request,
                    &PublicationClock,
                    None
                )
                .unwrap(),
                if case == "lost" {
                    O::Deleted
                } else {
                    O::Refused
                }
            );
            continue;
        }
        let protected = if case == "protected" {
            vec!["refs/heads/task/transport"]
        } else {
            vec![]
        };
        assert_eq!(
            delete_remote_branch(
                &mut store,
                authorization,
                &request,
                &protected,
                &PublicationClock,
                None
            )
            .unwrap(),
            if case == "delete" {
                O::Deleted
            } else if case == "symbolic" {
                O::Uncertain
            } else {
                O::Refused
            },
            "{case}"
        );
        assert_eq!(
            crate::test_support::branch_exists(&remote, "task/transport"),
            case != "delete"
        );
        assert!(crate::test_support::branch_exists(
            &source,
            "task/transport"
        ));
        if case == "symbolic" {
            assert!(crate::test_support::branch_exists(&remote, "main"));
        }
    }
}

#[test]
fn deletion_lost_transport_response_and_receive_side_movement() {
    use receipts_state::RemotePublicationOutcomeV1 as O;
    use std::{cell::RefCell, rc::Rc};
    for race in [false, true] {
        let source = TestRepo::new("delete-network-loss");
        let sha = source.head_sha();
        let endpoint = "ssh://git@fixture.invalid/repo.git";
        let (db, mut store) = transport_store(&source);
        let config = source.path().join("trusted-config");
        std::fs::write(&config, "").unwrap();
        let settings = TrustedTransportSettings {
            git_config: &config,
            home: None,
            ssh_auth_sock: None,
        };
        let request = PublicationRequest {
            repository: source.path(),
            remote: endpoint,
            branch: "task/transport",
            commit: &sha,
            wire: W::PushOnAccept,
            mode: ProjectRemoteMode::PushAcceptedA3,
            class: PublicationBranchClass::Task,
            accepted_commit: Some(&sha),
        };
        let mut deletion = transport_intent(&source, endpoint, &sha, "delete-loss");
        deletion.deletion_authorization = Some("delete-loss".into());
        let a = publication_admission(&store);
        let authorization = store.authorize_remote_deletion_v1(a, &deletion).unwrap();
        // (remote SHA, fail next read, number of destructive commands)
        let state = Rc::new(RefCell::new((Some(sha.clone()), false, 0)));
        let fixture_state = state.clone();
        let fixture_sha = sha.clone();
        let _fixture = crate::git::publication_fixture(Box::new(move |request| {
            let args: Vec<_> = request
                .arguments()
                .iter()
                .map(|a| a.to_str().unwrap())
                .collect();
            if args.contains(&"push") {
                let mut state = fixture_state.borrow_mut();
                state.2 += 1;
                assert!(args.contains(
                    &format!("--force-with-lease=refs/heads/task/transport:{fixture_sha}").as_str()
                ));
                // A moved ref rejects the lease; a lost response may hide success.
                state.0 = if race { Some("b".repeat(40)) } else { None };
                state.1 = !race;
                return Some(Err(()));
            }
            if args.contains(&"ls-remote") {
                let mut state = fixture_state.borrow_mut();
                if state.1 {
                    state.1 = false;
                    return Some(Err(()));
                }
                return Some(Ok(crate::git::fixture_capture(
                    true,
                    state
                        .0
                        .as_ref()
                        .map(|s| format!("{s}\trefs/heads/task/transport\n"))
                        .unwrap_or_default(),
                )));
            }
            None
        }));
        assert_eq!(
            delete_remote_branch(
                &mut store,
                authorization,
                &request,
                &[],
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            if race { O::RemoteFailed } else { O::Uncertain }
        );
        assert_eq!(state.borrow().2, 1);
        drop(store);
        let mut store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
        assert_eq!(
            reconcile_remote_deletion(
                &mut store,
                "p",
                "delete-loss",
                &request,
                &PublicationClock,
                Some(&settings)
            )
            .unwrap(),
            if race { O::RemoteFailed } else { O::Deleted }
        );
        assert_eq!(
            state.borrow().2,
            1,
            "reconciliation must never repeat deletion"
        );
    }
}

#[test]
fn uncertain_deletion_configuration_refusal_keeps_competitors_blocked() {
    uncertain_deletion_local_refusal("config");
}

#[test]
fn uncertain_deletion_branch_lock_keeps_competitors_blocked() {
    uncertain_deletion_local_refusal("lock");
}

fn uncertain_deletion_local_refusal(obstacle: &str) {
    use receipts_state::RemotePublicationOutcomeV1 as O;
    let source = TestRepo::new(&format!("uncertain-delete-{obstacle}"));
    let remote = TestRepo::new(&format!("uncertain-delete-remote-{obstacle}"));
    git(source.path(), &["branch", "task/transport"]);
    let sha = source.head_sha();
    let endpoint = remote.path().to_str().unwrap();
    let (db, mut store) = transport_store(&source);
    let request = PublicationRequest {
        repository: source.path(),
        remote: endpoint,
        branch: "task/transport",
        commit: &sha,
        wire: W::PushOnAccept,
        mode: ProjectRemoteMode::PushAcceptedA3,
        class: PublicationBranchClass::Task,
        accepted_commit: Some(&sha),
    };
    let publication = transport_intent(&source, endpoint, &sha, "original-publication");
    let a = publication_admission(&store);
    assert_eq!(
        publish_durable(&mut store, a, &publication, &request, &PublicationClock).unwrap(),
        O::Published
    );
    let mut deletion = publication.clone();
    deletion.operation_id = "lost-deletion".into();
    deletion.deletion_authorization = Some(deletion.operation_id.clone());
    let a = publication_admission(&store);
    let authorization = store.authorize_remote_deletion_v1(a, &deletion).unwrap();
    let a = publication_admission(&store);
    store
        .append_remote_publication_result_v1(
            a,
            "p",
            "lost-deletion",
            1,
            O::Uncertain,
            None,
            "d12:delete-started",
            &PublicationClock,
        )
        .unwrap();
    // Simulate remote success followed by a lost response and process restart.
    git(
        remote.path(),
        &["update-ref", "-d", "refs/heads/task/transport"],
    );
    drop(authorization);
    drop(store);
    let mut store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
    let lock = source.path().join(".git/refs/heads/task/transport.lock");
    if obstacle == "config" {
        git(source.path(), &["config", "core.sshCommand", "false"]);
    } else {
        std::fs::write(&lock, "fixture-owned lock").unwrap();
    }
    assert_eq!(
        reconcile_remote_deletion(
            &mut store,
            "p",
            "lost-deletion",
            &request,
            &PublicationClock,
            None
        )
        .unwrap(),
        O::Uncertain
    );
    drop(store);
    let mut store = receipts_state::TrustedCoreStore::open_exclusive(&db).unwrap();
    let history = store
        .reader()
        .read_remote_publication_v1("p", "lost-deletion")
        .unwrap()
        .unwrap();
    assert_eq!(history.outcome(), O::Uncertain);
    assert_eq!(history.results.len(), 2);
    assert_eq!(history.results[1].observed_commit, None);
    assert_eq!(
        history.results[1].evidence_ref,
        if obstacle == "config" {
            "d12:deletion-configuration-refused"
        } else {
            "d12:deletion-ref-locked"
        }
    );
    let mut competing_publication = publication.clone();
    competing_publication.operation_id = "competing-publication".into();
    let a = publication_admission(&store);
    assert!(matches!(
        publish_durable(
            &mut store,
            a,
            &competing_publication,
            &request,
            &PublicationClock
        ),
        Err(receipts_state::StateError::SetupRefused {
            reason: receipts_state::SetupRefusalV1::ConflictingReplay
        })
    ));
    let mut competing_deletion = deletion.clone();
    competing_deletion.operation_id = "competing-deletion".into();
    competing_deletion.deletion_authorization = Some(competing_deletion.operation_id.clone());
    let a = publication_admission(&store);
    assert!(matches!(
        store.authorize_remote_deletion_v1(a, &competing_deletion),
        Err(receipts_state::StateError::SetupRefused {
            reason: receipts_state::SetupRefusalV1::ConflictingReplay
        })
    ));
    for operation in ["competing-publication", "competing-deletion"] {
        assert!(
            store
                .reader()
                .read_remote_publication_v1("p", operation)
                .unwrap()
                .is_none()
        );
    }
    if obstacle == "config" {
        git(source.path(), &["config", "--unset", "core.sshCommand"]);
    } else {
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            "fixture-owned lock"
        );
        std::fs::remove_file(&lock).unwrap();
    }
    assert_eq!(
        reconcile_remote_deletion(
            &mut store,
            "p",
            "lost-deletion",
            &request,
            &PublicationClock,
            None
        )
        .unwrap(),
        O::Deleted
    );
    let history = store
        .reader()
        .read_remote_publication_v1("p", "lost-deletion")
        .unwrap()
        .unwrap();
    assert_eq!(history.results.len(), 3);
    assert_eq!(
        history.results[2].evidence_ref,
        "d12:deletion-readback-absent"
    );
    assert_eq!(history.outcome(), O::Deleted);
    assert!(!crate::test_support::branch_exists(
        &remote,
        "task/transport"
    ));
    // A terminal remote observation releases the ref for future publication.
    let a = publication_admission(&store);
    assert_eq!(
        store
            .reserve_remote_publication_v1(a, &competing_publication)
            .unwrap(),
        receipts_state::SetupWriteOutcomeV1::Applied
    );
}
