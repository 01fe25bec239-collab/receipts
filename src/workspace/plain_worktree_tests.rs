use super::*;
use crate::WorktreeCreateRequest;
use crate::test_support::{TestRepo, branch_exists, git, worktree_registered};

fn create(repo: &TestRepo) -> crate::WorktreeCreateResponse {
    WorktreeCreateRequest::new(
        repo.path(),
        repo.path().join("wt"),
        "plain/test",
        &repo.head_sha(),
        None,
    )
    .unwrap()
    .create()
    .unwrap()
}

#[test]
fn fresh_clone_removes_clean_tree_and_retains_branch_without_adopting_attach() {
    let repo = TestRepo::new("plain-owned-clean");
    let response = create(&repo);
    let root = fs::canonicalize(repo.path()).unwrap();
    let attached =
        WorktreeCreateRequest::new(&root, response.path(), "plain/test", &repo.head_sha(), None)
            .unwrap()
            .create()
            .unwrap();
    assert!(attached.plain_ownership().is_none());
    let owned = response.plain_ownership().unwrap().clone();
    drop(response);
    let path = owned.path().to_path_buf();
    owned.remove(&root, &path).unwrap();
    assert!(!path.exists());
    assert!(!worktree_registered(&repo, &path));
    assert!(branch_exists(&repo, "plain/test"));
    assert!(owned.remove(&root, &path).is_err());
}

#[test]
fn dirty_foreign_and_changed_head_targets_are_retained() {
    let repo = TestRepo::new("plain-owned-refusals");
    let response = create(&repo);
    let owned = response.plain_ownership().unwrap();
    let root = fs::canonicalize(repo.path()).unwrap();
    let foreign = TestRepo::new("plain-foreign");
    assert!(owned.remove(foreign.path(), owned.path()).is_err());
    assert!(owned.remove(&root, &root).is_err());
    let evidence = owned.path().join("keep");
    fs::write(&evidence, "evidence").unwrap();
    assert!(owned.remove(&root, owned.path()).is_err());
    assert_eq!(fs::read_to_string(&evidence).unwrap(), "evidence");
    git(owned.path(), &["add", "keep"]);
    git(owned.path(), &["commit", "-m", "preserve"]);
    assert!(owned.remove(&root, owned.path()).is_err());
    assert!(worktree_registered(&repo, owned.path()));
    assert!(evidence.exists());
}

#[test]
fn replaced_gitfile_and_symlink_checkout_refuse_without_removal() {
    let repo = TestRepo::new("plain-owned-replaced");
    let response = create(&repo);
    let owned = response.plain_ownership().unwrap();
    let root = fs::canonicalize(repo.path()).unwrap();
    let gitfile = owned.path().join(".git");
    let bytes = fs::read(&gitfile).unwrap();
    fs::rename(&gitfile, owned.path().join("old-gitfile")).unwrap();
    fs::write(&gitfile, bytes).unwrap();
    assert!(owned.verify().is_err());
    assert!(owned.remove(&root, owned.path()).is_err());
    assert!(gitfile.exists());
    let moved = root.join("moved");
    fs::rename(owned.path(), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, owned.path()).unwrap();
    assert!(owned.remove(&root, owned.path()).is_err());
    assert!(moved.join(".git").exists());
}

#[test]
fn in_place_pointer_edits_refuse_and_preserve_the_checkout() {
    for index in [2, 4, 5] {
        let repo = TestRepo::new("plain-pointer-edit");
        let response = create(&repo);
        let owned = response.plain_ownership().unwrap();
        let path = &owned.pins.paths[index];
        let mut bytes = fs::read(path).unwrap();
        bytes.push(b' ');
        fs::write(path, &bytes).unwrap();
        assert!(owned.verify().is_err());
        let root = fs::canonicalize(repo.path()).unwrap();
        assert!(owned.remove(&root, owned.path()).is_err());
        assert!(owned.path().is_dir());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
