use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use super::*;
use crate::provision::WorkspaceProvisionRequest;
use crate::test_support::TestRepo;

fn sample() -> TaskBinding {
    TaskBinding {
        repository: PathBuf::from(std::ffi::OsString::from_vec(b"/r\xff/repo".to_vec())),
        worktree: PathBuf::from("/r/wt"),
        branch: "task/ü".into(),
        base_sha: CommitSha::parse(&"ab".repeat(20)).unwrap(),
        workspace_id: "ws 🦀".into(),
        task_id: "task-1".into(),
    }
}

fn provisioned(repo: &TestRepo) -> (TaskBinding, PathBuf) {
    let base = repo.head_sha();
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-1",
        Some("task-1"),
        "task/b",
        repo.path().join("wt"),
        &base,
    )
    .unwrap()
    .provision()
    .unwrap();
    let root = std::fs::canonicalize(repo.path()).unwrap();
    let binding = TaskBinding {
        repository: root.clone(),
        worktree: handle.worktree_path().to_path_buf(),
        branch: "task/b".into(),
        base_sha: CommitSha::parse(&base).unwrap(),
        workspace_id: "ws-1".into(),
        task_id: "task-1".into(),
    };
    let file = admin_dir(&root, &binding.worktree)
        .unwrap()
        .join(BINDING_FILE);
    (binding, file)
}

fn overwrite(file: &Path, bytes: &[u8]) {
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(file, bytes).unwrap();
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o400)).unwrap();
}

#[test]
fn encoding_roundtrips_exact_bytes_including_non_utf8_paths_and_unicode_ids() {
    let binding = sample();
    let bytes = binding.encode();
    assert_eq!(TaskBinding::decode(&bytes).unwrap(), binding);
    assert_eq!(binding.encode(), bytes, "deterministic");
    assert!(bytes.starts_with(b"receipts-workspace-binding/v1\n"));
}

#[test]
fn decoder_refuses_every_noncanonical_or_damaged_record() {
    let good = sample().encode();
    let text = String::from_utf8(good.clone()).unwrap();
    let mut cases: Vec<Vec<u8>> = vec![
        vec![],
        good[..good.len() - 1].to_vec(),
        good[..good.len() / 2].to_vec(),
        [good.clone(), b"extra=1\n".to_vec()].concat(),
        [good.clone(), good.clone()].concat(),
        text.replace("/v1", "/v2").into_bytes(),
        text.replace("branch=", "branc=").into_bytes(),
        text.replace("task=", "TASK=").into_bytes(),
        text.to_uppercase().into_bytes(),
        text.replace("workspace=", "workspace=zz").into_bytes(),
        text.replace('\n', "\r\n").into_bytes(),
        vec![0xff, 0xfe],
    ];
    // One flipped digest nibble, one flipped field nibble.
    let digest_at = text.rfind("sha256=").unwrap() + "sha256=".len();
    let mut digest_flip = good.clone();
    digest_flip[digest_at] = if digest_flip[digest_at] == b'0' {
        b'1'
    } else {
        b'0'
    };
    cases.push(digest_flip);
    let base_at = text.find("base=").unwrap() + "base=".len();
    let mut field_flip = good.clone();
    field_flip[base_at] = if field_flip[base_at] == b'0' {
        b'1'
    } else {
        b'0'
    };
    cases.push(field_flip);
    for case in cases {
        assert!(
            TaskBinding::decode(&case).is_err(),
            "{:?}",
            String::from_utf8_lossy(&case)
        );
    }
}

#[test]
fn provisioning_records_a_private_binding_the_verifier_accepts() {
    let repo = TestRepo::new("bind-ok");
    let (binding, file) = provisioned(&repo);
    let meta = std::fs::symlink_metadata(&file).unwrap();
    assert!(meta.is_file() && meta.mode() & 0o777 == 0o400);
    assert_eq!(std::fs::read(&file).unwrap(), binding.encode());
    verify(&binding.repository, &binding).unwrap();
}

#[test]
fn verifier_names_each_mismatched_field() {
    let repo = TestRepo::new("bind-fields");
    let (binding, _) = provisioned(&repo);
    let root = &binding.repository;
    let cases: [(&str, TaskBinding); 5] = [
        (
            "branch",
            TaskBinding {
                branch: "task/other".into(),
                ..binding.clone()
            },
        ),
        (
            "base",
            TaskBinding {
                base_sha: CommitSha::parse(&"cd".repeat(20)).unwrap(),
                ..binding.clone()
            },
        ),
        (
            "workspace",
            TaskBinding {
                workspace_id: "ws-2".into(),
                ..binding.clone()
            },
        ),
        (
            "task",
            TaskBinding {
                task_id: "task-2".into(),
                ..binding.clone()
            },
        ),
        (
            "repository",
            TaskBinding {
                repository: "/elsewhere".into(),
                ..binding.clone()
            },
        ),
    ];
    for (field, wrong) in cases {
        let error = verify(root, &wrong).unwrap_err();
        assert!(
            error.contains(field) && error.contains("differs"),
            "{field}: {error}"
        );
    }
}

#[test]
fn missing_corrupt_swapped_and_unauthenticated_records_refuse_without_repair() {
    let repo = TestRepo::new("bind-bad");
    let (binding, file) = provisioned(&repo);
    let root = &binding.repository;
    let good = std::fs::read(&file).unwrap();

    // Corrupt content (digest no longer matches) is refused and left as found.
    let mut corrupt = good.clone();
    let at = corrupt.len() - 3;
    corrupt[at] = if corrupt[at] == b'0' { b'1' } else { b'0' };
    overwrite(&file, &corrupt);
    assert!(verify(root, &binding).unwrap_err().contains("digest"));
    assert_eq!(std::fs::read(&file).unwrap(), corrupt);

    // Truncated to nothing.
    overwrite(&file, b"");
    assert!(verify(root, &binding).unwrap_err().contains("malformed"));

    // Group/other-writable is unauthenticated even with intact bytes.
    overwrite(&file, &good);
    verify(root, &binding).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o620)).unwrap();
    assert!(
        verify(root, &binding)
            .unwrap_err()
            .contains("unauthenticated")
    );
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).unwrap();

    // A symlink (even to an identical copy) is not a regular file.
    let copy = file.with_file_name("copy");
    std::fs::write(&copy, &good).unwrap();
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o400)).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&copy, &file).unwrap();
    assert!(verify(root, &binding).unwrap_err().contains("regular file"));
    std::fs::remove_file(&file).unwrap();

    // Missing is refused and never created.
    assert!(verify(root, &binding).unwrap_err().contains("missing"));
    assert!(!file.exists());
}

#[test]
fn an_owner_other_than_the_trusted_identity_or_root_is_unauthenticated() {
    let repo = TestRepo::new("bind-owner");
    let (binding, file) = provisioned(&repo);
    let owner = std::fs::metadata(&file).unwrap().uid();
    if owner == 0 {
        return; // Root-owned records are always trusted.
    }
    verify_as(&binding.repository, &binding, owner).unwrap();
    assert!(
        verify_as(&binding.repository, &binding, owner + 1)
            .unwrap_err()
            .contains("not owned by the trusted identity")
    );
}

#[test]
fn an_existing_marker_is_never_overwritten() {
    let repo = TestRepo::new("bind-once");
    let (binding, file) = provisioned(&repo);
    let before = std::fs::read(&file).unwrap();
    let other = TaskBinding {
        task_id: "task-9".into(),
        ..binding.clone()
    };
    assert!(matches!(
        record(&binding.repository, &other),
        Err(WorkspaceError::WorktreeBindingFailed { .. })
    ));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn admin_dir_is_found_from_the_repository_side_not_the_worktree_pointer() {
    let repo = TestRepo::new("bind-pointer");
    let (binding, file) = provisioned(&repo);
    let admin = file.parent().unwrap().to_path_buf();
    // Redirect the worktree's own pointer elsewhere: lookup is unaffected.
    std::fs::write(binding.worktree.join(".git"), "gitdir: /nonexistent\n").unwrap();
    assert_eq!(
        admin_dir(&binding.repository, &binding.worktree).unwrap(),
        admin
    );
    // An unregistered path has no administrative directory.
    assert!(admin_dir(&binding.repository, &binding.repository.join("nowhere")).is_err());
}

#[test]
fn provisioning_without_task_context_records_no_binding() {
    let repo = TestRepo::new("bind-plain");
    let handle = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-1",
        None,
        "task/plain",
        repo.path().join("wt"),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap();
    let root = std::fs::canonicalize(repo.path()).unwrap();
    assert!(!is_task_bound(&root, handle.worktree_path()).unwrap());
}
