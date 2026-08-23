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
//! [`ExecutionError`] covers validation failures plus the distinct spawn
//! and wait boundaries.
//!
//! Deliberately excluded here (later runner slices): timeouts, terminate/
//! kill handling, output capture, digests, checkpoints, recovery. Also
//! excluded by architecture: sandbox enforcement of any kind. The
//! workspace boundary provides workspace isolation only — it is NOT a
//! security sandbox; filesystem, network, and process isolation belong to
//! the runtime/host sandbox layer.

mod error;
mod outcome;
mod request;
mod runner;

pub use error::ExecutionError;
pub use outcome::ProcessRunOutcome;
pub use request::ProcessRunRequest;
pub use runner::run;

#[cfg(test)]
mod execution_tests;
