//! Closed host identity for Host Integration.
//!
//! [`HostId`] enumerates exactly the hosts the frozen architecture
//! recognizes for adapter binding. It is identity only: no capability
//! claims, no detection logic, and no host behavior of any kind attach to
//! it.

/// Identifies the host behind a [`HostAdapter`](crate::HostAdapter).
///
/// The set is closed by contract: exactly these three identities exist,
/// and matching over them is exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostId {
    /// Claude Code.
    ClaudeCode,
    /// Codex.
    Codex,
    /// Headless (non-interactive) execution.
    Headless,
}

impl HostId {
    /// Stable diagnostic name of this host identity.
    pub fn as_str(self) -> &'static str {
        match self {
            HostId::ClaudeCode => "CLAUDE_CODE",
            HostId::Codex => "CODEX",
            HostId::Headless => "HEADLESS",
        }
    }
}
