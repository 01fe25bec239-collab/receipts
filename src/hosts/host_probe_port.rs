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
//! identity, content digest and probe policy, and to the SHA-256 digest and
//! length (or typed absence/error) of the version output, every probe and
//! every settings source, plus the host and project context. Raw probe and
//! settings bytes are never retained. Metadata such as mtimes and caller
//! validity flags are never part of it.
//!
//! [`HostCapabilityCacheEntryV1`] couples one complete fingerprint with the
//! mode override and the report selected from that same observation. It has
//! no public constructor: only the real refresh path mints it, so a report
//! and a fingerprint obtained independently can never be paired into reuse.

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::host_capability_observation::{
    HostCapabilityObservationError, HostCapabilityObservationSource,
};
use crate::{HostCapabilityModeOverride, HostCapabilityReportNonTemporalCore, HostId};

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
    /// Value-free description of the admitted probe environment policy.
    pub probe_policy: String,
}

/// Implemented by the admitted composition, never by Host.
pub trait HostProbeProcessPort {
    /// The verified executable this port runs.
    fn executable(&self) -> Result<HostExecutableIdentityV1, HostCapabilityObservationError>;
    /// Runs one probe to successful completion and returns its full stdout;
    /// non-zero exit, timeout, oversize output or cleanup failure is an error.
    fn run(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, HostCapabilityObservationError>;
}

/// Length and SHA-256 of observed bytes; the bytes themselves are dropped.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ContentDigest {
    len: usize,
    sha256: [u8; 32],
}

impl std::fmt::Debug for ContentDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex: String = self.sha256.iter().map(|b| format!("{b:02x}")).collect();
        write!(f, "{}:sha256:{hex}", self.len)
    }
}

pub(crate) type Material = Result<ContentDigest, HostCapabilityObservationError>;

pub(crate) fn digest(bytes: &Result<Vec<u8>, HostCapabilityObservationError>) -> Material {
    bytes.as_ref().map_err(|e| *e).map(|b| ContentDigest {
        len: b.len(),
        sha256: Sha256::digest(b).into(),
    })
}

/// Digest-only material one observation was derived from.
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

}

/// One complete observation's fingerprint, the mode override it was
/// composed with and the report selected from it. Minted only by
/// `refresh_host_capability_report`; no public constructor or field.
///
/// ```compile_fail,E0451
/// use receipts_host_integration::{HostCapabilityCacheEntryV1,
///     HostCapabilityReportNonTemporalCore, HostObservationFingerprintV1};
/// fn forge(fingerprint: HostObservationFingerprintV1, report: HostCapabilityReportNonTemporalCore) {
///     let _ = HostCapabilityCacheEntryV1 { fingerprint, mode_override: None, report };
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityCacheEntryV1 {
    pub(crate) fingerprint: HostObservationFingerprintV1,
    pub(crate) mode_override: Option<HostCapabilityModeOverride>,
    pub(crate) report: HostCapabilityReportNonTemporalCore,
}

impl HostCapabilityCacheEntryV1 {
    /// `None` unless the fingerprint is complete.
    pub(crate) fn mint(
        fingerprint: HostObservationFingerprintV1,
        mode_override: Option<HostCapabilityModeOverride>,
        report: HostCapabilityReportNonTemporalCore,
    ) -> Option<Self> {
        fingerprint.is_complete().then_some(Self {
            fingerprint,
            mode_override,
            report,
        })
    }

    pub fn fingerprint(&self) -> &HostObservationFingerprintV1 {
        &self.fingerprint
    }

    pub fn report(&self) -> &HostCapabilityReportNonTemporalCore {
        &self.report
    }

    /// Reuse rule: `current` (freshly minted) and `self` agree on every
    /// report-shaping input — host, project, executable identity and probe
    /// policy, every material digest and the mode override — and on the
    /// selected report itself.
    pub fn permits_reuse_by(&self, current: &Self) -> bool {
        self == current
    }
}
