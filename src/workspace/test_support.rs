//! Shared deterministic Git fixtures for Workspace-Execution tests.
//!
//! Every helper builds throwaway Git repositories under the system
//! temporary directory (never inside this repository), configures a local
//! repository Git identity so nothing depends on any developer's global
//! identity, and cleans up on drop. All fixture Git invocation uses
//! explicit argv through [`std::process::Command`]; no shell command string
//! exists anywhere here. No network remote is ever configured or contacted:
//! every operation is purely local.
//!
//! Fixture commands stay immune to hostile variables that environment tests
//! may install into this process while other tests run in parallel: a fixed
//! list of Git-control variables is stripped from every fixture invocation,
//! and hermetic configuration pins are applied last so they can never be
//! stripped by that sweep.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Every Git-control variable any suite in this crate may install into the
/// process environment while other tests run in parallel. Fixture commands
/// strip these unconditionally — a static list closes the race where a
/// parallel test installs a hostile value after the fixture has snapshotted
/// the current environment.
pub(crate) const HOSTILE_GIT_CONTROL_KEYS: [&str; 25] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_KEY_1",
    "GIT_CONFIG_KEY_2",
    "GIT_CONFIG_KEY_3",
    "GIT_CONFIG_VALUE_0",
    "GIT_CONFIG_VALUE_1",
    "GIT_CONFIG_VALUE_2",
    "GIT_CONFIG_VALUE_3",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CEILING_DIRECTORIES",
    "GIT_EXEC_PATH",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_TERMINAL_PROMPT",
];

/// A temporary directory removed on drop.
pub(crate) struct TempDir {
    root: PathBuf,
}

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "receipts-workspace-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("temporary test directory creation");
        Self { root }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A throwaway local Git repository with one seeded empty commit.
pub(crate) struct TestRepo {
    /// Owning temporary directory removed on drop (may be a parent of the
    /// repository directory itself).
    pub(crate) root: TempDir,
    /// The directory holding the `.git` repository.
    repo_dir: PathBuf,
}

impl TestRepo {
    /// A repository seeded directly in its own temporary root.
    pub(crate) fn new(tag: &str) -> Self {
        Self::seeded(TempDir::new(tag), None)
    }

    /// A repository seeded one level below its temporary root, so sibling
    /// paths (for example symlink aliases) can live next to it.
    pub(crate) fn new_nested(tag: &str, repo_subdir: &str) -> Self {
        Self::seeded(TempDir::new(tag), Some(repo_subdir))
    }

    fn seeded(root: TempDir, repo_subdir: Option<&str>) -> Self {
        let repo_dir = match repo_subdir {
            Some(subdir) => root.path().join(subdir),
            None => root.path().to_path_buf(),
        };
        std::fs::create_dir_all(&repo_dir).expect("repository directory creation");
        git(&repo_dir, &["init", "--quiet", "--initial-branch", "main"]);
        git(
            &repo_dir,
            &["config", "user.name", "Receipts Workspace Tests"],
        );
        git(
            &repo_dir,
            &["config", "user.email", "workspace-tests@receipts.invalid"],
        );
        git(&repo_dir, &["config", "commit.gpgsign", "false"]);
        git(
            &repo_dir,
            &["commit", "--quiet", "--allow-empty", "--message", "seed"],
        );
        Self { root, repo_dir }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.repo_dir
    }

    /// The repository's current HEAD commit SHA.
    pub(crate) fn head_sha(&self) -> String {
        stdout_trimmed(&git(self.path(), &["rev-parse", "HEAD"]))
    }

    /// Writes a tracked file at `relative` inside the repository's working
    /// tree, commits it with the repository-local identity, and returns the
    /// resulting HEAD commit SHA.
    pub(crate) fn commit_file(&self, relative: &str, content: &str) -> String {
        let target = self.repo_dir.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create parent directory for tracked file");
        }
        std::fs::write(target, content).expect("write tracked file");
        git(self.path(), &["add", relative]);
        git(
            self.path(),
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "--message",
                &format!("track {relative}"),
            ],
        );
        self.head_sha()
    }
}

pub(crate) fn dev_null() -> &'static Path {
    Path::new("/dev/null")
}

/// Runs one fixture Git command with explicit argv and hermetic config,
/// asserting success.
pub(crate) fn git(directory: &Path, args: &[&str]) -> Output {
    let output = git_raw(directory, args);
    assert!(
        output.status.success(),
        "fixture git command {args:?} failed: {}",
        stderr_text(&output)
    );
    output
}

pub(crate) fn git_raw(directory: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    // Fixture commands must stay immune to the hostile variables that
    // environment tests install in this process while other tests run in
    // parallel: no ambient Git-control variable may influence any fixture.
    // The hermetic configuration pins are applied last so they can never be
    // stripped by this sweep.
    for key in HOSTILE_GIT_CONTROL_KEYS {
        command.env_remove(key);
    }
    command
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dev_null())
        .env("GIT_CONFIG_SYSTEM", dev_null())
        .args(args)
        .output()
        .expect("git executable must be available for workspace tests")
}

pub(crate) fn stdout_trimmed(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(crate) fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// Whether the given branch exists as a local ref in the repository.
pub(crate) fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    git_raw(
        repo.path(),
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .status
    .success()
}

/// The HEAD commit SHA of a linked worktree directory inside the repository
/// root.
#[allow(dead_code)]
pub(crate) fn worktree_head(repo: &TestRepo, worktree_name: &str) -> String {
    stdout_trimmed(&git(
        &repo.path().join(worktree_name),
        &["rev-parse", "HEAD"],
    ))
}

pub(crate) fn porcelain_status(worktree: &Path) -> String {
    stdout_trimmed(&git_raw(worktree, &["status", "--porcelain"]))
}

/// The raw `git worktree list --porcelain` output of the repository.
pub(crate) fn worktree_list_porcelain(repo: &TestRepo) -> String {
    stdout_trimmed(&git_raw(repo.path(), &["worktree", "list", "--porcelain"]))
}

/// Whether a worktree checkout path still appears among the repository's
/// registered worktrees. Registration paths are compared canonically
/// because Git may report the physically resolved location while the test
/// holds a symlink-aliased spelling of the same checkout.
pub(crate) fn worktree_registered(repo: &TestRepo, worktree_path: &Path) -> bool {
    let expected = std::fs::canonicalize(worktree_path).ok();
    worktree_list_porcelain(repo).lines().any(|line| {
        line.strip_prefix("worktree ").is_some_and(|reported| {
            if let Some(expected) = &expected
                && let Ok(resolved) = std::fs::canonicalize(reported)
            {
                return resolved == *expected;
            }
            Path::new(reported) == worktree_path
        })
    })
}

pub(crate) fn unix_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("test symlink creation");
}

/// Locates the Git administrative directory that registers the linked
/// worktree checkout at `worktree_path` (`<repo>/.git/worktrees/<id>`), by
/// reading each candidate's recorded checkout location. Recorded locations
/// are compared canonically, since Git may store the physically resolved
/// spelling of the checkout path.
pub(crate) fn worktree_meta_dir(repo: &TestRepo, worktree_path: &Path) -> Option<PathBuf> {
    let registrations = repo.path().join(".git").join("worktrees");
    let expected_gitfile = std::fs::canonicalize(worktree_path.join(".git")).ok()?;
    for entry in std::fs::read_dir(&registrations).ok()?.flatten() {
        let gitdir = entry.path().join("gitdir");
        let Ok(recorded) = std::fs::read_to_string(&gitdir) else {
            continue;
        };
        if std::fs::canonicalize(recorded.trim()).is_ok_and(|resolved| resolved == expected_gitfile)
        {
            return Some(entry.path());
        }
    }
    None
}
