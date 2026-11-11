//! D12 publication for trusted composition; never expose this boundary to workers.
use crate::{CommitSha, WorkspaceRemotePublishPolicy as Wire, git, validate_branch_name};
use std::{ffi::OsStr, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRemoteMode {
    LocalOnly,
    PushA2Branches,
    PushAcceptedA3,
    PushAllCheckpoints,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationBranchClass {
    Workstream,
    Task,
    Checkpoint,
}
/// Supplied by trusted composition after verifying exact review acceptance.
/// This inert input is not an issuer authentication boundary.
pub struct PublicationRequest<'a> {
    pub repository: &'a Path,
    pub remote: &'a str,
    pub branch: &'a str,
    pub commit: &'a str,
    pub wire: Wire,
    pub mode: ProjectRemoteMode,
    pub class: PublicationBranchClass,
    pub accepted_commit: Option<&'a str>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PublicationOutcome {
    LocalOnly,
    Published,
    Refused,
    RemoteFailed,
}

pub fn publish(request: &PublicationRequest<'_>) -> PublicationOutcome {
    publish_locked(request, || {})
}
pub(super) fn publish_locked(
    request: &PublicationRequest<'_>,
    after_lock: impl FnOnce(),
) -> PublicationOutcome {
    use ProjectRemoteMode as M;
    use PublicationBranchClass as C;
    use PublicationOutcome as O;
    if request.wire == Wire::LocalOnly && request.mode == M::LocalOnly {
        return O::LocalOnly;
    }
    let eligible = match (request.wire, request.mode, request.class) {
        (Wire::PushOnAccept, M::PushA2Branches, C::Workstream)
        | (Wire::PushOnAccept, M::PushAcceptedA3, C::Workstream | C::Task) => {
            request.accepted_commit == Some(request.commit)
        }
        (Wire::PushAlways, M::PushAllCheckpoints, _) => true,
        _ => false,
    };
    if !eligible
        || request.branch == "main"
        || request.branch == "master"
        || validate_branch_name(request.branch).is_err()
        || CommitSha::parse(request.commit).is_err()
        || request.remote.is_empty()
        || request.remote.starts_with('-')
    {
        return O::Refused;
    }
    if git::refuse_untrusted_repository_config(request.repository, "publish").is_err() {
        return O::Refused;
    }
    let source = format!("refs/heads/{}", request.branch);
    let Ok(_guard) = BranchLock::acquire(request.repository, &source) else {
        return O::Refused;
    };
    after_lock();
    let Ok(head) = git::capture(
        request.repository,
        "publish anchor",
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(&source),
        ],
    ) else {
        return O::Refused;
    };
    if !head.success || head.stdout.trim() != request.commit {
        return O::Refused;
    }
    let refspec = format!("{source}:{source}");
    // The fixed argv has no force mode. Git's receive side enforces fast-forward.
    match git::capture(
        request.repository,
        "publish",
        &[
            OsStr::new("-c"),
            OsStr::new("protocol.file.allow=always"),
            OsStr::new("push"),
            OsStr::new("--porcelain"),
            OsStr::new("--"),
            OsStr::new(request.remote),
            OsStr::new(&refspec),
        ],
    ) {
        Ok(result) if result.success => O::Published,
        _ => O::RemoteFailed,
    }
}

// Git writers honor this per-ref lock, including when the ref is packed. It
// closes the anchor-check/push race for cooperating Git writers, not hostile
// filesystem mutation by a principal with repository write authority.
struct BranchLock {
    path: std::path::PathBuf,
    file: std::fs::File,
}
impl BranchLock {
    fn acquire(repository: &Path, reference: &str) -> Result<Self, ()> {
        let common = git::capture(
            repository,
            "publication common directory",
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--path-format=absolute"),
                OsStr::new("--git-common-dir"),
            ],
        )
        .map_err(|_| ())?
        .require_success("publication common directory")
        .map_err(|_| ())?;
        #[cfg(unix)]
        let root = {
            use std::os::unix::ffi::OsStrExt;
            let bytes = common.raw_stdout().strip_suffix(b"\n").ok_or(())?;
            std::fs::canonicalize(OsStr::from_bytes(bytes)).map_err(|_| ())?
        };
        #[cfg(not(unix))]
        let root = std::fs::canonicalize(common.stdout.trim()).map_err(|_| ())?;
        let path = root.join(format!("{reference}.lock"));
        std::fs::create_dir_all(path.parent().ok_or(())?).map_err(|_| ())?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| ())?;
        Ok(Self { path, file })
    }
}
impl Drop for BranchLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(owned), Ok(current)) =
                (self.file.metadata(), std::fs::symlink_metadata(&self.path))
                && owned.dev() == current.dev()
                && owned.ino() == current.ino()
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
