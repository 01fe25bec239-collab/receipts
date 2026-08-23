//! Narrow Unix graceful-termination helper (SIGTERM delivery only).
//!
//! This is the smallest OS mechanism that satisfies the frozen
//! terminate-then-bounded-kill contract: Rust's [`std::process::Child::kill`]
//! is a force-kill semantic on Unix, so a genuine graceful termination
//! step requires delivering `SIGTERM` directly. No shell exists here and
//! no dependency is added — one platform-gated `kill(2)` declaration with
//! the two POSIX-fixed signal numbers this slice needs.
//!
//! The helper signals exactly one checked pid owned by the runner. It can
//! never target pid 0 (process group of the caller), a negative value
//! (some other process group), or any out-of-range identifier: the `u32`
//! reported by the spawned child is range-checked into `pid_t` before any
//! system call is made.

#[cfg(unix)]
pub(crate) mod unix {
    use std::os::raw::c_int;

    // POSIX fixes these values for every supported Unix target; they are
    // not platform-variable.
    const SIGTERM: c_int = 15;
    /// `ESRCH` — no such process: the target had already exited.
    const ESRCH: c_int = 3;

    unsafe extern "C" {
        fn kill(pid: c_int, sig: c_int) -> c_int;
    }

    /// The outcome of one graceful-termination delivery attempt.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum SignalDelivery {
        /// The signal was delivered to the live child.
        Delivered,
        /// The kernel reports no such process: the child exited between
        /// the last observation and the delivery attempt.
        AlreadyExited,
        /// Delivery failed for any other reason.
        Failed { detail: String },
    }

    /// Pure classification of one raw `kill(2)` result against the target
    /// pid. Extracted so the errno table is unit-testable without ever
    /// signaling an unrelated process.
    pub(crate) fn classify_kill_result(
        ret: c_int,
        errno: Option<c_int>,
        target_pid: u32,
    ) -> SignalDelivery {
        if ret == 0 {
            return SignalDelivery::Delivered;
        }
        if errno == Some(ESRCH) {
            return SignalDelivery::AlreadyExited;
        }
        SignalDelivery::Failed {
            detail: format!(
                "SIGTERM delivery to runner-owned child pid {target_pid} failed (kill returned \
                 {ret}, errno {errno:?})"
            ),
        }
    }

    /// Delivers `SIGTERM` to exactly one runner-owned child pid.
    pub(crate) fn deliver_sigterm(child_pid: u32) -> SignalDelivery {
        let Ok(pid) = c_int::try_from(child_pid) else {
            return SignalDelivery::Failed {
                detail: format!(
                    "child pid {child_pid} does not fit the platform pid_t range; refusing to \
                     convert lossily"
                ),
            };
        };
        let ret = unsafe { kill(pid, SIGTERM) };
        let errno = if ret == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        classify_kill_result(ret, errno, child_pid)
    }

    /// Test-only liveness probe backed by `kill(pid, 0)`, which performs
    /// no signaling at all. Used exclusively by tests over pids that a
    /// runner-owned probe child itself recorded, never over arbitrary
    /// process identifiers. A reaped child answers `ESRCH`; a live or
    /// still-unreaped-zombie child answers success.
    #[cfg(test)]
    pub(crate) fn process_alive(pid: u32) -> bool {
        let Ok(pid) = c_int::try_from(pid) else {
            return false;
        };
        unsafe { kill(pid, 0) == 0 }
    }
}

#[cfg(unix)]
pub(crate) use unix::{SignalDelivery, deliver_sigterm};
