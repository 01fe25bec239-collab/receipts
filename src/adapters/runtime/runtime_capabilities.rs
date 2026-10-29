use std::{collections::BTreeMap, error::Error, fmt};

use crate::{ClaudeCapabilityProbeReport, CodexCapabilityProbeReport};

/// Provider-specific probe evidence, an open extension report, or no
/// trustworthy complete report.
///
/// This carrier makes no cross-provider equivalence, policy, or routing claim.
/// Probe failures remain errors at the probe boundary. `Extension` lets an
/// adapter outside the typed Codex/Claude reports return observations without
/// another variant; `Unknown` stays a separate whole-report no-evidence state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCapabilities {
    Codex(CodexCapabilityProbeReport),
    Claude(ClaudeCapabilityProbeReport),
    Extension(ExtensionCapabilityReport),
    Unknown,
}

/// One observed capability status; `Unknown` is an observation, not a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Unknown,
}

/// Immutable name→status observations from one runtime.
///
/// The runtime identifier and capability names are open, nonempty, exact
/// strings: no trimming, normalization, catalog, grammar or length ceiling.
/// Entries iterate in byte order of the name, independent of input order. A
/// missing name reports `Unknown`; it is never `Supported` or `Unsupported`,
/// and an empty report implies no capability. This asserts no authenticity,
/// probe freshness, routing, entitlement or admission. Debug hides all values.
#[derive(Clone, PartialEq, Eq)]
pub struct ExtensionCapabilityReport {
    runtime_id: Box<str>,
    entries: BTreeMap<Box<str>, CapabilityStatus>,
}

impl ExtensionCapabilityReport {
    /// Validate before retaining anything: empty identifiers and duplicate names
    /// refuse (a duplicate is never silently overwritten) and yield no report.
    pub fn new(
        runtime_id: &str,
        entries: impl IntoIterator<Item = (impl AsRef<str>, CapabilityStatus)>,
    ) -> Result<Self, ExtensionCapabilityReportError> {
        if runtime_id.is_empty() {
            return Err(ExtensionCapabilityReportError::EmptyRuntimeId);
        }
        let mut observed = BTreeMap::new();
        for (name, status) in entries {
            let name = name.as_ref();
            if name.is_empty() {
                return Err(ExtensionCapabilityReportError::EmptyCapabilityName);
            }
            if observed.insert(Box::from(name), status).is_some() {
                return Err(ExtensionCapabilityReportError::DuplicateCapabilityName);
            }
        }
        Ok(Self {
            runtime_id: runtime_id.into(),
            entries: observed,
        })
    }

    /// Exact caller identifier; not proof the report came from that runtime.
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    /// `None` means the name is absent; `Some(Unknown)` means observed unknown.
    pub fn get(&self, name: &str) -> Option<CapabilityStatus> {
        self.entries.get(name).copied()
    }

    /// Status for a name, with absence reported as `Unknown`.
    pub fn status(&self, name: &str) -> CapabilityStatus {
        self.get(name).unwrap_or(CapabilityStatus::Unknown)
    }

    /// Observed entries in byte order of the name.
    pub fn entries(&self) -> impl Iterator<Item = (&str, CapabilityStatus)> {
        self.entries.iter().map(|(name, status)| (&**name, *status))
    }
}

impl fmt::Debug for ExtensionCapabilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExtensionCapabilityReport([redacted])")
    }
}

/// Payload-free refusal; it never carries or formats the rejected values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionCapabilityReportError {
    EmptyRuntimeId,
    EmptyCapabilityName,
    DuplicateCapabilityName,
}

impl fmt::Display for ExtensionCapabilityReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRuntimeId => "capability report runtime identifier must not be empty",
            Self::EmptyCapabilityName => "capability name must not be empty",
            Self::DuplicateCapabilityName => "capability names must be unique within a report",
        })
    }
}
impl Error for ExtensionCapabilityReportError {}
