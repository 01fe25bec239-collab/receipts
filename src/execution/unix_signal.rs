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
    use std::os::raw::{c_int, c_uint, c_void};

    // POSIX fixes these values for every supported Unix target; they are
    // not platform-variable.
    const SIGTERM: c_int = 15;
    const SIGKILL: c_int = 9;
    /// `ESRCH` — no such process: the target had already exited.
    const ESRCH: c_int = 3;
    /// `EPERM` — the target exists but the signal could not be delivered.
    const EPERM: c_int = 1;

    const P_PID: c_uint = 1;
    const WNOHANG: c_int = 1;
    const WEXITED: c_int = 4;
    #[cfg(target_vendor = "apple")]
    const WNOWAIT: c_int = 0x20;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const WNOWAIT: c_int = 0x0100_0000;

    unsafe extern "C" {
        fn kill(pid: c_int, sig: c_int) -> c_int;
        fn getpgrp() -> c_int;
        #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
        fn waitid(idtype: c_uint, id: c_uint, infop: *mut c_void, options: c_int) -> c_int;
    }

    #[cfg(target_vendor = "apple")]
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listpgrppids(pgrpid: c_int, buffer: *mut c_void, buffersize: c_int) -> c_int;
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
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

    /// Non-reaping direct-child state used while the group leader must
    /// remain an ownership anchor for any later group signal.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum LeaderState {
        Running,
        Exited,
    }

    /// Observes an exited child without consuming its wait status.
    ///
    /// `waitid(P_PID, ..., WEXITED | WNOHANG | WNOWAIT)` is the POSIX
    /// primitive specifically defined to report a waitable exit while
    /// leaving that exit waitable. The zeroed, deliberately oversized and
    /// over-aligned buffer is ABI storage only; production reads just the
    /// common leading `si_signo`, which remains zero when `WNOHANG` finds no
    /// event and is `SIGCHLD` for an exited child. Apple and Linux/Android
    /// use at most 128 bytes for `siginfo_t`; 512 bytes keeps the FFI write
    /// inside valid storage without duplicating a platform-private layout.
    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    pub(crate) fn observe_leader_without_reaping(pid: u32) -> std::io::Result<LeaderState> {
        #[repr(C, align(16))]
        struct SigInfoStorage {
            si_signo: c_int,
            rest: [u8; 508],
        }

        let mut info = SigInfoStorage {
            si_signo: 0,
            rest: [0; 508],
        };
        let ret = unsafe {
            waitid(
                P_PID,
                pid,
                (&mut info as *mut SigInfoStorage).cast(),
                WEXITED | WNOHANG | WNOWAIT,
            )
        };
        if ret == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(if info.si_signo == 0 {
            LeaderState::Running
        } else {
            LeaderState::Exited
        })
    }

    /// Other Unix targets fail closed rather than substitute a reaping
    /// observation or guess at target-specific `waitid` flag values.
    #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
    pub(crate) fn observe_leader_without_reaping(_pid: u32) -> std::io::Result<LeaderState> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "non-reaping waitid observation is unavailable on this Unix target",
        ))
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

    /// Whether any live member other than an already-exited group leader
    /// remains. This is used only while that unreaped leader still anchors
    /// ownership; [`group_presence`] remains the final post-reap proof.
    #[cfg(target_vendor = "apple")]
    pub(crate) fn live_group_members_excluding_leader(
        group: OwnedProcessGroup,
        leader: u32,
    ) -> GroupPresence {
        const PROC_PIDT_SHORTBSDINFO: c_int = 13;
        const SZOMB: u32 = 5;

        #[repr(C)]
        struct ProcBsdShortInfo {
            pid: u32,
            ppid: u32,
            pgid: u32,
            status: u32,
            comm: [u8; 16],
            flags: u32,
            uid: u32,
            gid: u32,
            ruid: u32,
            rgid: u32,
            svuid: u32,
            svgid: u32,
            reserved: u32,
        }

        let mut capacity = 32usize;
        for _ in 0..8 {
            let mut pids = vec![0 as c_int; capacity];
            let byte_capacity = match c_int::try_from(std::mem::size_of_val(pids.as_slice())) {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            let count =
                unsafe { proc_listpgrppids(group.raw(), pids.as_mut_ptr().cast(), byte_capacity) };
            if count < 0 {
                return GroupPresence::Unknown {
                    detail: format!(
                        "live-member listing for owned process group -{} failed: {}",
                        group.raw(),
                        std::io::Error::last_os_error()
                    ),
                };
            }
            let count = usize::try_from(count).unwrap_or(0);
            if count >= capacity {
                capacity = capacity.saturating_mul(2);
                continue;
            }
            for pid in pids.into_iter().take(count) {
                if pid <= 0 || u32::try_from(pid).ok() == Some(leader) {
                    continue;
                }
                let mut info = ProcBsdShortInfo {
                    pid: 0,
                    ppid: 0,
                    pgid: 0,
                    status: 0,
                    comm: [0; 16],
                    flags: 0,
                    uid: 0,
                    gid: 0,
                    ruid: 0,
                    rgid: 0,
                    svuid: 0,
                    svgid: 0,
                    reserved: 0,
                };
                let size = c_int::try_from(std::mem::size_of::<ProcBsdShortInfo>())
                    .expect("proc_bsdshortinfo fits c_int");
                let read = unsafe {
                    proc_pidinfo(
                        pid,
                        PROC_PIDT_SHORTBSDINFO,
                        0,
                        (&mut info as *mut ProcBsdShortInfo).cast(),
                        size,
                    )
                };
                if read == size && info.status != SZOMB {
                    return GroupPresence::HasMembers;
                }
                if read != size && unsafe { kill(pid, 0) } == 0 {
                    return GroupPresence::Unknown {
                        detail: format!(
                            "process {pid} remained in owned group {} but its live state could not be inspected",
                            group.raw()
                        ),
                    };
                }
            }
            return GroupPresence::Empty;
        }
        GroupPresence::Unknown {
            detail: format!(
                "live-member listing for owned process group -{} did not fit a bounded snapshot",
                group.raw()
            ),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn live_group_members_excluding_leader(
        group: OwnedProcessGroup,
        leader: u32,
    ) -> GroupPresence {
        let entries = match std::fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(error) => {
                return GroupPresence::Unknown {
                    detail: format!("cannot inspect /proc for owned-group membership: {error}"),
                };
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return GroupPresence::Unknown {
                    detail: "cannot enumerate /proc for owned-group membership".to_string(),
                };
            };
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == leader {
                continue;
            }
            let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                Ok(stat) => stat,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return GroupPresence::Unknown {
                        detail: format!("cannot inspect /proc/{pid}/stat: {error}"),
                    };
                }
            };
            let Some((_, fields)) = stat.rsplit_once(") ") else {
                return GroupPresence::Unknown {
                    detail: format!("cannot parse /proc/{pid}/stat"),
                };
            };
            let mut fields = fields.split_whitespace();
            let state = fields.next();
            let _ppid = fields.next();
            let pgrp = fields.next().and_then(|value| value.parse::<c_int>().ok());
            if pgrp == Some(group.raw()) && state != Some("Z") {
                return GroupPresence::HasMembers;
            }
        }
        GroupPresence::Empty
    }

    #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
    pub(crate) fn live_group_members_excluding_leader(
        group: OwnedProcessGroup,
        _leader: u32,
    ) -> GroupPresence {
        GroupPresence::Unknown {
            detail: format!(
                "live-member inspection is unavailable for owned process group -{} on this Unix target",
                group.raw()
            ),
        }
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
    GroupPresence, GroupSignalDelivery, LeaderState, OwnedProcessGroup, deliver_group_sigkill,
    deliver_group_sigterm, group_presence, live_group_members_excluding_leader,
    observe_leader_without_reaping,
};
// Classifier tables and zero-signal probes are exercised directly by the
// timeout suite; production flow reaches them through the helpers above.
#[cfg(all(unix, test))]
pub(crate) use unix::{
    caller_process_group, classify_group_kill_result, classify_group_presence, process_alive,
    recorded_group_is_empty,
};
