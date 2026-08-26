//! Narrow Unix attempt-owned process-group control (SIGTERM/SIGKILL only).
//!
//! This is the smallest OS mechanism that satisfies the frozen
//! terminate-then-bounded-kill contract including its no-orphan invariant:
//! a timed attempt owns a dedicated process group, and termination targets
//! that whole group so child-spawned descendants cannot survive the
//! timeout boundary. Rust's [`std::process::Child::kill`] reaches only one
//! pid, so genuine graceful and forced group termination requires
//! delivering signals with `kill(2)` directly. No shell exists here and no
//! dependency is added — platform-gated `kill(2)`/`getpgrp(2)` declarations
//! with the POSIX-fixed signal numbers and errnos this slice needs.
//!
//! Safety invariants enforced here, all fail-closed:
//!
//! * an owned group is accepted only as a positive `pid_t`-fitting value
//!   derived from this invocation's spawned child; zero (the caller's own
//!   group under `kill(0, ...)`), one (`kill(-1, ...)` broadcast), values
//!   outside `pid_t`, and the caller's current process group are refused;
//! * signaling always negates the validated value, never caller-supplied
//!   integers;
//! * presence probing distinguishes `ESRCH` (no member remains) from
//!   `EPERM` (members exist but are not signalable) from any other error,
//!   and unknown states are reported as unknown instead of being silently
//!   treated as success or absence;
//! * nothing here is publicly reachable: every helper is crate-private and
//!   operates only on groups this runner created.

#[cfg(unix)]
pub(crate) mod unix {
    use std::os::raw::c_int;

    // POSIX fixes these values for every supported Unix target; they are
    // not platform-variable.
    const SIGTERM: c_int = 15;
    const SIGKILL: c_int = 9;
    /// `ESRCH` — no such process: the target had already exited.
    const ESRCH: c_int = 3;
    /// `EPERM` — the target exists but the signal could not be delivered.
    const EPERM: c_int = 1;

    unsafe extern "C" {
        fn kill(pid: c_int, sig: c_int) -> c_int;
        fn getpgrp() -> c_int;
    }

    /// The caller's (runner's) own process group id, used exclusively to
    /// prove that an attempt-owned group is distinct before it may ever be
    /// signaled. `getpgrp` cannot fail.
    pub(crate) fn caller_process_group() -> c_int {
        unsafe { getpgrp() }
    }

    /// A process-group identifier proven safe to signal as this
    /// invocation's own: constructed only through
    /// [`OwnedProcessGroup::from_child_pid`], which refuses everything the
    /// frozen safety contract forbids targeting.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct OwnedProcessGroup(c_int);

    impl OwnedProcessGroup {
        /// Validates the process group of a freshly spawned child (the
        /// child was spawned with `process_group(0)`, so it leads its own
        /// group: pgid == child pid).
        ///
        /// Fails closed — returns [`None`] — for any raw value the frozen
        /// contract forbids signaling:
        ///
        /// * `0`: `kill(0, sig)` / `kill(-0, sig)` would reach the
        ///   caller's whole process group;
        /// * `1`: `kill(-1, sig)` is the kernel-wide broadcast form;
        /// * values outside the platform `pid_t` range (lossy conversion
        ///   refused);
        /// * the caller's current process group, which must never be a
        ///   signaling target even if ownership setup somehow degenerated.
        pub(crate) fn from_child_pid(raw: u32) -> Option<Self> {
            if raw == 0 || raw == 1 {
                return None;
            }
            let pgid = c_int::try_from(raw).ok()?;
            if pgid <= 0 {
                return None;
            }
            if pgid == caller_process_group() {
                return None;
            }
            Some(Self(pgid))
        }

        /// The validated platform value, already known to be signable.
        pub(crate) fn raw(self) -> c_int {
            self.0
        }
    }

    /// The outcome of one owned-group signal delivery attempt.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum GroupSignalDelivery {
        /// The signal reached at least one live member of the owned group.
        Delivered,
        /// The kernel reports no such process group: every member had
        /// already exited between the last observation and delivery.
        GroupAlreadyGone,
        /// Delivery failed for any other reason; evidence preserved.
        Failed { detail: String },
    }

    /// Pure classification of one raw `kill(2)` result against an owned
    /// group. Extracted so the errno table is unit-testable without ever
    /// signaling an unrelated process.
    pub(crate) fn classify_group_kill_result(
        ret: c_int,
        errno: Option<c_int>,
        pgid: u32,
        label: &'static str,
    ) -> GroupSignalDelivery {
        if ret == 0 {
            return GroupSignalDelivery::Delivered;
        }
        if errno == Some(ESRCH) {
            return GroupSignalDelivery::GroupAlreadyGone;
        }
        GroupSignalDelivery::Failed {
            detail: format!(
                "{label} delivery to the attempt-owned process group -{pgid} failed (kill \
                 returned {ret}, errno {errno:?})"
            ),
        }
    }

    /// Delivers one signal to exactly this invocation's owned process
    /// group. The negative-pid broadcast-to-group form is used only with
    /// the pre-validated identifier; arbitrary values never reach here.
    pub(crate) fn signal_owned_group(
        group: OwnedProcessGroup,
        sig: c_int,
        label: &'static str,
    ) -> GroupSignalDelivery {
        let pgid = group.raw();
        let ret = unsafe { kill(-pgid, sig) };
        let errno = if ret == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        let Ok(unsigned_pgid) = u32::try_from(pgid) else {
            return GroupSignalDelivery::Failed {
                detail: format!(
                    "{label} delivery refused: owned process group {pgid} does not fit an \
                     unsigned pid range; refusing to convert lossily"
                ),
            };
        };
        classify_group_kill_result(ret, errno, unsigned_pgid, label)
    }

    /// `SIGTERM` to the owned process group: the graceful-termination step.
    pub(crate) fn deliver_group_sigterm(group: OwnedProcessGroup) -> GroupSignalDelivery {
        signal_owned_group(group, SIGTERM, "graceful SIGTERM")
    }

    /// `SIGKILL` to the owned process group: the forced-termination step.
    pub(crate) fn deliver_group_sigkill(group: OwnedProcessGroup) -> GroupSignalDelivery {
        signal_owned_group(group, SIGKILL, "forced SIGKILL")
    }

    /// Whether the owned process group still has any member (including
    /// unreaped zombies). Presence probing uses the zero-signal form,
    /// which performs no signaling at all.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum GroupPresence {
        /// At least one process (or zombie) still belongs to the group.
        HasMembers,
        /// The kernel reports the group empty: `ESRCH`.
        Empty,
        /// Membership could not be determined. Callers must treat this as
        /// potentially occupied — cleanup proceeds, never assumes success.
        Unknown { detail: String },
    }

    /// Pure classification of one raw zero-signal probe result against an
    /// owned group, distinguishing `ESRCH` (empty) from `EPERM`
    /// (members exist but are not signalable by us — still occupied) from
    /// any other failure (unknown, conservatively occupied).
    pub(crate) fn classify_group_presence(
        ret: c_int,
        errno: Option<c_int>,
        pgid: u32,
    ) -> GroupPresence {
        if ret == 0 {
            return GroupPresence::HasMembers;
        }
        if errno == Some(ESRCH) {
            return GroupPresence::Empty;
        }
        if errno == Some(EPERM) {
            return GroupPresence::HasMembers;
        }
        GroupPresence::Unknown {
            detail: format!(
                "presence probe of the attempt-owned process group -{pgid} failed (kill \
                 returned {ret}, errno {errno:?})"
            ),
        }
    }

    /// Observes whether the owned group still contains members.
    pub(crate) fn group_presence(group: OwnedProcessGroup) -> GroupPresence {
        let pgid = group.raw();
        let ret = unsafe { kill(-pgid, 0) };
        let errno = if ret == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        let Ok(unsigned_pgid) = u32::try_from(pgid) else {
            return GroupPresence::Unknown {
                detail: format!(
                    "presence probe refused: owned process group {pgid} does not fit an unsigned \
                     pid range"
                ),
            };
        };
        classify_group_presence(ret, errno, unsigned_pgid)
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

    /// Test-only emptiness probe over a group id that an attempt-owned
    /// probe child itself recorded before termination. Zero-signal form:
    /// performs no signaling. [`None`] means the identifier was not a
    /// plausible positive group id; [`Some(true)`] means the kernel
    /// reports the group empty.
    #[cfg(test)]
    pub(crate) fn recorded_group_is_empty(pgid: u32) -> Option<bool> {
        let signed = c_int::try_from(pgid).ok()?;
        if signed <= 0 {
            return None;
        }
        let ret = unsafe { kill(-signed, 0) };
        let errno = if ret == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        let Ok(unsigned) = u32::try_from(signed) else {
            return None;
        };
        Some(classify_group_presence(ret, errno, unsigned) == GroupPresence::Empty)
    }
}

#[cfg(unix)]
pub(crate) use unix::{
    GroupPresence, GroupSignalDelivery, OwnedProcessGroup, deliver_group_sigkill,
    deliver_group_sigterm, group_presence,
};
// Classifier tables and zero-signal probes are exercised directly by the
// timeout suite; production flow reaches them through the helpers above.
#[cfg(all(unix, test))]
pub(crate) use unix::{
    caller_process_group, classify_group_kill_result, classify_group_presence, process_alive,
    recorded_group_is_empty,
};
