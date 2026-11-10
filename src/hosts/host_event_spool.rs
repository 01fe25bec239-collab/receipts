//! Append-only untrusted Host event spool (HI-W2-2b).
//!
//! Host input never writes State. A hook process appends validated
//! [`NormalizedHostEvent`] lines (the HI-WIRE codec, one per line) to
//! `<project>/.receipts/inbox/host-events.jsonl`; the core ingests them
//! through validation later (HI-W4-1). Nothing here opens, names or reaches
//! a State database.
//!
//! Safety controls on every append:
//! * the project root must be absolute and already canonical; `.receipts`
//!   and `inbox` are created `0700` and must be real directories (never
//!   symlinks) owned by the effective user with no group/other write;
//! * the spool is opened `O_APPEND | O_NOFOLLOW | O_NONBLOCK` (the shared
//!   checked per-target table), created `0600`; the opened descriptor must be
//!   a regular, single-link file owned by the effective user with mode
//!   exactly `0600` — a substituted symlink, FIFO, hard link or foreign file
//!   is refused, never repaired;
//! * one line is at most [`HOST_EVENT_SPOOL_LINE_MAX_BYTES`] and is written
//!   with one `write` call (no partial line is reported as success); the file
//!   never grows past [`HOST_EVENT_SPOOL_MAX_BYTES`];
//! * data is synced before success is reported.
//!
//! Directory traversal and creation are descriptor-relative; each append locks
//! the opened inode through tail/size checks, write and sync. A partial tail is
//! retained and refuses append. Same-user hostile writers are outside custody.

use crate::NormalizedHostEvent;
use crate::private_fs::{HostDirectory, lock};
use crate::wire::normalized_host_event_to_json;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const HOST_EVENT_SPOOL_LINE_MAX_BYTES: usize = 65_536;
pub const HOST_EVENT_SPOOL_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Spool location relative to the project root.
pub const HOST_EVENT_SPOOL_RELATIVE_PATH: &str = ".receipts/inbox/host-events.jsonl";

/// Payload-free spool failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEventSpoolError {
    /// The project root is relative or not in canonical form.
    RootNotCanonical,
    /// A spool directory or the file is a symlink, non-directory or
    /// non-regular file, or has extra hard links.
    UnsafeEntry,
    /// An entry is owned by another user.
    NotOwned,
    /// Directory is group/other writable or the file mode is not `0600`.
    Permissions,
    LineTooLarge,
    SpoolFull,
    /// Existing bytes lack a complete newline-terminated tail; no bytes were changed.
    IncompleteTail,
    /// No checked no-follow constant for this target.
    Unsupported,
    Io,
}

impl std::fmt::Display for HostEventSpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "host event spool refused: {self:?}")
    }
}

impl std::error::Error for HostEventSpoolError {}

/// Writer for one project's spool. Holding it grants no State access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEventSpool {
    project_root: PathBuf,
    directory: std::sync::Arc<HostDirectory>,
}

fn io(e: std::io::Error) -> HostEventSpoolError {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => HostEventSpoolError::Permissions,
        std::io::ErrorKind::InvalidInput => HostEventSpoolError::UnsafeEntry,
        _ if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
            HostEventSpoolError::UnsafeEntry
        }
        _ => HostEventSpoolError::Io,
    }
}

impl HostEventSpool {
    pub fn for_project(project_root: &Path) -> Result<Self, HostEventSpoolError> {
        let canonical = fs::canonicalize(project_root).map_err(io)?;
        if !project_root.is_absolute() || canonical != project_root {
            return Err(HostEventSpoolError::RootNotCanonical);
        }
        let directory = std::sync::Arc::new(HostDirectory::open(&canonical).map_err(io)?);
        Ok(Self {
            project_root: canonical,
            directory,
        })
    }

    pub fn path(&self) -> PathBuf {
        self.project_root.join(HOST_EVENT_SPOOL_RELATIVE_PATH)
    }

    /// Appends one event as one complete line.
    pub fn append(&self, event: &NormalizedHostEvent) -> Result<(), HostEventSpoolError> {
        let mut line = normalized_host_event_to_json(event);
        line.push('\n');
        if line.len() > HOST_EVENT_SPOOL_LINE_MAX_BYTES {
            return Err(HostEventSpoolError::LineTooLarge);
        }
        let root = &self.directory;
        root.verify().map_err(io)?;
        let receipts = root
            .child(std::ffi::OsStr::new(".receipts"), true)
            .map_err(io)?;
        let inbox = receipts
            .child(std::ffi::OsStr::new("inbox"), true)
            .map_err(io)?;
        let name = std::ffi::OsStr::new("host-events.jsonl");
        let mut file = inbox.open_file(name, true, true).map_err(io)?;
        lock(&file).map_err(io)?;
        inbox.verify_file(name, &file, true).map_err(io)?;
        let meta = file.metadata().map_err(io)?;
        if meta.len() + line.len() as u64 > HOST_EVENT_SPOOL_MAX_BYTES {
            return Err(HostEventSpoolError::SpoolFull);
        }
        if meta.len() != 0 {
            file.seek(SeekFrom::End(-1)).map_err(io)?;
            let mut tail = [0u8];
            file.read_exact(&mut tail).map_err(io)?;
            if tail[0] != b'\n' {
                return Err(HostEventSpoolError::IncompleteTail);
            }
        }
        if file.write(line.as_bytes()).map_err(io)? != line.len() {
            return Err(HostEventSpoolError::Io);
        }
        file.sync_data().map_err(io)
    }
}

#[cfg(test)]
#[path = "host_event_spool_tests.rs"]
mod tests;
