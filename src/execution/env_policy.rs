//! Typed, immutable child-environment allowlist (WE-W0-2 / H2, D1 Option C).
//!
//! A [`ProcessRunRequest`](crate::execution::ProcessRunRequest) without a
//! policy keeps the empty child environment. An explicitly opted-in
//! [`ChildEnvPolicy`] snapshots, once at construction, only the D1 names:
//!
//! * BASE: [`BASE_NAMES`] plus `PATH` filtered to absolute entries (empty and
//!   `.` entries dropped; `PATH` omitted when nothing survives);
//! * the selected provider's config name, and its API-key name only in
//!   [`ChildEnvAuthMode::ApiKey`]; never the other provider's names;
//! * [`NETWORK_NAMES`] only when an explicitly supplied project file says
//!   `network=on` *and* the caller's confirmation approves network;
//! * extras only from that exact confirmed project file.
//!
//! Missing parent values stay absent; nothing is read at spawn. [`REFUSED`]
//! names are rejected at construction regardless of source or confirmation.
//!
//! The provider, auth mode and [`ProjectEnvConfirmation`] are caller
//! admission inputs, not authenticated provider/executable attestation or an
//! authenticated user-consent record. Debug output and errors carry variable
//! names only, never values or confirmed bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::io::Read as _;
use std::path::Path;
use std::process::Command;

use crate::execution::error::ExecutionError;

/// Base names admitted for every opted-in policy, when present in the parent.
pub const BASE_NAMES: [&str; 13] = [
    "HOME",
    "USER",
    "LOGNAME",
    "TERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
];

/// Names admitted only with project-file `network=on` plus confirmed approval.
pub const NETWORK_NAMES: [&str; 5] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
];

/// Exact names and `*`-suffixed prefixes refused at construction, always.
pub const REFUSED: [&str; 10] = [
    "LD_*",
    "DYLD_*",
    "NODE_OPTIONS",
    "BASH_ENV",
    "ENV",
    "PROMPT_COMMAND",
    "PYTHONPATH",
    "PERL5OPT",
    "RUBYOPT",
    "GIT_*",
];

/// Maximum project-file and confirmed-content size.
pub const MAX_PROJECT_ENV_BYTES: usize = 64 * 1024;

const PROVIDER_NAMES: [&str; 4] = [
    "CLAUDE_CONFIG_DIR",
    "ANTHROPIC_API_KEY",
    "CODEX_HOME",
    "OPENAI_API_KEY",
];

/// Caller-selected provider context. Never inferred from an executable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildEnvProvider {
    Claude,
    Codex,
}

impl ChildEnvProvider {
    fn config_name(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Codex => "CODEX_HOME",
        }
    }

    fn api_key_name(self) -> &'static str {
        match self {
            Self::Claude => "ANTHROPIC_API_KEY",
            Self::Codex => "OPENAI_API_KEY",
        }
    }
}

/// Caller-selected auth mode. Never inferred from which keys are present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildEnvAuthMode {
    Subscription,
    /// The user explicitly selected API-key mode for this provider.
    ApiKey,
}

/// The caller's explicit user-confirmation claim for one project file: the
/// exact confirmed bytes, the exact approved extra names and network approval.
#[derive(Clone, PartialEq, Eq)]
pub struct ProjectEnvConfirmation {
    confirmed_bytes: Box<[u8]>,
    approved_names: BTreeSet<String>,
    network_approved: bool,
}

impl ProjectEnvConfirmation {
    pub fn new(
        confirmed_bytes: impl Into<Vec<u8>>,
        approved_names: impl IntoIterator<Item = impl Into<String>>,
        network_approved: bool,
    ) -> Result<Self, ExecutionError> {
        let confirmed_bytes = confirmed_bytes.into();
        if confirmed_bytes.len() > MAX_PROJECT_ENV_BYTES {
            return Err(ExecutionError::ProjectEnvUnconfirmed {
                reason: "confirmed content exceeds 65536 bytes",
            });
        }
        let mut names = BTreeSet::new();
        for (index, name) in approved_names.into_iter().enumerate() {
            let name = name.into();
            admit_extra_name(&name, "confirmation name", index + 1)?;
            if names.contains(&name) {
                return Err(ExecutionError::ChildEnvNameDuplicated { name });
            }
            names.insert(name);
        }
        Ok(Self {
            confirmed_bytes: confirmed_bytes.into(),
            approved_names: names,
            network_approved,
        })
    }
}

impl fmt::Debug for ProjectEnvConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectEnvConfirmation")
            .field("approved_names", &self.approved_names)
            .field("network_approved", &self.network_approved)
            .finish_non_exhaustive()
    }
}

/// An immutable admitted name/value snapshot applied after `env_clear`.
///
/// There is no mutable map or setter: every entry passed the D1 rules above.
#[derive(Clone, PartialEq, Eq)]
pub struct ChildEnvPolicy {
    provider: ChildEnvProvider,
    auth_mode: ChildEnvAuthMode,
    entries: BTreeMap<String, OsString>,
}

impl ChildEnvPolicy {
    /// Base plus provider names from the current parent environment.
    pub fn new(
        provider: ChildEnvProvider,
        auth_mode: ChildEnvAuthMode,
    ) -> Result<Self, ExecutionError> {
        Self::build(provider, auth_mode, None, |name| std::env::var_os(name))
    }

    /// Like [`Self::new`], plus network names and extras admitted from one
    /// explicit absolute regular project file that exactly matches
    /// `confirmation`. The file is read once here and never again.
    pub fn with_project_file(
        provider: ChildEnvProvider,
        auth_mode: ChildEnvAuthMode,
        project_file: &Path,
        confirmation: &ProjectEnvConfirmation,
    ) -> Result<Self, ExecutionError> {
        Self::build(
            provider,
            auth_mode,
            Some((project_file, confirmation)),
            |name| std::env::var_os(name),
        )
    }

    /// Admitted variable names, sorted. Values are not exposed.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// `parent` is the construction-time snapshot source; tests inject a
    /// synthetic one instead of mutating the process environment.
    pub(crate) fn build(
        provider: ChildEnvProvider,
        auth_mode: ChildEnvAuthMode,
        project: Option<(&Path, &ProjectEnvConfirmation)>,
        parent: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, ExecutionError> {
        let (extras, network) = match project {
            Some((path, confirmation)) => admit_project_file(path, confirmation)?,
            None => (BTreeMap::new(), false),
        };
        let mut parent_names = BASE_NAMES.to_vec();
        parent_names.push(provider.config_name());
        if auth_mode == ChildEnvAuthMode::ApiKey {
            parent_names.push(provider.api_key_name());
        }
        if network {
            parent_names.extend(NETWORK_NAMES);
        }

        let mut entries = BTreeMap::new();
        for name in parent_names {
            if let Some(value) = parent(name) {
                entries.insert(name.to_owned(), value);
            }
        }
        if let Some(path) = parent("PATH").and_then(absolute_path_entries) {
            entries.insert("PATH".to_owned(), path);
        }
        for (name, value) in extras {
            entries.insert(name, value.into());
        }
        for (name, value) in &entries {
            if value.as_encoded_bytes().contains(&0) {
                return Err(ExecutionError::ChildEnvValueInvalid { name: name.clone() });
            }
        }
        Ok(Self {
            provider,
            auth_mode,
            entries,
        })
    }

    pub(crate) fn apply(&self, command: &mut Command) {
        command.envs(&self.entries);
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &BTreeMap<String, OsString> {
        &self.entries
    }
}

impl fmt::Debug for ChildEnvPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChildEnvPolicy")
            .field("provider", &self.provider)
            .field("auth_mode", &self.auth_mode)
            .field("names", &self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Keeps only absolute entries; `None` when none survive (never an empty PATH).
fn absolute_path_entries(path: OsString) -> Option<OsString> {
    let kept: Vec<_> = std::env::split_paths(&path)
        .filter(|entry| entry.is_absolute())
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

fn is_refused(name: &str) -> bool {
    REFUSED.iter().any(|rule| match rule.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *rule,
    })
}

/// Syntax, refusal and reservation checks for one extra name.
fn admit_extra_name(
    name: &str,
    origin: &'static str,
    position: usize,
) -> Result<(), ExecutionError> {
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == b'_')
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if !valid {
        return Err(ExecutionError::ChildEnvNameInvalid { origin, position });
    }
    if is_refused(name) {
        return Err(ExecutionError::ChildEnvNameRefused {
            name: name.to_owned(),
        });
    }
    if name == "PATH"
        || BASE_NAMES.contains(&name)
        || NETWORK_NAMES.contains(&name)
        || PROVIDER_NAMES.contains(&name)
    {
        return Err(ExecutionError::ChildEnvNameReserved {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Reads once, requires exact confirmed bytes, then parses and binds names
/// and network approval. Returns admitted extras and the network decision.
fn admit_project_file(
    path: &Path,
    confirmation: &ProjectEnvConfirmation,
) -> Result<(BTreeMap<String, String>, bool), ExecutionError> {
    let rejected = |reason| ExecutionError::ProjectEnvFileRejected { reason };
    if !path.is_absolute() {
        return Err(rejected("path is not absolute"));
    }
    // No pathname precheck: whatever entry is present at open time is opened
    // nonblocking (a FIFO without a writer cannot stall construction), and
    // only the opened handle's own metadata decides admission.
    let file = open_nonblocking(path)?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err(rejected("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROJECT_ENV_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| rejected("could not be read"))?;
    if bytes.len() > MAX_PROJECT_ENV_BYTES {
        return Err(rejected("exceeds 65536 bytes"));
    }
    if *bytes != *confirmation.confirmed_bytes {
        return Err(ExecutionError::ProjectEnvUnconfirmed {
            reason: "file content differs from confirmed content",
        });
    }

    let (extras, network) = parse_project_env(&bytes)?;
    if !extras.keys().eq(confirmation.approved_names.iter()) {
        return Err(ExecutionError::ProjectEnvUnconfirmed {
            reason: "extra names differ from approved names",
        });
    }
    if network != confirmation.network_approved {
        return Err(ExecutionError::ProjectEnvUnconfirmed {
            reason: "network directive and network approval differ",
        });
    }
    Ok((extras, network))
}

/// `O_NONBLOCK`, only for targets whose value was checked against local
/// authoritative definitions: the macOS SDK `sys/fcntl.h` (`0x00000004`) and
/// libc 0.2.189 Linux gnu/musl and Android x86/x86_64/arm/aarch64 (`2048`).
/// This is the same platform set as timed execution and `stdin.rs`.
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const NONBLOCKING_OPEN_FLAG: Option<i32> = Some(0x4);
#[cfg(all(
    any(target_os = "linux", target_os = "android"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64"
    )
))]
const NONBLOCKING_OPEN_FLAG: Option<i32> = Some(0x800);
#[cfg(not(any(
    all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64"
        )
    )
)))]
const NONBLOCKING_OPEN_FLAG: Option<i32> = None;

/// Opens read-only with `O_NONBLOCK` so the open itself never waits for a
/// FIFO writer. Unsupported targets are refused before any open is attempted.
fn open_nonblocking(path: &Path) -> Result<std::fs::File, ExecutionError> {
    let rejected = |reason| ExecutionError::ProjectEnvFileRejected { reason };
    let Some(flag) = NONBLOCKING_OPEN_FLAG else {
        return Err(rejected("nonblocking open unsupported on this platform"));
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flag)
            .open(path)
            .map_err(|_| rejected("could not be opened"))
    }
    #[cfg(not(unix))]
    {
        let _ = (flag, path);
        Err(rejected("nonblocking open unsupported on this platform"))
    }
}

/// Strict UTF-8 LF format: `NAME=VALUE` lines split at the first `=`, plus
/// at most one exact `network=on`. No comments, trimming, quoting or escapes.
fn parse_project_env(bytes: &[u8]) -> Result<(BTreeMap<String, String>, bool), ExecutionError> {
    let malformed = |line, reason| ExecutionError::ProjectEnvFileMalformed { line, reason };
    let text = std::str::from_utf8(bytes).map_err(|_| malformed(0, "not UTF-8"))?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let mut extras = BTreeMap::new();
    let mut network = false;
    for (index, line) in text.split('\n').enumerate() {
        let number = index + 1;
        if line.is_empty() {
            continue;
        }
        if line.contains(['\r', '\0']) {
            return Err(malformed(number, "contains CR or NUL"));
        }
        if line == "network=on" {
            if network {
                return Err(malformed(number, "repeated network directive"));
            }
            network = true;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(malformed(number, "missing '='"));
        };
        admit_extra_name(name, "project file line", number)?;
        if extras.contains_key(name) {
            return Err(ExecutionError::ChildEnvNameDuplicated {
                name: name.to_owned(),
            });
        }
        extras.insert(name.to_owned(), value.to_owned());
    }
    Ok((extras, network))
}

#[cfg(all(test, unix))]
#[path = "env_policy_tests.rs"]
mod env_policy_tests;
