//! Original, process-local custody for production D4. Durable records describe
//! custody; only the surviving owner retains the pins and may perform actions.
use crate::{
    WorkspaceHandle, WorkspaceProvisionRequest, git, protected_custody::Dir, worktree_binding,
};
use receipts_state::{WorkspaceAcceptedTargetV1, WorkspaceRecoveryActionGuardV1};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

fn fail(detail: impl ToString) -> io::Error {
    io::Error::other(detail.to_string())
}
fn command(root: &Path, args: &[&OsStr]) -> io::Result<Vec<u8>> {
    git::refuse_untrusted_repository_config(root, "D4 recovery").map_err(fail)?;
    let output = git::capture(root, "D4 recovery", args)
        .and_then(|o| o.require_success("D4 recovery"))
        .map_err(fail)?;
    Ok(output.raw_stdout().to_vec())
}
fn text(root: &Path, args: &[&str]) -> io::Result<String> {
    let args: Vec<_> = args.iter().map(OsStr::new).collect();
    String::from_utf8(command(root, &args)?)
        .map(|s| s.trim_end_matches('\n').to_owned())
        .map_err(fail)
}

pub struct RecoveryCaptureRequest<'a> {
    pub project: &'a str,
    pub attempt: &'a str,
    pub checkpoint_id: &'a str,
    pub kind: crate::WorkspaceCheckpointKind,
    pub output_directory: &'a Path,
    pub timestamp: crate::WorkspaceDateTimeV1,
}
pub struct RecoveryCheckpoint {
    pub record: receipts_state::CheckpointRecord,
    pub outputs: Vec<receipts_state::OutputRef>,
    pub capture: crate::WorkspaceCheckpointTemporalCaptureCore,
}

type TrackedBlob = (Vec<u8>, String, Vec<u8>);

pub struct WorkspaceRecoveryCustody {
    handle: WorkspaceHandle,
    repository: PathBuf,
    admin: PathBuf,
    common: PathBuf,
    directories: Vec<(PathBuf, Dir)>,
    files: Vec<(PathBuf, File, Vec<u8>)>,
    reservation: (PathBuf, File, Vec<u8>),
    identity: String,
}
impl WorkspaceRecoveryCustody {
    /// Never adopts a path or decoded handle. Failure preserves an ambiguous
    /// reservation for reconciliation; no stale reservation is automatically removed.
    pub fn provision(request: &WorkspaceProvisionRequest) -> io::Result<Self> {
        let repository = fs::canonicalize(request.repository_root())?;
        let common = fs::canonicalize(text(
            &repository,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?)?;
        match fs::symlink_metadata(common.join("receipts-custody.state")) {
            Ok(_) => return Err(fail("protected custody remains quarantined")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let handle = request.provision().map_err(fail)?;
        let root = handle.worktree_path();
        let admin = worktree_binding::admin_dir(&repository, root).map_err(fail)?;
        let mut directories: Vec<(PathBuf, Dir)> = Vec::new();
        for base in [
            root,
            admin.as_path(),
            common.as_path(),
            repository.as_path(),
        ] {
            for path in base.ancestors() {
                if !directories.iter().any(|(p, _)| p == path) {
                    directories.push((path.into(), Dir::open_absolute(path)?));
                }
            }
        }
        let mut files = Vec::new();
        for path in [
            root.join(".git"),
            admin.join("gitdir"),
            admin.join("commondir"),
            admin.join(worktree_binding::BINDING_FILE),
        ] {
            let m = fs::symlink_metadata(&path)?;
            if !m.is_file() || m.nlink() != 1 || m.len() > 16 * 1024 {
                return Err(fail("ambiguous Git metadata"));
            }
            let f = OpenOptions::new()
                .read(true)
                .custom_flags(nonblock())
                .open(&path)?;
            let opened = f.metadata()?;
            if (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
                return Err(fail("replaced Git metadata"));
            }
            let mut bytes = Vec::new();
            (&f).take(16 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 {
                return Err(fail("oversize Git metadata"));
            }
            files.push((path, f, bytes));
        }
        let path = admin.join("receipts-d4-reservation");
        let mut reservation = OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let identities: Vec<_> = directories
            .iter()
            .map(|(p, d)| (p, d.meta.dev, d.meta.ino))
            .collect();
        let bytes = serde_json::to_vec(&(
            handle.workspace_id(),
            handle.branch(),
            handle.base_sha().as_str(),
            &identities,
        ))
        .map_err(fail)?;
        let identity = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        reservation.write_all(&bytes)?;
        reservation.sync_all()?;
        File::open(&admin)?.sync_all()?;
        let result = Self {
            handle,
            repository,
            admin,
            common,
            directories,
            files,
            reservation: (path, reservation, bytes),
            identity,
        };
        result.verify()?;
        Ok(result)
    }
    pub fn handle(&self) -> &WorkspaceHandle {
        &self.handle
    }
    /// A fresh start handle from the continuously pinned workspace, never from wire data.
    pub fn attempt_handle(&self) -> io::Result<WorkspaceHandle> {
        let head = crate::CommitSha::parse(&self.head()?).map_err(fail)?;
        Ok(WorkspaceHandle::provisioned(
            self.handle.workspace_id().into(),
            self.handle.task_id().map(str::to_owned),
            self.handle.branch().into(),
            self.handle.worktree_path().into(),
            head,
            None,
            None,
        ))
    }
    pub fn capture(
        &self,
        request: RecoveryCaptureRequest<'_>,
        streams: &[(&str, &crate::execution::CapturedStream)],
    ) -> io::Result<RecoveryCheckpoint> {
        self.verify()?;
        let diff = command(
            self.handle.worktree_path(),
            &[
                OsStr::new("diff"),
                OsStr::new("--binary"),
                OsStr::new("--no-ext-diff"),
                OsStr::new("--no-textconv"),
                OsStr::new("HEAD"),
                OsStr::new("--"),
            ],
        )?;
        let mut artifacts = vec![("dirty-diff".to_owned(), diff)];
        artifacts.extend(streams.iter().map(|(name, stream)| {
            let mut bytes = stream.head().to_vec();
            bytes.extend_from_slice(stream.tail());
            ((*name).to_owned(), bytes)
        }));
        let mut outputs = Vec::new();
        let mut dirty = None;
        for (name, bytes) in artifacts {
            let (digest, path) =
                crate::output_store::store_output(request.output_directory, &bytes)?;
            let reference = format!("{}:{name}", request.checkpoint_id);
            if name == "dirty-diff" {
                dirty = Some(
                    crate::WorkspaceCheckpointRef::new(
                        crate::WorkspaceCheckpointRefType::ArtifactId,
                        reference.clone(),
                        Some(digest.clone()),
                        None,
                    )
                    .map_err(fail)?,
                );
            }
            outputs.push(
                receipts_state::OutputRef::new(
                    request.project,
                    &reference,
                    &digest,
                    path.to_str()
                        .ok_or_else(|| fail("non-UTF8 artifact path"))?,
                    &bytes.len().to_string(),
                )
                .map_err(fail)?,
            );
        }
        let capture = crate::capture_workspace_checkpoint_temporal_evidence(
            crate::WorkspaceCheckpointEvidenceCaptureRequest {
                directory: self.handle.worktree_path(),
                checkpoint_id: request.checkpoint_id.into(),
                workspace_id: self.handle.workspace_id().into(),
                task_id: self.handle.task_id().map(str::to_owned),
                attempt_id: Some(request.attempt.into()),
                kind: request.kind,
                base_sha: None,
                dirty_diff_ref: dirty,
                executed_checks: vec![],
            },
            request.timestamp,
        )
        .map_err(fail)?;
        let record = receipts_state::CheckpointRecord::parse_json(
            request.project,
            &crate::workspace_checkpoint_to_json(&capture).map_err(fail)?,
        )
        .map_err(fail)?;
        self.verify()?;
        Ok(RecoveryCheckpoint {
            record,
            outputs,
            capture,
        })
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn verify(&self) -> io::Result<()> {
        match fs::symlink_metadata(self.common.join("receipts-custody.state")) {
            Ok(_) => return Err(fail("protected custody remains quarantined")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        for (path, pin) in &self.directories {
            let current = Dir::open_absolute(path)?;
            let original = pin.current()?;
            if (current.meta.dev, current.meta.ino) != (original.dev, original.ino) {
                return Err(fail("replaced custody directory"));
            }
        }
        for (path, pin, bytes) in &self.files {
            let current = fs::symlink_metadata(path)?;
            let original = pin.metadata()?;
            if !current.is_file()
                || current.nlink() != 1
                || current.len() > 16 * 1024
                || (current.dev(), current.ino()) != (original.dev(), original.ino())
                || bounded_bytes(path, 16 * 1024)? != *bytes
            {
                return Err(fail("replaced Git metadata"));
            }
        }
        let (path, pin, bytes) = &self.reservation;
        let current = fs::symlink_metadata(path)?;
        let original = pin.metadata()?;
        if !current.is_file()
            || current.nlink() != 1
            || (current.dev(), current.ino()) != (original.dev(), original.ino())
        {
            return Err(fail("lost recovery reservation"));
        }
        if bounded_bytes(path, 64 * 1024)? != *bytes {
            return Err(fail("changed durable provisioning record"));
        }
        if worktree_binding::admin_dir(&self.repository, self.handle.worktree_path())
            .map_err(fail)?
            != self.admin
            || text(
                self.handle.worktree_path(),
                &["symbolic-ref", "--quiet", "HEAD"],
            )? != format!("refs/heads/{}", self.handle.branch())
        {
            return Err(fail("changed workspace registration or branch"));
        }
        Ok(())
    }
    pub fn head(&self) -> io::Result<String> {
        self.verify()?;
        text(
            self.handle.worktree_path(),
            &["rev-parse", "--verify", "HEAD^{commit}"],
        )
    }
    pub fn accepted_tree(&self) -> io::Result<(String, String)> {
        let head = self.head()?;
        let tree = text(
            self.handle.worktree_path(),
            &["rev-parse", &format!("{head}^{{tree}}")],
        )?;
        self.readback(&head, &tree)?;
        Ok((head, tree))
    }
    fn entries(&self, commit: &str) -> io::Result<Vec<TrackedBlob>> {
        crate::CommitSha::parse(commit).map_err(fail)?;
        let raw = command(
            self.handle.worktree_path(),
            &[
                OsStr::new("ls-tree"),
                OsStr::new("-rz"),
                OsStr::new("--full-tree"),
                OsStr::new(commit),
            ],
        )?;
        let mut entries = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            if std::time::Instant::now() >= deadline {
                return Err(fail("bounded physical readback budget exhausted"));
            }
            let (meta, path) = record.split_at(
                record
                    .iter()
                    .position(|b| *b == b'\t')
                    .ok_or_else(|| fail("invalid tree entry"))?,
            );
            let fields: Vec<_> = meta.split(|b| *b == b' ').collect();
            if fields.len() != 3
                || !matches!(fields[0], b"100644" | b"100755")
                || fields[1] != b"blob"
            {
                return Err(fail("unsupported tracked symlink, submodule or tree mode"));
            }
            let path = path[1..].to_vec();
            let native = PathBuf::from(OsString::from_vec(path.clone()));
            if path
                .split(|b| *b == b'/')
                .any(|part| part.is_empty() || matches!(part, b"." | b".."))
                || native
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(fail("unsafe tracked path"));
            }
            let object = std::str::from_utf8(fields[2]).map_err(fail)?;
            let bytes = command(
                self.handle.worktree_path(),
                &[
                    OsStr::new("cat-file"),
                    OsStr::new("blob"),
                    OsStr::new(object),
                ],
            )?;
            entries.push((
                path,
                String::from_utf8(fields[0].to_vec()).map_err(fail)?,
                bytes,
            ));
        }
        Ok(entries)
    }
    /// Actual file bytes and execute bits are compared with accepted blobs;
    /// index flags, stat caches and core.fileMode cannot substitute for readback.
    pub fn readback(&self, commit: &str, tree: &str) -> io::Result<()> {
        self.verify()?;
        if self.head()? != commit || text(self.handle.worktree_path(), &["write-tree"])? != tree {
            return Err(fail("HEAD/index differs from accepted target"));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for (name, mode, bytes) in self.entries(commit)? {
            if std::time::Instant::now() >= deadline {
                return Err(fail("bounded physical readback budget exhausted"));
            }
            let path = self.handle.worktree_path().join(OsString::from_vec(name));
            self.safe_ancestors(&path)?;
            let listed = fs::symlink_metadata(&path)?;
            if !listed.is_file()
                || listed.nlink() != 1
                || listed.len() != bytes.len() as u64
                || (listed.mode() & 0o111 != 0) != (mode == "100755")
            {
                return Err(fail("tracked content/mode differs from accepted tree"));
            }
            let f = OpenOptions::new()
                .read(true)
                .custom_flags(nonblock())
                .open(&path)?;
            let opened = f.metadata()?;
            let mut observed = Vec::new();
            if (listed.dev(), listed.ino()) != (opened.dev(), opened.ino()) {
                return Err(fail("tracked file replaced during readback"));
            }
            f.take(bytes.len() as u64 + 1).read_to_end(&mut observed)?;
            if observed != bytes {
                return Err(fail("physical tracked content differs from accepted blob"));
            }
        }
        let diff = command(
            self.handle.worktree_path(),
            &[
                OsStr::new("diff"),
                OsStr::new("--name-only"),
                OsStr::new("--no-ext-diff"),
                OsStr::new("--no-textconv"),
                OsStr::new(commit),
                OsStr::new("--"),
            ],
        )?;
        if !diff.is_empty() {
            return Err(fail("extra tracked modifications remain"));
        }
        self.verify()
    }
    fn safe_ancestors(&self, path: &Path) -> io::Result<()> {
        for ancestor in path
            .ancestors()
            .skip(1)
            .take_while(|p| *p != self.handle.worktree_path())
        {
            let m = fs::symlink_metadata(ancestor)?;
            if !m.is_dir() || m.dev() != fs::metadata(self.handle.worktree_path())?.dev() {
                return Err(fail("ambiguous tracked directory"));
            }
        }
        Ok(())
    }
    /// The trusted composition supplies its State-validated immutable target
    /// while retaining the current Reset action guard through reset/readback.
    pub fn reset(
        &self,
        target: &WorkspaceAcceptedTargetV1,
        _action: &WorkspaceRecoveryActionGuardV1<'_>,
    ) -> io::Result<()> {
        self.verify()?;
        if target.workspace_id != self.handle.workspace_id()
            || target.provisioning_identity != self.identity
        {
            return Err(fail("foreign reset target"));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let entries = self.entries(&target.accepted_commit)?;
        let untracked = command(
            self.handle.worktree_path(),
            &[
                OsStr::new("ls-files"),
                OsStr::new("--others"),
                OsStr::new("-z"),
            ],
        )?;
        for path in untracked.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            for (tracked, _, _) in &entries {
                if path == tracked
                    || path
                        .strip_prefix(tracked.as_slice())
                        .is_some_and(|s| s.starts_with(b"/"))
                    || tracked
                        .strip_prefix(path)
                        .is_some_and(|s| s.starts_with(b"/"))
                {
                    return Err(fail(
                        "untracked/ignored reset obstruction; evidence retained",
                    ));
                }
            }
        }
        for (name, _, _) in &entries {
            if std::time::Instant::now() >= deadline {
                return Err(fail("bounded reset preflight budget exhausted"));
            }
            let path = self
                .handle
                .worktree_path()
                .join(OsString::from_vec(name.clone()));
            match fs::symlink_metadata(&path) {
                Ok(m) if !m.is_file() || m.nlink() != 1 => {
                    return Err(fail("ambiguous tracked reset target"));
                }
                Ok(_) => self.safe_ancestors(&path)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        command(
            self.handle.worktree_path(),
            &[
                OsStr::new("reset"),
                OsStr::new("--hard"),
                OsStr::new(&target.accepted_commit),
                OsStr::new("--"),
            ],
        )?;
        self.readback(&target.accepted_commit, &target.accepted_tree)
    }
    /// Only after State has durably resolved and released its reservations.
    pub fn release(&self) -> io::Result<()> {
        self.verify()?;
        fs::remove_file(&self.reservation.0)?;
        File::open(&self.admin)?.sync_all()
    }
}
#[cfg(target_os = "macos")]
fn nonblock() -> i32 {
    0x0004
}
#[cfg(target_os = "linux")]
fn nonblock() -> i32 {
    0x0800
}

fn bounded_bytes(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nonblock())
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(fail("non-regular recovery metadata"));
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(fail("oversize recovery metadata"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestRepo, git};
    fn provision(repo: &TestRepo) -> WorkspaceRecoveryCustody {
        WorkspaceRecoveryCustody::provision(
            &WorkspaceProvisionRequest::new(
                repo.path(),
                "workspace",
                Some("task"),
                "task/recovery",
                repo.path().join("wt"),
                &repo.head_sha(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn recovery_custody_quarantine_refuses_before_provisioning() {
        let repo = TestRepo::new("production-quarantine");
        let marker = repo.path().join(".git/receipts-custody.state");
        fs::write(&marker, b"retained protected custody").unwrap();
        let request = WorkspaceProvisionRequest::new(
            repo.path(),
            "workspace",
            Some("task"),
            "task/recovery",
            repo.path().join("wt"),
            &repo.head_sha(),
        )
        .unwrap();
        assert!(WorkspaceRecoveryCustody::provision(&request).is_err());
        assert!(!repo.path().join("wt").exists());
        assert_eq!(fs::read(marker).unwrap(), b"retained protected custody");
    }
    #[test]
    fn recovery_custody_replaced_directories_and_durable_record_refuse() {
        for target in 0..4 {
            let repo = TestRepo::new("production-identities");
            let custody = provision(&repo);
            let path = match target {
                0 => custody.handle.worktree_path().to_owned(),
                1 => custody.admin.clone(),
                2 => custody.common.clone(),
                _ => custody.reservation.0.clone(),
            };
            let retained = path.with_extension("retained");
            fs::rename(&path, &retained).unwrap();
            if target == 3 {
                fs::write(&path, fs::read(&retained).unwrap()).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            assert!(custody.verify().is_err());
            if target == 3 {
                fs::remove_file(&path).unwrap();
            } else {
                fs::remove_dir(&path).unwrap();
            }
            fs::rename(&retained, &path).unwrap();
            custody.verify().unwrap();
        }
    }
    #[test]
    fn recovery_custody_physical_readback_ignores_index_cache_shortcuts() {
        let repo = TestRepo::new("production-readback");
        repo.commit_file("tracked.txt", "accepted\n");
        let custody = provision(&repo);
        let (head, tree) = custody.accepted_tree().unwrap();
        git(
            custody.handle.worktree_path(),
            &["update-index", "--assume-unchanged", "tracked.txt"],
        );
        fs::write(
            custody.handle.worktree_path().join("tracked.txt"),
            b"modified\n",
        )
        .unwrap();
        assert_eq!(
            text(custody.handle.worktree_path(), &["diff", "--name-only"]).unwrap(),
            ""
        );
        assert!(custody.readback(&head, &tree).is_err());
    }
}
