//! The typed, orchestrator-owned timeout policy for bounded runs.

use std::time::Duration;

use crate::execution::error::ExecutionError;

/// An immutable timeout policy supplied explicitly by the orchestrator.
///
/// The runner never invents a default deadline and never reads timeout
/// values from the environment or from configuration files: bounded
/// execution happens only through an explicitly constructed policy passed
/// to [`run_with_timeout`](crate::run_with_timeout). The unbounded
/// [`run`](crate::run) API is untouched by this type.
///
/// Construction is total validation: a policy that could not satisfy the
/// frozen terminate → bounded-grace → forced-kill sequence cannot exist.
/// Both intervals must be finite (a [`Duration`] is always finite) and
/// strictly greater than zero:
///
/// * `run_timeout == Duration::ZERO` is rejected — a zero run deadline
///   could not distinguish "monitor the child" from "terminate
///   immediately";
/// * `termination_grace == Duration::ZERO` is rejected — the frozen
///   sequence requires a real bounded grace opportunity between graceful
///   termination delivery and any forced kill.
///
/// Invalid values are refused outright; they are never silently clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessTimeoutPolicy {
    run_timeout: Duration,
    termination_grace: Duration,
}

impl ProcessTimeoutPolicy {
    /// Validates and assembles an explicit timeout policy.
    ///
    /// Fails closed with [`ExecutionError::InvalidTimeoutPolicy`] when
    /// either interval is zero, naming the offending interval in the error
    /// detail.
    pub fn new(run_timeout: Duration, termination_grace: Duration) -> Result<Self, ExecutionError> {
        if run_timeout.is_zero() {
            return Err(ExecutionError::InvalidTimeoutPolicy {
                detail: "run_timeout must be greater than zero; a zero run deadline is refused \
                         because it could never observe the child before terminating it"
                    .to_string(),
            });
        }
        if termination_grace.is_zero() {
            return Err(ExecutionError::InvalidTimeoutPolicy {
                detail: "termination_grace must be greater than zero; the frozen sequence \
                         requires a real bounded grace opportunity between graceful termination \
                         and any forced kill"
                    .to_string(),
            });
        }
        Ok(Self {
            run_timeout,
            termination_grace,
        })
    }

    /// How long the child may run before the run is classified as timed
    /// out.
    pub fn run_timeout(&self) -> Duration {
        self.run_timeout
    }

    /// How long the runner waits after graceful termination delivery
    /// before resorting to a forced kill.
    pub fn termination_grace(&self) -> Duration {
        self.termination_grace
    }
}
