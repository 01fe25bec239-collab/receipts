//! R9 filesystem case-alias checking for write-scope verification.
//!
//! The A0 path language stays case-sensitive and path bytes are never
//! normalized. Separately, on a filesystem that folds case, two Git paths
//! that differ only by case name ONE file, so a worker could change a
//! protected file through an alias spelling. This module reports such alias
//! pairs between each changed path and every other changed or tracked path,
//! classifying the alias with the shared [`WritePathSet`] matcher.
//!
//! Equivalence is established only for ASCII case folding on a filesystem
//! probed as case-insensitive. A non-ASCII changed path on a non-sensitive
//! (or unknown) filesystem is reported as unestablished: the filesystem's own
//! Unicode folding/normalization may or may not alias it. Both outcomes fail
//! verification closed. Limits: one probe at the worktree root, so
//! per-directory case-folding attributes below it (e.g. Linux casefold
//! directories) are not detected; tracked listings share the bounded Git
//! evidence limit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::write_path_pattern::WritePathSet;

/// Probed case behavior of the filesystem holding the worktree root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemCaseSensitivity {
    Sensitive,
    Insensitive,
    /// No ASCII-letter path component could be probed, or probing failed.
    Unknown,
}

/// One changed path and a differently spelled path the filesystem may treat
/// as the same file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CaseAliasCollision {
    path: String,
    alias: String,
    alias_allowed: bool,
    alias_forbidden: bool,
}

impl CaseAliasCollision {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn alias(&self) -> &str {
        &self.alias
    }
    /// Whether the alias spelling is inside the allowed write paths.
    pub fn alias_allowed(&self) -> bool {
        self.alias_allowed
    }
    /// Whether the alias spelling matches a forbidden write path.
    pub fn alias_forbidden(&self) -> bool {
        self.alias_forbidden
    }
}

/// R9 outcome. Only [`Self::CaseSensitiveFilesystem`] and
/// [`Self::NoAliases`] permit a passing verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteScopeCaseAliasCheck {
    /// The probe established a case-sensitive filesystem; distinct spellings
    /// are distinct files.
    CaseSensitiveFilesystem,
    /// No changed path has a case alias and none is non-ASCII.
    NoAliases {
        filesystem: FilesystemCaseSensitivity,
    },
    /// Alias pairs exist, or equivalence could not be established.
    Aliases {
        filesystem: FilesystemCaseSensitivity,
        collisions: Vec<CaseAliasCollision>,
        unestablished: Vec<String>,
    },
}

impl WriteScopeCaseAliasCheck {
    pub fn passes(&self) -> bool {
        !matches!(self, Self::Aliases { .. })
    }
}

/// Read-only probe: flips the case of the deepest ASCII-letter component of
/// the canonical `directory` and compares file identities.
pub fn probe_case_sensitivity(directory: &Path) -> FilesystemCaseSensitivity {
    let Ok(canonical) = std::fs::canonicalize(directory) else {
        return FilesystemCaseSensitivity::Unknown;
    };
    let components: Vec<_> = canonical.components().collect();
    for index in (0..components.len()).rev() {
        let std::path::Component::Normal(name) = components[index] else {
            continue;
        };
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().any(|byte| byte.is_ascii_alphabetic()) {
            continue;
        }
        let flipped: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        let prefix: PathBuf = components[..index].iter().collect();
        let original = prefix.join(name);
        let alias = prefix.join(flipped);
        return match (identity(&original), std::fs::symlink_metadata(&alias)) {
            (Some(original), Ok(_)) if identity(&alias) == Some(original) => {
                FilesystemCaseSensitivity::Insensitive
            }
            (Some(_), Ok(_)) => FilesystemCaseSensitivity::Sensitive,
            (Some(_), Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                FilesystemCaseSensitivity::Sensitive
            }
            _ => FilesystemCaseSensitivity::Unknown,
        };
    }
    FilesystemCaseSensitivity::Unknown
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// Pure alias analysis over already admitted path identities.
pub(crate) fn analyze(
    filesystem: FilesystemCaseSensitivity,
    changed: &[String],
    tracked: &BTreeSet<String>,
    allowed: &WritePathSet,
    forbidden: &WritePathSet,
) -> WriteScopeCaseAliasCheck {
    if filesystem == FilesystemCaseSensitivity::Sensitive {
        return WriteScopeCaseAliasCheck::CaseSensitiveFilesystem;
    }
    let mut by_fold: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for path in tracked.iter().chain(changed) {
        if path.is_ascii() {
            by_fold
                .entry(path.to_ascii_lowercase())
                .or_default()
                .insert(path);
        }
    }
    let mut collisions = BTreeSet::new();
    let mut unestablished = Vec::new();
    for path in changed {
        if !path.is_ascii() {
            unestablished.push(path.clone());
            continue;
        }
        for alias in &by_fold[&path.to_ascii_lowercase()] {
            if *alias != path {
                collisions.insert(CaseAliasCollision {
                    path: path.clone(),
                    alias: (*alias).to_owned(),
                    alias_allowed: allowed.matches(alias),
                    alias_forbidden: forbidden.matches(alias),
                });
            }
        }
    }
    if collisions.is_empty() && unestablished.is_empty() {
        WriteScopeCaseAliasCheck::NoAliases { filesystem }
    } else {
        WriteScopeCaseAliasCheck::Aliases {
            filesystem,
            collisions: collisions.into_iter().collect(),
            unestablished,
        }
    }
}
