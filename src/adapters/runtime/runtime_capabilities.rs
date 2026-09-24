use crate::{ClaudeCapabilityProbeReport, CodexCapabilityProbeReport};

/// Provider-specific probe evidence, or no trustworthy complete report.
///
/// This carrier makes no cross-provider equivalence, policy, or routing claim.
/// Probe failures remain errors at the probe boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCapabilities {
    Codex(CodexCapabilityProbeReport),
    Claude(ClaudeCapabilityProbeReport),
    Unknown,
}
