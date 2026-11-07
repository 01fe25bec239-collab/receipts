//! Persistent task identity binding for `WorktreeCreate` (WE-W1-5).
//!
//! Hardened provisioning of a task-bound worktree records one small,
//! versioned, owner-local file inside the worktree's own Git administrative
//! directory (`<common-dir>/worktrees/<id>/receipts-workspace-binding`). That
//! directory is Git metadata, not worker payload, and is the same custody the
//! protected boundary later transfers to the controller. The record names the
//! exact canonical repository and worktree paths, task branch, ORIGINAL base
//! commit, workspace ID and task ID, and carries a SHA-256 over those fields.
//!
//! A later task attach is accepted only when the record is present, the
//! canonical encoding of the parsed fields equals the file bytes (which also
//! proves the digest), the file is a regular file owned by the current
//! effective user or root and not group/other writable, and every field equals
//! the request. Missing, malformed, mismatched or unauthenticated records
//! refuse; nothing is inferred from branch ancestry and no record is ever
//! created, repaired or overwritten at attach time. Records are created with
//! `create_new`, so an existing marker is never overwritten.
//!
//! Limits: this authenticates by custody (owner and mode), not by a secret —
//! anything that can already act as the owning user or root can forge the
//! file, which is outside the admission assumptions. After protected custody
//! transfer the record is controller-owned and only attachable by a process
//! running as that identity or root. A crash between worktree creation and
//! the record leaves no (or a malformed) record, so attach refuses and the
//! existing branch makes re-provisioning refuse as well; nothing is deleted.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::WorkspaceError;
use crate::git;
use crate::handle::CommitSha;

/// File name inside the worktree's administrative directory.
pub(crate) const BINDING_FILE: &str = "receipts-workspace-binding";
const MAGIC: &str = "receipts-workspace-binding/v1";
const MAX_BINDING_BYTES: u64 = 16 * 1024;
const MAX_GITDIR_BYTES: u64 = 8 * 1024;
const MAX_ADMIN_ENTRIES: usize = 10_000;

/// Exact identity of one task-bound worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskBinding {
    pub(crate) repository: PathBuf,
    pub(crate) worktree: PathBuf,
    pub(crate) branch: String,
    pub(crate) base_sha: CommitSha,
    pub(crate) workspace_id: String,
    pub(crate) task_id: String,
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn native_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn native_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? << 4 | digit(pair[1])?)
        })
        .collect()
}

impl TaskBinding {
    /// Canonical encoding: fixed key order, lowercase hex values, trailing
    /// digest of everything before it.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut body = String::new();
        let _ = writeln!(body, "{MAGIC}");
        let _ = writeln!(body, "repository={}", hex(&path_bytes(&self.repository)));
        let _ = writeln!(body, "worktree={}", hex(&path_bytes(&self.worktree)));
        let _ = writeln!(body, "branch={}", hex(self.branch.as_bytes()));
        let _ = writeln!(body, "base={}", self.base_sha.as_str());
        let _ = writeln!(body, "workspace={}", hex(self.workspace_id.as_bytes()));
        let _ = writeln!(body, "task={}", hex(self.task_id.as_bytes()));
        let digest = Sha256::digest(body.as_bytes());
        let _ = writeln!(body, "sha256={}", hex(&digest));
        body.into_bytes()
    }

    /// Strict decode: parse the fields, then require the canonical encoding
    /// of the result to equal the input exactly (this verifies the digest and
    /// rejects every non-canonical spelling).
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        const MALFORMED: &str = "binding record is malformed";
        let text = std::str::from_utf8(bytes).map_err(|_| MALFORMED)?;
        let mut lines = text.split_inclusive('\n');
        match lines.next() {
            Some(line) if line == format!("{MAGIC}\n") => {}
            Some(line) if line.starts_with("receipts-workspace-binding/") => {
                return Err("binding record version is unsupported");
            }
            _ => return Err(MALFORMED),
        }
        let mut field = |key: &str| -> Result<String, &'static str> {
            let line = lines.next().ok_or(MALFORMED)?;
            let value = line
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('='))
                .and_then(|rest| rest.strip_suffix('\n'))
                .ok_or(MALFORMED)?;
            Ok(value.to_owned())
        };
        let utf8 = |value: String| -> Result<String, &'static str> {
            String::from_utf8(unhex(&value).ok_or(MALFORMED)?).map_err(|_| MALFORMED)
        };
        let repository = native_path(&unhex(&field("repository")?).ok_or(MALFORMED)?);
        let worktree = native_path(&unhex(&field("worktree")?).ok_or(MALFORMED)?);
        let branch = utf8(field("branch")?)?;
        let base_sha = CommitSha::parse(&field("base")?).map_err(|_| MALFORMED)?;
        let workspace_id = utf8(field("workspace")?)?;
        let task_id = utf8(field("task")?)?;
        let binding = Self {
            repository,
            worktree,
            branch,
            base_sha,
            workspace_id,
            task_id,
        };
        if binding.encode() != bytes {
            return Err("binding record is not canonical or its digest does not match");
        }
        Ok(binding)
    }

    /// Names (never values) of the fields that differ from `other`.
    fn differing_fields(&self, other: &Self) -> Vec<&'static str> {
        [
            ("repository", self.repository == other.repository),
            ("worktree", self.worktree == other.worktree),
            ("branch", self.branch == other.branch),
            ("base", self.base_sha == other.base_sha),
            ("workspace", self.workspace_id == other.workspace_id),
            ("task", self.task_id == other.task_id),
        ]
        .into_iter()
        .filter_map(|(name, equal)| (!equal).then_some(name))
        .collect()
    }
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("file exceeds its size bound"));
    }
    Ok(bytes)
}

/// The repository's canonical common Git directory.
fn common_dir(root: &Path) -> Result<PathBuf, String> {
    let capture = git::capture(
        root,
        "resolve git common directory",
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--path-format=absolute"),
            OsStr::new("--git-common-dir"),
        ],
    )
    .and_then(|capture| capture.require_success("resolve git common directory"))
    .map_err(|error| error.to_string())?;
    let raw = capture.raw_stdout();
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    std::fs::canonicalize(native_path(raw))
        .map_err(|error| format!("git common directory: {error}"))
}

/// The administrative directory that registers exactly the worktree checkout
/// at `target`, found from the repository side (never by following the
/// worktree's own `.git` pointer, which is payload-writable before custody).
pub(crate) fn admin_dir(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let registrations = common_dir(root)?.join("worktrees");
    let expected = target.join(".git");
    let expected_canonical = std::fs::canonicalize(&expected).ok();
    let entries = std::fs::read_dir(&registrations)
        .map_err(|error| format!("worktree registrations: {error}"))?;
    let mut found: Option<PathBuf> = None;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ADMIN_ENTRIES {
            return Err("too many worktree registrations to inspect".into());
        }
        let dir = entry
            .map_err(|error| format!("worktree registrations: {error}"))?
            .path();
        let Ok(recorded) = read_bounded(&dir.join("gitdir"), MAX_GITDIR_BYTES) else {
            continue;
        };
        let recorded = native_path(recorded.strip_suffix(b"\n").unwrap_or(&recorded));
        let same = recorded == expected
            || matches!(
                (std::fs::canonicalize(&recorded).ok(), &expected_canonical),
                (Some(left), Some(right)) if left == *right
            );
        if same && found.replace(dir).is_some() {
            return Err("several administrative directories record this worktree".into());
        }
    }
    found.ok_or_else(|| "no administrative directory records this worktree".into())
}

/// Records `binding` for the freshly created worktree. Uses `create_new`: an
/// existing marker is a failure, never overwritten.
pub(crate) fn record(root: &Path, binding: &TaskBinding) -> Result<(), WorkspaceError> {
    let fail = |detail: String| WorkspaceError::WorktreeBindingFailed { detail };
    let dir = admin_dir(root, &binding.worktree).map_err(fail)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o400);
    }
    let mut file = options
        .open(dir.join(BINDING_FILE))
        .map_err(|error| fail(format!("create binding: {error}")))?;
    file.write_all(&binding.encode())
        .map_err(|error| fail(format!("write binding: {error}")))
}

/// Whether any entry named like a binding exists for the worktree at
/// `target` (valid or not): such a worktree is task-bound and must never be
/// attached as a plain worktree.
pub(crate) fn is_task_bound(root: &Path, target: &Path) -> Result<bool, String> {
    let dir = admin_dir(root, target)?;
    match std::fs::symlink_metadata(dir.join(BINDING_FILE)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("binding entry: {error}")),
    }
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: `geteuid` takes no arguments, has no preconditions and cannot fail.
    unsafe { geteuid() }
}

/// Custody authentication: a regular, non-symlink file of bounded size that
/// is owned by `trusted_uid` or root and not group/other writable.
#[cfg(unix)]
fn authenticated_read(path: &Path, trusted_uid: u32) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::MetadataExt;
    let listed = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "no task binding is recorded for this worktree (missing)".to_owned()
        } else {
            format!("binding entry: {error}")
        }
    })?;
    if !listed.file_type().is_file() || listed.len() > MAX_BINDING_BYTES {
        return Err("binding entry is not a bounded regular file".into());
    }
    if listed.mode() & 0o022 != 0 {
        return Err("binding is group/other writable (unauthenticated)".into());
    }
    if listed.uid() != trusted_uid && listed.uid() != 0 {
        return Err("binding is not owned by the trusted identity (unauthenticated)".into());
    }
    let file = std::fs::File::open(path).map_err(|error| format!("open binding: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("binding metadata: {error}"))?;
    if (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return Err("binding entry changed while being read".into());
    }
    read_bounded(path, MAX_BINDING_BYTES).map_err(|error| format!("read binding: {error}"))
}

#[cfg(not(unix))]
fn authenticated_read(_: &Path, _: u32) -> Result<Vec<u8>, String> {
    Err("binding authentication is unsupported on this platform".into())
}

#[cfg(not(unix))]
fn effective_uid() -> u32 {
    0
}

/// Verifies that the worktree at `expected.worktree` carries exactly the
/// `expected` identity. Refusal reasons name fields, never values.
pub(crate) fn verify(root: &Path, expected: &TaskBinding) -> Result<(), String> {
    verify_as(root, expected, effective_uid())
}

pub(crate) fn verify_as(
    root: &Path,
    expected: &TaskBinding,
    trusted_uid: u32,
) -> Result<(), String> {
    let dir = admin_dir(root, &expected.worktree)?;
    let bytes = authenticated_read(&dir.join(BINDING_FILE), trusted_uid)?;
    let recorded = TaskBinding::decode(&bytes).map_err(str::to_owned)?;
    let differing = recorded.differing_fields(expected);
    if differing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "recorded task binding differs in: {}",
            differing.join(", ")
        ))
    }
}

#[cfg(test)]
#[path = "worktree_binding_tests.rs"]
mod tests;
