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
//! Limits: ancestor checks are sequential pathname checks, not descriptor
//! anchored; the owner/mode checks are controls, not proof that same-user
//! hostile code is excluded. The effective uid is learned without libc by
//! creating (then removing) one private probe file in the inbox.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::NormalizedHostEvent;
use crate::host_capability_observation::no_follow_open_flags;
use crate::wire::normalized_host_event_to_json;

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
}

fn io(_: std::io::Error) -> HostEventSpoolError {
    HostEventSpoolError::Io
}

impl HostEventSpool {
    pub fn for_project(project_root: &Path) -> Result<Self, HostEventSpoolError> {
        let canonical = fs::canonicalize(project_root).map_err(io)?;
        if !project_root.is_absolute() || canonical != project_root {
            return Err(HostEventSpoolError::RootNotCanonical);
        }
        Ok(Self {
            project_root: canonical,
        })
    }

    pub fn path(&self) -> PathBuf {
        self.project_root.join(HOST_EVENT_SPOOL_RELATIVE_PATH)
    }

    /// Appends one event as one complete line.
    pub fn append(&self, event: &NormalizedHostEvent) -> Result<(), HostEventSpoolError> {
        let (flags, eloop) =
            no_follow_open_flags().map_err(|_| HostEventSpoolError::Unsupported)?;
        let mut line = normalized_host_event_to_json(event);
        line.push('\n');
        if line.len() > HOST_EVENT_SPOOL_LINE_MAX_BYTES {
            return Err(HostEventSpoolError::LineTooLarge);
        }
        let receipts = self.project_root.join(".receipts");
        let inbox = receipts.join("inbox");
        for dir in [&receipts, &inbox] {
            match fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(io(e)),
            }
            if !fs::symlink_metadata(dir).map_err(io)?.is_dir() {
                return Err(HostEventSpoolError::UnsafeEntry);
            }
        }
        let uid = effective_uid(&inbox)?;
        for dir in [&receipts, &inbox] {
            let meta = fs::symlink_metadata(dir).map_err(io)?;
            if !meta.is_dir() {
                return Err(HostEventSpoolError::UnsafeEntry);
            }
            if meta.uid() != uid {
                return Err(HostEventSpoolError::NotOwned);
            }
            if meta.mode() & 0o022 != 0 {
                return Err(HostEventSpoolError::Permissions);
            }
        }
        // Stable non-regular entries are refused before open (a FIFO with
        // no reader would otherwise surface as a generic error); the
        // no-follow open and descriptor checks below still close the race.
        match fs::symlink_metadata(self.path()) {
            Ok(meta) if !meta.is_file() => return Err(HostEventSpoolError::UnsafeEntry),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(flags)
            .open(self.path())
            .map_err(|e| match e.raw_os_error() {
                Some(code) if code == eloop => HostEventSpoolError::UnsafeEntry,
                _ => HostEventSpoolError::Io,
            })?;
        let meta = file.metadata().map_err(io)?;
        if !meta.is_file() || meta.nlink() != 1 {
            return Err(HostEventSpoolError::UnsafeEntry);
        }
        if meta.uid() != uid {
            return Err(HostEventSpoolError::NotOwned);
        }
        if meta.mode() & 0o777 != 0o600 {
            return Err(HostEventSpoolError::Permissions);
        }
        if meta.len() + line.len() as u64 > HOST_EVENT_SPOOL_MAX_BYTES {
            return Err(HostEventSpoolError::SpoolFull);
        }
        if file.write(line.as_bytes()).map_err(io)? != line.len() {
            return Err(HostEventSpoolError::Io);
        }
        file.sync_data().map_err(io)
    }
}

/// The uid of a file this process creates is its effective uid.
fn effective_uid(dir: &Path) -> Result<u32, HostEventSpoolError> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let probe = dir.join(format!(".uid-probe-{}-{n}-{nanos}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)
        .map_err(io)?;
    let uid = file.metadata().map(|m| m.uid()).map_err(io);
    drop(file);
    let removed = fs::remove_file(&probe).map_err(io);
    let uid = uid?;
    removed?;
    Ok(uid)
}

#[cfg(test)]
#[path = "host_event_spool_tests.rs"]
mod tests;
