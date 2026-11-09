//! Shared durable claim/seal journal for the Review sink compositions.
//!
//! This is the test composition's implementation of the `ReviewRecordSink`
//! claim and seal contract over plain files: an `O_EXCL` claim file (atomic
//! across threads and processes, fsynced), and a sealed result created
//! atomically with `link(2)` (create-if-absent) so a sealed package can never
//! be replaced. State's own atomic/claim support is another owner's pending
//! work; this proves the Review side of the contract and restart recovery,
//! and is not a production journal.

#![allow(dead_code)]

use receipts_review_integration::{ReviewClaim, SealedReview};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub struct FileJournal {
    dir: PathBuf,
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("fsync {}: {e}", dir.display()))
}

impl FileJournal {
    pub fn new(dir: &Path) -> Self {
        fs::create_dir_all(dir).expect("journal directory");
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn claim_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.claim"))
    }
    fn sealed_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.sealed"))
    }

    /// Atomic durable compare-and-set.
    pub fn claim(&self, key: &str) -> Result<ReviewClaim, String> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.claim_path(key))
        {
            Ok(file) => {
                file.sync_all().map_err(|e| e.to_string())?;
                sync_dir(&self.dir)?;
                Ok(ReviewClaim::Acquired)
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => match fs::read(self.sealed_path(key))
            {
                Ok(bytes) => Ok(ReviewClaim::Sealed(SealedReview::from_bytes(&bytes)?)),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(ReviewClaim::InFlight),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    }

    /// Durable, create-if-absent seal; identical re-seal is a no-op.
    pub fn seal(&self, key: &str, sealed: &SealedReview) -> Result<(), String> {
        if !self.claim_path(key).exists() {
            return Err("no claim to seal under".into());
        }
        let bytes = sealed.to_bytes();
        let target = self.sealed_path(key);
        let tmp = self.dir.join(format!(
            "{key}.tmp-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        let linked = fs::hard_link(&tmp, &target);
        let _ = fs::remove_file(&tmp);
        match linked {
            Ok(()) => sync_dir(&self.dir),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                if fs::read(&target).map_err(|e| e.to_string())? == bytes {
                    Ok(())
                } else {
                    Err("a different package is already sealed".into())
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn claim_files(&self) -> usize {
        fs::read_dir(&self.dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".claim"))
            .count()
    }
}

/// Deterministic ULID-shaped event identity. State orders events by id, so
/// the id is `claim time (48 bits) | event ordinal (16 bits) | key hash (64
/// bits)`: stable across retries and restarts (the claim file's time and the
/// event's index in the package do not change), ordered by claim and then by
/// position in the package.
pub fn event_id_for(claim_ms: u64, ordinal: u16, key: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    let value = (u128::from(claim_ms & ((1 << 48) - 1)) << 80)
        | (u128::from(ordinal) << 64)
        | u128::from(h.finish());
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    (0..26)
        .map(|i| ALPHABET[((value >> (5 * (25 - i))) & 31) as usize] as char)
        .collect()
}

impl FileJournal {
    /// The claim's creation time in ms since the epoch (fixed at claim).
    pub fn claim_time_ms(&self, key: &str) -> u64 {
        let modified = fs::metadata(self.claim_path(key))
            .and_then(|m| m.modified())
            .expect("claim file");
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}
