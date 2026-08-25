//! Argv-only local process-runner foundation for attempt execution
//! (`M-WORK-1`).
//!
//! This is the first, smallest independently testable process-execution
//! slice. It runs exactly one program described by a
//! [`ProcessRunRequest`]:
//!
//! * the executable is one explicit **absolute** path — no `PATH` lookup
//!   ever happens — validated to be an existing regular file with
//!   executable permission bits (where the platform exposes them) and
//!   executed in its canonical (`realpath`) form;
//! * recognized common shell interpreters are rejected at validation, so
//!   this foundation cannot become a shell command-string runner;
//! * arguments travel verbatim as discrete argv values; no shell exists
//!   anywhere and no argument is ever split or interpreted;
//! * the requested child working directory is canonicalized together with
//!   an explicit workspace root, and must resolve (component-wise) to the
//!   root itself or a descendant of it — symlink escapes, `..` chains past
//!   the root, and textual-prefix siblings are all refused, failing closed;
//! * the child inherits nothing from the parent environment: construction
//!   starts from `env_clear` and the allowlist is intentionally empty,
//!   because the absolute executable needs no `PATH` to be located;
//! * stdin is null (immediate EOF for the child), stdout and stderr are
//!   null: this slice returns exit metadata only.
//!
//! The result is typed exit-status metadata ([`ProcessRunOutcome`]): a
//! child exiting non-zero is normal runner output, never an error.
//! [`ExecutionError`] covers validation failures plus the distinct spawn,
//! wait, and process-control boundaries.
//!
//! Bounded execution ([`run_with_timeout`]) extends the same validated
//! spawn foundation with an explicitly supplied orchestrator-owned
//! [`ProcessTimeoutPolicy`]: monotonic deadline monitoring, graceful
//! termination (`SIGTERM`) of the attempt's own dedicated process group,
//! a bounded termination grace, a forced `SIGKILL` of that group only if
//! any attempt-owned member survives it, and a verified final reap that
//! proves no attempt-owned descendant remains. A timed-out run is
//! classified in the typed [`ProcessTermination`] outcome and can never be
//! silently reported as an ordinary successful completion. No default
//! timeout exists: the unbounded [`run`] API keeps its accepted semantics.
//!
//! Deliberately excluded here (later runner slices): output capture,
//! digests, checkpoints, recovery. Also excluded by architecture: sandbox
//! enforcement of any kind. The workspace boundary provides workspace
//! isolation only — it is NOT a security sandbox; filesystem, network,
//! and process isolation belong to the runtime/host sandbox layer.

mod error;
mod outcome;
mod request;
mod runner;
mod timeout;
#[cfg(unix)]
mod unix_signal;

pub use error::ExecutionError;
pub use outcome::{ProcessRunOutcome, ProcessTermination};
pub use request::ProcessRunRequest;
pub use runner::{run, run_with_timeout};
pub use timeout::ProcessTimeoutPolicy;

#[cfg(test)]
mod execution_tests;
// The timeout suite exercises Unix process-group machinery directly
// (probe pids/pgids recorded by runner-owned children); on other targets
// the public boundary is the fail-closed stub above.
#[cfg(all(test, unix))]
mod timeout_tests;
