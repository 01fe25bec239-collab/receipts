//! Adapter observations only. Trusted Model Intelligence owns durable registry writes.
use crate::{RuntimeAuthStatus, RuntimeCapabilities, RuntimeObservation};
use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Unknown,
    Installed,
    Unavailable,
    VersionUnsupported,
}

/// A CLI version/help probe establishes installation, not provider availability,
/// account quota, model entitlement or permission to dispatch.
pub struct Provider {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    pub auth_state: RuntimeAuthStatus,
    pub health: RuntimeObservation,
    pub models: RuntimeObservation,
    pub usage: RuntimeObservation,
    pub quota: RuntimeObservation,
    pub install_state: InstallState,
    pub last_refreshed_at: Option<SystemTime>,
    pub capabilities: RuntimeCapabilities,
}

pub(crate) struct ProbeCache {
    ttl: Duration,
    entry: Mutex<Option<(Instant, SystemTime, RuntimeCapabilities)>>,
}
impl ProbeCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entry: Mutex::new(None),
        }
    }
    pub(crate) fn capabilities(&self) -> RuntimeCapabilities {
        let entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        entry
            .as_ref()
            .filter(|(at, _, _)| at.elapsed() < self.ttl)
            .map(|(_, _, report)| report.clone())
            .unwrap_or(RuntimeCapabilities::Unknown)
    }
    pub(crate) fn observe(
        &self,
        probe: impl FnOnce() -> RuntimeCapabilities,
    ) -> (RuntimeCapabilities, SystemTime) {
        let mut entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, wall, report)) = &*entry {
            if at.elapsed() < self.ttl {
                return (report.clone(), *wall);
            }
        }
        let report = probe();
        let wall = SystemTime::now();
        *entry = Some((Instant::now(), wall, report.clone()));
        (report, wall)
    }
}

pub(crate) fn provider(
    id: &'static str,
    name: &'static str,
    report: RuntimeCapabilities,
    at: SystemTime,
) -> Provider {
    Provider {
        provider_id: id,
        display_name: name,
        auth_state: RuntimeAuthStatus::Unknown,
        health: RuntimeObservation::Unknown,
        models: RuntimeObservation::Unknown,
        usage: RuntimeObservation::Unknown,
        quota: RuntimeObservation::Unknown,
        install_state: match &report {
            RuntimeCapabilities::Unknown => InstallState::Unavailable,
            RuntimeCapabilities::Codex(p)
                if p.json != crate::CodexCapabilityEvidence::Supported =>
            {
                InstallState::VersionUnsupported
            }
            RuntimeCapabilities::Claude(p)
                if [
                    p.print,
                    p.input_format,
                    p.output_format,
                    p.no_session_persistence,
                    p.permission_mode,
                    p.verbose,
                ]
                .iter()
                .any(|v| *v != crate::ClaudeCapabilityEvidence::Supported) =>
            {
                InstallState::VersionUnsupported
            }
            _ => InstallState::Installed,
        },
        last_refreshed_at: Some(at),
        capabilities: report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn expiry_reprobes_and_failed_probe_replaces_old_evidence() {
        let cache = ProbeCache::new(Duration::ZERO);
        let calls = Cell::new(0);
        for _ in 0..2 {
            cache.observe(|| {
                calls.set(calls.get() + 1);
                RuntimeCapabilities::Unknown
            });
        }
        assert_eq!(calls.get(), 2);
        assert_eq!(cache.capabilities(), RuntimeCapabilities::Unknown);
        let cache = ProbeCache::new(Duration::from_secs(60));
        cache.observe(|| RuntimeCapabilities::Unknown);
        cache.observe(|| panic!("unexpired result must be cached"));
        let p = provider(
            "openai",
            "OpenAI",
            RuntimeCapabilities::Unknown,
            SystemTime::now(),
        );
        assert_eq!(p.install_state, InstallState::Unavailable);
        assert_eq!(p.health, RuntimeObservation::Unknown);
        assert_eq!(p.usage, RuntimeObservation::Unknown);
    }
}
