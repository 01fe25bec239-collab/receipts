//! Host-owned probe/process port and the exact observation fingerprint.
//!
//! Host never launches a process itself. The admitted composition (the CLI
//! layer) owns the absolute verified executable, its explicit child
//! environment policy and the bounded Workspace runner; it implements
//! [`HostProbeProcessPort`]. Host only states which read-only probe argv it
//! needs and parses the bytes it gets back. There is no PATH-resolved
//! `claude` fallback.
//!
//! [`HostObservationFingerprintV1`] binds one observation to the executable
//! identity and content digest, its version output, and the exact bytes (or
//! typed absence/error) of every probe and settings source, plus the host and
//! project context. Equality is exact byte equality; metadata such as mtimes
//! and caller validity flags are never part of it. A cached report may be
//! reused only for an identical, complete fingerprint.

use std::path::PathBuf;
use std::time::Duration;

use crate::HostId;
use crate::host_capability_observation::{
    HostCapabilityObservationError, HostCapabilityObservationSource,
};

/// One bounded read-only probe invocation requested by Host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProbeCommandV1 {
    /// Arguments after the executable; never a shell string.
    pub args: Vec<String>,
    pub current_dir: PathBuf,
    pub timeout: Duration,
    pub stdout_max_bytes: usize,
    pub stderr_max_bytes: usize,
}

/// The composition's verified executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExecutableIdentityV1 {
    /// Absolute canonical path of a regular executable file.
    pub canonical_path: PathBuf,
    /// Lowercase hex SHA-256 of the executable's bytes.
    pub content_sha256: String,
}

/// Implemented by the admitted composition, never by Host.
pub trait HostProbeProcessPort {
    /// The verified executable this port runs.
    fn executable(&self) -> Result<HostExecutableIdentityV1, HostCapabilityObservationError>;
    /// Runs one probe to successful completion and returns its full stdout;
    /// non-zero exit, timeout, oversize output or cleanup failure is an error.
    fn run(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, HostCapabilityObservationError>;
}

pub(crate) type Material = Result<Vec<u8>, HostCapabilityObservationError>;

/// Exact material one observation was derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObservationFingerprintV1 {
    pub(crate) host: HostId,
    pub(crate) project_root: PathBuf,
    /// `None` when the host needs no probe executable.
    pub(crate) executable: Option<Result<HostExecutableIdentityV1, HostCapabilityObservationError>>,
    pub(crate) version: Option<Material>,
    pub(crate) plugins: Option<Material>,
    pub(crate) sources: Vec<(HostCapabilityObservationSource, Material)>,
}

impl HostObservationFingerprintV1 {
    /// Every required input was actually observed: probes succeeded and each
    /// settings source was read or is definitively absent. Any other error is
    /// retained uncertainty and forbids reuse.
    pub fn is_complete(&self) -> bool {
        let probed = |m: &Option<Material>| m.as_ref().is_none_or(Result::is_ok);
        self.executable.as_ref().is_none_or(Result::is_ok)
            && probed(&self.version)
            && probed(&self.plugins)
            && self
                .sources
                .iter()
                .all(|(_, m)| matches!(m, Ok(_) | Err(HostCapabilityObservationError::Missing)))
    }

    /// Reuse rule: the cached fingerprint is identical and complete.
    pub fn permits_reuse_of(&self, cached: &Self) -> bool {
        self.is_complete() && self == cached
    }
}
