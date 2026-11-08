//! Real rehydration materializers (ST-W2-2).
//!
//! * [`ArtifactFileMaterializerV1`]: a bounded, digest-checked reader for
//!   artifacts the trusted composition catalogued under one artifact root.
//! * [`MaterializerRegistryV1`]: the composition root's single place to plug
//!   the stable repository-blob port (implemented by Workspace in WE-W3-4;
//!   State runs no Git) and an artifact port into
//!   [`crate::TrustedCoreStore::rehydrate_context`]. An unplugged port fails
//!   that source closed with `MATERIALIZER_NOT_REGISTERED`.
//!
//! State queries keep their existing closed in-State registry
//! ([`crate::StateQueryRefV1`]); nothing here widens it.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, ExternalSourceMaterialization, MAX_STREAM_CHUNK_BYTES,
    RepositorySnapshotMaterializer, RepositorySnapshotRefV1, SourceMaterializationFailure,
};
use crate::error::StateError;
use crate::trusted_time::TrustedClockV1;

pub const ARTIFACT_FILE_MATERIALIZER_ID: &str = "state-context:artifact-file:v1";
/// Longest catalogued relative path, in bytes.
pub const MAX_ARTIFACT_RELATIVE_PATH_BYTES: usize = 1_024;

/// One artifact the trusted composition admitted: its id, its path below
/// the artifact root and the SHA-256 (64 lowercase hex) of its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCatalogEntryV1 {
    pub artifact_id: String,
    pub relative_path: String,
    pub sha256: String,
}

/// Reads catalogued artifacts below one root, for one project.
///
/// Every read walks the catalogued path from the held root descriptor with
/// `openat(O_NOFOLLOW)` (so no symlink anywhere below the root, and no `..`
/// or absolute component, can escape it), requires a singly linked regular
/// file within `max_bytes`, streams it in bounded chunks, re-checks the
/// file's identity/size/timestamps after reading and only then compares
/// the SHA-256 with the catalog. Any failure is a typed source failure, so
/// the rehydration attempt fails loudly and returns no partial body.
pub struct ArtifactFileMaterializerV1<'a> {
    root: std::fs::File,
    project_id: String,
    catalog: BTreeMap<String, ArtifactCatalogEntryV1>,
    max_bytes: u64,
    clock: &'a dyn TrustedClockV1,
}

impl std::fmt::Debug for ArtifactFileMaterializerV1<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactFileMaterializerV1")
            .field("project_id", &self.project_id)
            .field("artifacts", &self.catalog.len())
            .finish_non_exhaustive()
    }
}

fn invalid(detail: &str) -> StateError {
    StateError::ContextRehydrationValidation {
        detail: detail.to_string(),
    }
}

fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_ARTIFACT_RELATIVE_PATH_BYTES
        && !path.contains(['\0', '\\'])
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

impl<'a> ArtifactFileMaterializerV1<'a> {
    /// `root` must be an absolute path to a directory; it is resolved once
    /// (system aliases such as `/var`) and then opened component by component
    /// without following symlinks. Catalog entries must have unique ids,
    /// non-escaping relative paths and lowercase-hex SHA-256 digests.
    pub fn new(
        root: &Path,
        project_id: &str,
        entries: Vec<ArtifactCatalogEntryV1>,
        max_bytes: u64,
        clock: &'a dyn TrustedClockV1,
    ) -> Result<Self, StateError> {
        if !root.is_absolute() {
            return Err(invalid("artifact root must be absolute"));
        }
        if project_id.is_empty() || project_id.chars().count() > 200 {
            return Err(invalid("artifact project_id must be 1..=200 characters"));
        }
        if max_bytes == 0
            || max_bytes > crate::context_rehydration::MAX_MATERIALIZED_BYTES_PER_SOURCE
        {
            return Err(invalid("artifact max_bytes outside the per-source bound"));
        }
        let mut catalog = BTreeMap::new();
        for entry in entries {
            if entry.artifact_id.is_empty()
                || entry.artifact_id.chars().count() > 200
                || !valid_relative_path(&entry.relative_path)
                || entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid("artifact catalog entry is malformed"));
            }
            if catalog.insert(entry.artifact_id.clone(), entry).is_some() {
                return Err(invalid("duplicate artifact id in catalog"));
            }
        }
        let canonical = root
            .canonicalize()
            .map_err(|_| invalid("artifact root is not accessible"))?;
        Ok(Self {
            root: fs::open_root(&canonical).map_err(|code| invalid(code))?,
            project_id: project_id.to_string(),
            catalog,
            max_bytes,
            clock,
        })
    }

    fn entry(
        &self,
        reference: &ArtifactRefV1,
    ) -> Result<&ArtifactCatalogEntryV1, SourceMaterializationFailure> {
        if reference.project_id != self.project_id {
            return Err(failure("ARTIFACT_PROJECT_MISMATCH"));
        }
        self.catalog
            .get(&reference.artifact_id)
            .ok_or_else(|| failure("ARTIFACT_NOT_REGISTERED"))
    }

    fn open(
        &self,
        reference: &ArtifactRefV1,
    ) -> Result<(fs::Opened, &ArtifactCatalogEntryV1), SourceMaterializationFailure> {
        let entry = self.entry(reference)?;
        let opened = fs::open_below(&self.root, &entry.relative_path).map_err(failure)?;
        if opened.facts.size > self.max_bytes {
            return Err(failure("ARTIFACT_OVERSIZE"));
        }
        Ok((opened, entry))
    }
}

fn failure(code: &str) -> SourceMaterializationFailure {
    SourceMaterializationFailure {
        code: code.to_string(),
        materializer_id: Some(ARTIFACT_FILE_MATERIALIZER_ID.to_string()),
        provenance: None,
        materialized_at: None,
        failure_detail: None,
    }
}

impl ArtifactMaterializer for ArtifactFileMaterializerV1<'_> {
    fn known_length(
        &mut self,
        reference: &ArtifactRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(Some(self.open(reference)?.0.facts.size))
    }

    fn materialize(
        &mut self,
        reference: &ArtifactRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        use std::io::Read;
        let (mut opened, entry) = self.open(reference)?;
        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut chunk = vec![0_u8; MAX_STREAM_CHUNK_BYTES];
        loop {
            let read = opened
                .file
                .read(&mut chunk)
                .map_err(|_| failure("ARTIFACT_READ_FAILED"))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > opened.facts.size || total > self.max_bytes {
                return Err(failure("ARTIFACT_CHANGED_DURING_READ"));
            }
            hasher.update(&chunk[..read]);
            accept_chunk(&chunk[..read])?;
        }
        let after = fs::Facts::of(&opened.file).map_err(failure)?;
        if after != opened.facts || total != after.size {
            return Err(failure("ARTIFACT_CHANGED_DURING_READ"));
        }
        let mut hex = String::with_capacity(64);
        for byte in hasher.finalize() {
            use std::fmt::Write as _;
            write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
        }
        if hex != entry.sha256 {
            return Err(failure("ARTIFACT_DIGEST_MISMATCH"));
        }
        let materialized_at = self
            .clock
            .sample()
            .map_err(|_| failure("ARTIFACT_CLOCK_UNAVAILABLE"))?
            .canonical_utc_timestamp;
        Ok(ExternalSourceMaterialization {
            materializer_id: ARTIFACT_FILE_MATERIALIZER_ID.to_string(),
            provenance: format!("artifact-file:sha256:{hex}"),
            materialized_at,
        })
    }
}

#[cfg(unix)]
mod fs {
    use std::ffi::CString;
    use std::fs::File;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};
    use std::path::Path;

    /// The identity and content facts compared before and after a read.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct Facts {
        dev: u64,
        ino: u64,
        pub(super) size: u64,
        mtime: (i64, i64),
        ctime: (i64, i64),
    }

    impl Facts {
        pub(super) fn of(file: &File) -> Result<Self, &'static str> {
            let m = file.metadata().map_err(|_| "ARTIFACT_READ_FAILED")?;
            if !m.file_type().is_file() {
                return Err("ARTIFACT_NOT_REGULAR_FILE");
            }
            if m.nlink() != 1 {
                return Err("ARTIFACT_HARD_LINK_REFUSED");
            }
            Ok(Self {
                dev: m.dev(),
                ino: m.ino(),
                size: m.size(),
                mtime: (m.mtime(), m.mtime_nsec()),
                ctime: (m.ctime(), m.ctime_nsec()),
            })
        }
    }

    pub(super) struct Opened {
        pub(super) file: File,
        pub(super) facts: Facts,
    }

    fn open_at(dir: Option<&File>, name: &[u8], flags: libc::c_int) -> Result<File, &'static str> {
        let name = CString::new(name).map_err(|_| "ARTIFACT_PATH_INVALID")?;
        let base = dir.map_or(libc::AT_FDCWD, AsRawFd::as_raw_fd);
        // SAFETY: valid descriptor (or AT_FDCWD) and NUL-terminated name;
        // no O_CREAT, so no mode argument is read.
        let fd = unsafe {
            libc::openat(
                base,
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                // Platforms report a no-follow symlink as ELOOP or (for a
                // directory component) ENOTDIR; classify without following.
                Some(libc::ELOOP | libc::ENOTDIR) if is_symlink(base, &name) => {
                    "ARTIFACT_SYMLINK_REFUSED"
                }
                Some(libc::ENOENT) => "ARTIFACT_MISSING",
                Some(libc::ENOTDIR) => "ARTIFACT_PATH_INVALID",
                _ => "ARTIFACT_OPEN_FAILED",
            });
        }
        // SAFETY: fd is a freshly opened descriptor we now own.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn is_symlink(base: libc::c_int, name: &CString) -> bool {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid descriptor (or AT_FDCWD), NUL-terminated name and a
        // writable stat buffer; AT_SYMLINK_NOFOLLOW never follows the entry.
        let rc = unsafe {
            libc::fstatat(
                base,
                name.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        // SAFETY: fstatat succeeded and initialised the buffer.
        rc == 0 && (unsafe { st.assume_init() }.st_mode & libc::S_IFMT) == libc::S_IFLNK
    }

    pub(super) fn open_root(canonical: &Path) -> Result<File, &'static str> {
        let mut dir = open_at(None, b"/", libc::O_RDONLY | libc::O_DIRECTORY)?;
        for component in canonical.components().skip(1) {
            dir = open_at(
                Some(&dir),
                component.as_os_str().as_bytes(),
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
        }
        Ok(dir)
    }

    pub(super) fn open_below(root: &File, relative: &str) -> Result<Opened, &'static str> {
        let mut segments = relative.split('/').peekable();
        let mut dir: Option<File> = None;
        while let Some(segment) = segments.next() {
            let parent = dir.as_ref().unwrap_or(root);
            if segments.peek().is_some() {
                dir = Some(open_at(
                    Some(parent),
                    segment.as_bytes(),
                    libc::O_RDONLY | libc::O_DIRECTORY,
                )?);
            } else {
                let file = open_at(
                    Some(parent),
                    segment.as_bytes(),
                    libc::O_RDONLY | libc::O_NONBLOCK,
                )?;
                let facts = Facts::of(&file)?;
                return Ok(Opened { file, facts });
            }
        }
        Err("ARTIFACT_PATH_INVALID")
    }
}

#[cfg(not(unix))]
mod fs {
    use std::fs::File;
    use std::path::Path;

    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct Facts {
        pub(super) size: u64,
    }
    impl Facts {
        pub(super) fn of(_: &File) -> Result<Self, &'static str> {
            Err("UNSUPPORTED_PLATFORM")
        }
    }
    pub(super) struct Opened {
        pub(super) file: File,
        pub(super) facts: Facts,
    }
    pub(super) fn open_root(_: &Path) -> Result<File, &'static str> {
        Err("UNSUPPORTED_PLATFORM")
    }
    pub(super) fn open_below(_: &File, _: &str) -> Result<Opened, &'static str> {
        Err("UNSUPPORTED_PLATFORM")
    }
}

/// The composition root's materializer registry: one optional repository
/// blob port (Workspace's `GitBlobMaterializer`, WE-W3-4) and one optional
/// artifact port. Hand [`Self::ports`] to `rehydrate_context`.
pub struct MaterializerRegistryV1<'a> {
    repository: RepositoryPortV1<'a>,
    artifacts: ArtifactPortV1<'a>,
}

/// The registered repository-blob port, or a closed refusal.
pub struct RepositoryPortV1<'a>(Option<&'a mut dyn RepositorySnapshotMaterializer>);
/// The registered artifact port, or a closed refusal.
pub struct ArtifactPortV1<'a>(Option<&'a mut dyn ArtifactMaterializer>);

fn not_registered() -> SourceMaterializationFailure {
    SourceMaterializationFailure {
        code: "MATERIALIZER_NOT_REGISTERED".to_string(),
        materializer_id: None,
        provenance: None,
        materialized_at: None,
        failure_detail: None,
    }
}

impl<'a> MaterializerRegistryV1<'a> {
    pub fn new(
        repository: Option<&'a mut dyn RepositorySnapshotMaterializer>,
        artifacts: Option<&'a mut dyn ArtifactMaterializer>,
    ) -> Self {
        Self {
            repository: RepositoryPortV1(repository),
            artifacts: ArtifactPortV1(artifacts),
        }
    }

    /// Disjoint borrows of both ports for one rehydration call.
    pub fn ports(&mut self) -> (&mut RepositoryPortV1<'a>, &mut ArtifactPortV1<'a>) {
        (&mut self.repository, &mut self.artifacts)
    }
}

impl RepositorySnapshotMaterializer for RepositoryPortV1<'_> {
    fn known_length(
        &mut self,
        reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        self.0
            .as_mut()
            .ok_or_else(not_registered)?
            .known_length(reference)
    }

    fn materialize(
        &mut self,
        reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.0
            .as_mut()
            .ok_or_else(not_registered)?
            .materialize(reference, accept_chunk)
    }
}

impl ArtifactMaterializer for ArtifactPortV1<'_> {
    fn known_length(
        &mut self,
        reference: &ArtifactRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        self.0
            .as_mut()
            .ok_or_else(not_registered)?
            .known_length(reference)
    }

    fn materialize(
        &mut self,
        reference: &ArtifactRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.0
            .as_mut()
            .ok_or_else(not_registered)?
            .materialize(reference, accept_chunk)
    }
}
