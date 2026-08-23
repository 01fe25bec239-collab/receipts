//! Pure, deterministic host detection/selection policy.
//!
//! This module resolves already-observed host-presence facts, supplied by
//! its caller, into exactly one [`HostId`]. It is a resolution policy only:
//! it observes nothing. No environment, process, filesystem, plugin,
//! configuration, or network probing exists here; discovering host-presence
//! facts belongs to later concrete host slices that will feed this
//! boundary.
//!
//! Resolution semantics (normative):
//!
//! 1. An explicit override is authoritative and wins over every automatic
//!    signal combination, including conflicting ones;
//! 2. Claude-only automatic detection resolves to
//!    [`HostId::ClaudeCode`];
//! 3. Codex-only automatic detection resolves to [`HostId::Codex`];
//! 4. no automatic interactive-host signal resolves to [`HostId::Headless`]
//!    (the non-host fallback for direct CLI / CI operation);
//! 5. simultaneous Claude + Codex automatic detection without an explicit
//!    override is an explicit typed error — never a silent choice.
//!
//! The resolver is pure: no I/O, no subprocesses, no global state, no
//! async runtime, no dependencies. Equivalent inputs always produce
//! equivalent outputs.

use std::fmt;

use crate::host_id::HostId;

/// Already-observed host-presence facts supplied by the caller.
///
/// Each field records whether the corresponding interactive host was
/// observed present by whatever probing layer invoked this boundary. This
/// type performs no observation itself and carries no capability claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HostDetectionSignals {
    /// Claude Code was detected as present.
    pub claude_detected: bool,
    /// Codex was detected as present.
    pub codex_detected: bool,
}

impl HostDetectionSignals {
    /// Signals with neither interactive host observed: the direct CLI / CI
    /// path.
    pub const NONE: Self = Self {
        claude_detected: false,
        codex_detected: false,
    };

    /// Exactly Claude Code observed.
    pub const CLAUDE_ONLY: Self = Self {
        claude_detected: true,
        codex_detected: false,
    };

    /// Exactly Codex observed.
    pub const CODEX_ONLY: Self = Self {
        claude_detected: false,
        codex_detected: true,
    };

    /// Both interactive hosts observed: ambiguous without an explicit
    /// override.
    pub const BOTH: Self = Self {
        claude_detected: true,
        codex_detected: true,
    };
}

/// Explicit failure of pure host resolution.
///
/// Every failure mode of resolving supplied signals into one host identity
/// surfaces as an explicit typed error; ambiguity is never converted into a
/// silently chosen host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HostDetectionError {
    /// Automatic detection observed both interactive hosts simultaneously
    /// and no explicit override was supplied.
    ///
    /// Choosing either host — or falling back to Headless — would be an
    /// arbitrary silent decision, so resolution fails closed instead. The
    /// conflicting identities are preserved for diagnosis.
    AmbiguousAutomaticDetection {
        /// The two automatically detected hosts, in conflict with each
        /// other.
        detected: [HostId; 2],
    },
}

impl fmt::Display for HostDetectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostDetectionError::AmbiguousAutomaticDetection { detected } => write!(
                f,
                "ambiguous automatic host detection: {} and {} were both detected; \
                 supply an explicit host override to disambiguate",
                detected[0].as_str(),
                detected[1].as_str(),
            ),
        }
    }
}

impl std::error::Error for HostDetectionError {}

/// Resolves supplied host-presence facts plus an optional explicit override
/// into exactly one host identity.
///
/// An explicit override of `Some(id)` is returned verbatim and takes
/// precedence over every automatic signal combination. Without an override,
/// single-host detection resolves to that host, absence of both signals
/// resolves to [`HostId::Headless`], and simultaneous Claude + Codex
/// detection fails with
/// [`HostDetectionError::AmbiguousAutomaticDetection`].
///
/// Pure and deterministic: equivalent arguments always produce an
/// equivalent result.
pub fn resolve_host(
    signals: HostDetectionSignals,
    explicit_override: Option<HostId>,
) -> Result<HostId, HostDetectionError> {
    if let Some(host) = explicit_override {
        return Ok(host);
    }

    match (signals.claude_detected, signals.codex_detected) {
        (false, false) => Ok(HostId::Headless),
        (true, false) => Ok(HostId::ClaudeCode),
        (false, true) => Ok(HostId::Codex),
        (true, true) => Err(HostDetectionError::AmbiguousAutomaticDetection {
            detected: [HostId::ClaudeCode, HostId::Codex],
        }),
    }
}
