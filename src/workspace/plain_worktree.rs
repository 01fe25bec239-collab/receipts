//! Live ownership of a freshly created plain worktree. No path-based adoption.
//! Pins prevent inode reuse; cleanup rechecks registration, HEAD and dirt through
//! the existing non-force teardown. Branches and all refused targets are retained.
//! Git operations retain Workspace's sequential same-user custody assumptions.
use crate::worktree_binding;
use crate::{CommitSha, WorkspaceError, WorkspaceHandle, WorkspaceTeardownRequest};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn refused() -> WorkspaceError {
    WorkspaceError::WorktreeCreateConflict {
        detail: "plain worktree ownership is absent, stale, unsafe or task/protected".into(),
    }
}
fn uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid has no preconditions.
    unsafe { geteuid() }
}
fn flags() -> Result<i32, WorkspaceError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64" | "aarch64") => Ok(0x100 | 0x4),
        ("linux", "x86_64") => Ok(0x20000 | 0x800),
        ("linux", "aarch64") => Ok(0x8000 | 0x800),
        _ => Err(refused()),
    }
}
fn pin(path: &Path, directory: bool) -> Result<File, WorkspaceError> {
    for ancestor in path.ancestors().skip(usize::from(!directory)) {
        let m = fs::symlink_metadata(ancestor).map_err(|_| refused())?;
        if !m.is_dir()
            || (m.uid() != uid() && m.uid() != 0)
            || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0))
        {
            return Err(refused());
        }
    }
    let listed = fs::symlink_metadata(path).map_err(|_| refused())?;
    if listed.uid() != uid()
        || listed.mode() & 0o022 != 0
        || (directory && !listed.is_dir())
        || (!directory && (!listed.is_file() || listed.nlink() != 1))
    {
        return Err(refused());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(flags()?)
        .open(path)
        .map_err(|_| refused())?;
    let m = file.metadata().map_err(|_| refused())?;
    if (m.dev(), m.ino()) != (listed.dev(), listed.ino()) {
        return Err(refused());
    }
    Ok(file)
}
#[derive(Debug)]
struct Pins {
    paths: Vec<PathBuf>,
    files: Vec<File>,
    contents: Vec<Option<Vec<u8>>>,
}
fn contents(file: &File) -> Result<Option<Vec<u8>>, WorkspaceError> {
    let metadata = file.metadata().map_err(|_| refused())?;
    if metadata.is_dir() {
        return Ok(None);
    }
    if metadata.len() > 8192 {
        return Err(refused());
    }
    let mut bytes = vec![0; metadata.len() as usize];
    if file.read_at(&mut bytes, 0).map_err(|_| refused())? != bytes.len() {
        return Err(refused());
    }
    Ok(Some(bytes))
}
impl Pins {
    fn verify(&self) -> Result<(), WorkspaceError> {
        for ((path, file), expected) in self.paths.iter().zip(&self.files).zip(&self.contents) {
            let original = file.metadata().map_err(|_| refused())?;
            let linked = pin(path, original.is_dir())?;
            let current = linked.metadata().map_err(|_| refused())?;
            if (original.dev(), original.ino()) != (current.dev(), current.ino())
                || &contents(&linked)? != expected
            {
                return Err(refused());
            }
        }
        Ok(())
    }
}
/// Minted only after this process creates a fresh clean plain worktree.
/// Clones retain the identity pins. No deserializer or public adoption exists.
#[derive(Debug, Clone)]
pub struct OwnedPlainWorktree {
    root: PathBuf,
    handle: WorkspaceHandle,
    pins: Arc<Pins>,
}
impl PartialEq for OwnedPlainWorktree {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.handle == other.handle
            && Arc::ptr_eq(&self.pins, &other.pins)
    }
}
impl Eq for OwnedPlainWorktree {}
impl OwnedPlainWorktree {
    pub(crate) fn created(
        root: &Path,
        path: &Path,
        branch: &str,
        head: &CommitSha,
    ) -> Result<Self, WorkspaceError> {
        if worktree_binding::is_task_bound(root, path).map_err(|_| refused())? {
            return Err(refused());
        }
        let admin = worktree_binding::admin_dir(root, path).map_err(|_| refused())?;
        let paths = vec![
            root.to_path_buf(),
            path.to_path_buf(),
            path.join(".git"),
            admin.clone(),
            admin.join("gitdir"),
            admin.join("commondir"),
        ];
        let files = paths
            .iter()
            .enumerate()
            .map(|(i, p)| pin(p, matches!(i, 0 | 1 | 3)))
            .collect::<Result<Vec<_>, _>>()?;
        let contents = files.iter().map(contents).collect::<Result<_, _>>()?;
        Ok(Self {
            root: root.to_path_buf(),
            handle: WorkspaceHandle::provisioned(
                "plain".into(),
                None,
                branch.into(),
                path.into(),
                head.clone(),
                None,
                None,
            ),
            pins: Arc::new(Pins {
                paths,
                files,
                contents,
            }),
        })
    }
    pub fn path(&self) -> &Path {
        self.handle.worktree_path()
    }
    /// The caller must admit removal policy separately. A hook path conveys none.
    pub fn remove(&self, repository: &Path, path: &Path) -> Result<(), WorkspaceError> {
        if repository != self.root || path != self.path() {
            return Err(refused());
        }
        self.verify()?;
        WorkspaceTeardownRequest::new(repository).teardown_owned(
            &self.handle,
            &|root, target| {
                if root != self.root || target != self.path() {
                    return Err(refused());
                }
                self.verify()
            },
        )?;
        Ok(())
    }
    fn verify(&self) -> Result<(), WorkspaceError> {
        self.pins.verify()?;
        if worktree_binding::is_task_bound(&self.root, self.path()).map_err(|_| refused())? {
            return Err(refused());
        }
        let admin = worktree_binding::admin_dir(&self.root, self.path()).map_err(|_| refused())?;
        if self.pins.paths.get(3) != Some(&admin) {
            return Err(refused());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "plain_worktree_tests.rs"]
mod tests;
