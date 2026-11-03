//! Linux enforcement for [`crate::protected`]. x86_64 only (the qualified
//! architecture); other Linux architectures refuse at admission.
//!
//! Child setup runs between fork and exec, so the hooks below perform only
//! raw system calls on data prepared before fork (no allocation, no locks).
//! NOT verified by local Darwin tests; see the hosted Linux diagnostic.

#[cfg(target_arch = "x86_64")]
mod imp {
    use std::ffi::{CString, c_int, c_long, c_void};
    use std::io;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use crate::execution::{AttemptContainment, LaunchCustomization};
    use crate::git::ControllerLaunch;
    use crate::protected::{
        Binding, EstablishError, Gate, LANDLOCK_ACCESS_FS_ABI3, ProtectedLaunchConfig, SockFilter,
        controller_landlock_plan, seccomp_program_x86_64, verify_ancestors, verify_custody,
    };
    use crate::protected_custody as custody;

    unsafe extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
    }

    const SYS_WRITE: c_long = 1;
    const SYS_GETEUID: c_long = 107;
    const SYS_SETGROUPS: c_long = 116;
    const SYS_SETRESUID: c_long = 117;
    const SYS_SETRESGID: c_long = 119;
    const SYS_CAPSET: c_long = 126;
    const SYS_STATFS: c_long = 137;
    const SYS_PRCTL: c_long = 157;
    const SYS_MOUNT: c_long = 165;
    const SYS_UNSHARE: c_long = 272;
    const SYS_CLOSE_RANGE: c_long = 436;
    const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
    const SYS_LANDLOCK_ADD_RULE: c_long = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;

    const PR_SET_KEEPCAPS: c_long = 8;
    const PR_GET_SECCOMP: c_long = 21;
    const PR_SET_SECCOMP: c_long = 22;
    const PR_CAPBSET_DROP: c_long = 24;
    const PR_SET_NO_NEW_PRIVS: c_long = 38;
    const PR_CAP_AMBIENT: c_long = 47;
    const PR_CAP_AMBIENT_RAISE: c_long = 2;
    const PR_CAP_AMBIENT_CLEAR_ALL: c_long = 4;
    const SECCOMP_MODE_FILTER: c_long = 2;
    const CAP_DAC_READ_SEARCH: c_long = 2;
    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    const CLONE_NEWNS: c_long = 0x0002_0000;
    const MS_RDONLY: c_long = 1;
    const MS_REMOUNT: c_long = 32;
    const MS_BIND: c_long = 4096;
    const MS_REC: c_long = 16384;
    const MS_PRIVATE: c_long = 1 << 18;
    const CLOSE_RANGE_CLOEXEC: c_long = 4;
    const LANDLOCK_CREATE_RULESET_VERSION: c_long = 1;
    const LANDLOCK_RULE_PATH_BENEATH: c_long = 1;
    const CGROUP2_SUPER_MAGIC: u64 = 0x6367_7270;
    const O_PATH: i32 = 0o10_000_000;
    const CLEANUP_WINDOW: Duration = Duration::from_secs(5);
    /// Every variadic syscall argument is a full-width `long`.
    const Z: c_long = 0;

    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    #[repr(C)]
    struct CapHeader {
        version: u32,
        pid: c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CapData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }

    #[repr(C, packed)]
    struct PathBeneath {
        allowed_access: u64,
        parent_fd: i32,
    }

    #[derive(Debug)]
    pub(crate) struct LinuxSupport {
        pub(crate) cap_last_cap: u32,
        cgroup_root: PathBuf,
    }

    fn cstring(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "path contains NUL".to_string())
    }

    fn read_trimmed(path: &str) -> Result<String, String> {
        std::fs::read_to_string(path)
            .map(|text| text.trim().to_owned())
            .map_err(|error| format!("{path}: {error}"))
    }

    /// Admission probes; any failure refuses protected operation.
    pub(crate) fn probe(cgroup_root: &Path) -> Result<LinuxSupport, String> {
        if unsafe { syscall(SYS_GETEUID) } != 0 {
            return Err("launcher must run with effective UID 0 (administrator setup)".into());
        }
        let root = cstring(cgroup_root)?;
        let mut statfs = [0u64; 16];
        if unsafe { syscall(SYS_STATFS, root.as_ptr(), statfs.as_mut_ptr()) } != 0 {
            return Err(format!(
                "statfs cgroup root: {}",
                io::Error::last_os_error()
            ));
        }
        if statfs[0] != CGROUP2_SUPER_MAGIC {
            return Err("cgroup root is not a cgroup v2 filesystem".into());
        }
        for file in ["cgroup.procs", "cgroup.kill", "cgroup.events"] {
            if !cgroup_root.join(file).is_file() {
                return Err(format!(
                    "delegated cgroup lacks {file} (kernel >= 5.14 required)"
                ));
            }
        }
        let abi = unsafe {
            syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<c_void>(),
                Z,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        if abi < 3 {
            return Err(format!("Landlock ABI >= 3 required (found {abi})"));
        }
        if unsafe { syscall(SYS_PRCTL, PR_GET_SECCOMP, Z, Z, Z, Z) } < 0 {
            return Err("seccomp is unavailable".into());
        }
        for (path, expected) in [
            ("/proc/sys/fs/protected_hardlinks", "1"),
            ("/proc/sys/fs/protected_symlinks", "1"),
        ] {
            if read_trimmed(path)? != expected {
                return Err(format!("{path} must be {expected}"));
            }
        }
        let cap_last_cap: u32 = read_trimmed("/proc/sys/kernel/cap_last_cap")?
            .parse()
            .map_err(|_| "unparseable cap_last_cap".to_string())?;
        if cap_last_cap > 63 {
            return Err("unknown capability range".into());
        }
        Ok(LinuxSupport {
            cap_last_cap,
            cgroup_root: cgroup_root.to_path_buf(),
        })
    }

    /// Whether any live process has `uid` as real/effective/saved/fs UID.
    pub(crate) fn uid_in_use(uid: u32) -> Result<bool, String> {
        for entry in std::fs::read_dir("/proc").map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            if !name.as_bytes().iter().all(u8::is_ascii_digit) {
                continue;
            }
            let status = match std::fs::read_to_string(entry.path().join("status")) {
                Ok(status) => status,
                // Vanished between listing and reading.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) if error.raw_os_error() == Some(3) => continue,
                Err(error) => return Err(error.to_string()),
            };
            let line = status
                .lines()
                .find(|line| line.starts_with("Uid:"))
                .ok_or("malformed /proc status")?;
            if line
                .split_whitespace()
                .skip(1)
                .any(|field| field.parse::<u32>().ok() == Some(uid))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Transfers metadata to the controller, payload to the attempt's worker
    /// and the worktree root to controller:worker 01770, then verifies.
    ///
    /// Everything that can refuse runs first and read-only on stable
    /// descriptors (recorded root identities, ancestors, mount topology, the
    /// `.git` pointer, link/special-file policy of both trees). Only then does
    /// the reservation record move to `custody` and ownership change, every
    /// time through the same verified object.
    pub(crate) fn establish_custody(
        config: &ProtectedLaunchConfig,
        binding: &Binding,
    ) -> Result<(), EstablishError> {
        let (cu, cg, wg) = (
            config.controller_uid,
            config.controller_gid,
            config.worker_gid,
        );
        let refused = EstablishError::Refused;
        binding.reservation.revalidate().map_err(refused)?;
        let ids = binding
            .reservation
            .ids()
            .ok_or_else(|| refused("custody record has no root identities".into()))?;
        let roots = custody::Roots::open(&binding.common_dir, &binding.worktree, Some(ids))
            .map_err(refused)?;
        verify_ancestors(&binding.common_dir, &binding.worktree, cu).map_err(refused)?;
        custody::preflight_roots(&roots, &binding.common_dir, &binding.worktree)
            .map_err(refused)?;
        binding
            .reservation
            .set_phase(custody::Phase::Custody)
            .map_err(|error| refused(format!("custody record: {error}")))?;
        let own = custody::Ownership {
            controller_uid: cu,
            controller_gid: cg,
            worker_uid: binding.worker_uid,
            worker_gid: wg,
        };
        custody::acquire(&roots, &own).map_err(EstablishError::Incomplete)?;
        verify_custody(&binding.common_dir, &binding.worktree, cu, wg)
            .map_err(EstablishError::Incomplete)
    }

    /// Returns metadata and payload to the pre-admission owner. Called only
    /// after the attempt's cgroup was proven empty and the shared release
    /// barrier drained every controller operation; also requires that no live
    /// process still holds the worker identity. Validates the persisted
    /// original-owner evidence and the recorded root identities, and checks the
    /// same link/mount boundaries as acquisition BEFORE the first ownership
    /// change; every change then goes through a verified object.
    pub(crate) fn release_custody(binding: &Binding) -> Result<(), String> {
        if uid_in_use(binding.worker_uid)? {
            return Err("a process still runs as the worker identity".into());
        }
        let original = binding.original;
        binding.reservation.revalidate()?;
        let ids = binding
            .reservation
            .ids()
            .ok_or("custody record has no root identities")?;
        let roots = custody::Roots::open(&binding.common_dir, &binding.worktree, Some(ids))?;
        custody::preflight_roots(&roots, &binding.common_dir, &binding.worktree)?;
        custody::restore(&roots, original.uid, original.gid, original.worktree_mode)
    }

    /// One owned leaf cgroup; containment kills it, proves it unpopulated
    /// and removes it. Unknown outcomes keep it in place.
    #[derive(Debug)]
    struct Cgroup {
        dir: PathBuf,
    }

    fn create_cgroup(root: &Path, name: &str) -> Result<(Cgroup, OwnedFd), String> {
        let dir = root.join(name);
        std::fs::create_dir(&dir).map_err(|error| format!("create cgroup {name}: {error}"))?;
        let procs = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join("cgroup.procs"))
            .map_err(|error| format!("open cgroup.procs: {error}"))?;
        Ok((Cgroup { dir }, procs.into()))
    }

    impl AttemptContainment for Cgroup {
        fn kill_and_verify_empty(&self) -> Result<(), String> {
            std::fs::write(self.dir.join("cgroup.kill"), "1")
                .map_err(|error| format!("cgroup.kill: {error}"))?;
            let started = Instant::now();
            loop {
                let events = std::fs::read_to_string(self.dir.join("cgroup.events"))
                    .map_err(|error| format!("cgroup.events: {error}"))?;
                if events.lines().any(|line| line == "populated 0")
                    && std::fs::remove_dir(&self.dir).is_ok()
                {
                    return Ok(());
                }
                if started.elapsed() >= CLEANUP_WINDOW {
                    return Err(format!(
                        "{} not proven empty within {CLEANUP_WINDOW:?}; retained",
                        self.dir.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    enum Identity {
        Worker { gitdir: CString },
        Controller { landlock: Vec<(OwnedFd, u64)> },
    }

    struct Hook {
        procs: OwnedFd,
        uid: u32,
        gid: u32,
        cap_last_cap: u32,
        filter: Vec<SockFilter>,
        identity: Identity,
    }

    fn check(result: c_long) -> io::Result<()> {
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl Hook {
        /// Runs in the forked child. Raw system calls only.
        fn apply(&self) -> io::Result<()> {
            unsafe {
                check(syscall(
                    SYS_WRITE,
                    self.procs.as_raw_fd() as c_long,
                    c"0".as_ptr(),
                    1 as c_long,
                ))?;
                let keep = match &self.identity {
                    Identity::Worker { gitdir } => {
                        check(syscall(SYS_UNSHARE, CLONE_NEWNS))?;
                        check(syscall(
                            SYS_MOUNT,
                            std::ptr::null::<u8>(),
                            c"/".as_ptr(),
                            std::ptr::null::<u8>(),
                            MS_REC | MS_PRIVATE,
                            std::ptr::null::<u8>(),
                        ))?;
                        check(syscall(
                            SYS_MOUNT,
                            gitdir.as_ptr(),
                            gitdir.as_ptr(),
                            std::ptr::null::<u8>(),
                            MS_BIND | MS_REC,
                            std::ptr::null::<u8>(),
                        ))?;
                        check(syscall(
                            SYS_MOUNT,
                            std::ptr::null::<u8>(),
                            gitdir.as_ptr(),
                            std::ptr::null::<u8>(),
                            MS_REMOUNT | MS_BIND | MS_RDONLY,
                            std::ptr::null::<u8>(),
                        ))?;
                        None
                    }
                    Identity::Controller { .. } => Some(CAP_DAC_READ_SEARCH),
                };
                for cap in 0..=c_long::from(self.cap_last_cap) {
                    if Some(cap) != keep {
                        check(syscall(SYS_PRCTL, PR_CAPBSET_DROP, cap, Z, Z, Z))?;
                    }
                }
                if keep.is_some() {
                    check(syscall(SYS_PRCTL, PR_SET_KEEPCAPS, 1 as c_long, Z, Z, Z))?;
                }
                check(syscall(SYS_SETGROUPS, Z, std::ptr::null::<u32>()))?;
                check(syscall(
                    SYS_SETRESGID,
                    self.gid as c_long,
                    self.gid as c_long,
                    self.gid as c_long,
                ))?;
                check(syscall(
                    SYS_SETRESUID,
                    self.uid as c_long,
                    self.uid as c_long,
                    self.uid as c_long,
                ))?;
                // Explicit final sets for BOTH identities: permitted, effective
                // and inheritable are exactly the retained capability (none for
                // the worker), the ambient set is cleared and then holds only
                // that capability, and keep-caps is reset.
                let header = CapHeader {
                    version: LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let bit = keep.map_or(0u32, |cap| 1u32 << cap);
                let data = [
                    CapData {
                        effective: bit,
                        permitted: bit,
                        inheritable: bit,
                    },
                    CapData {
                        effective: 0,
                        permitted: 0,
                        inheritable: 0,
                    },
                ];
                check(syscall(
                    SYS_CAPSET,
                    &header as *const CapHeader,
                    data.as_ptr(),
                ))?;
                check(syscall(
                    SYS_PRCTL,
                    PR_CAP_AMBIENT,
                    PR_CAP_AMBIENT_CLEAR_ALL,
                    Z,
                    Z,
                    Z,
                ))?;
                if let Some(cap) = keep {
                    check(syscall(
                        SYS_PRCTL,
                        PR_CAP_AMBIENT,
                        PR_CAP_AMBIENT_RAISE,
                        cap,
                        Z,
                        Z,
                    ))?;
                }
                check(syscall(SYS_PRCTL, PR_SET_KEEPCAPS, Z, Z, Z, Z))?;
                check(syscall(
                    SYS_PRCTL,
                    PR_SET_NO_NEW_PRIVS,
                    1 as c_long,
                    Z,
                    Z,
                    Z,
                ))?;
                if let Identity::Controller { landlock } = &self.identity {
                    let handled: u64 = LANDLOCK_ACCESS_FS_ABI3;
                    let ruleset = syscall(
                        SYS_LANDLOCK_CREATE_RULESET,
                        &handled as *const u64,
                        std::mem::size_of::<u64>() as c_long,
                        Z,
                    );
                    check(ruleset)?;
                    for (fd, access) in landlock {
                        let rule = PathBeneath {
                            allowed_access: *access,
                            parent_fd: fd.as_raw_fd(),
                        };
                        check(syscall(
                            SYS_LANDLOCK_ADD_RULE,
                            ruleset,
                            LANDLOCK_RULE_PATH_BENEATH,
                            &rule as *const PathBeneath,
                            Z,
                        ))?;
                    }
                    check(syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, Z))?;
                }
                check(syscall(
                    SYS_CLOSE_RANGE,
                    3 as c_long,
                    c_long::from(u32::MAX),
                    CLOSE_RANGE_CLOEXEC,
                ))?;
                let program = SockFprog {
                    len: self.filter.len() as u16,
                    filter: self.filter.as_ptr(),
                };
                check(syscall(
                    SYS_PRCTL,
                    PR_SET_SECCOMP,
                    SECCOMP_MODE_FILTER,
                    &program as *const SockFprog,
                    Z,
                    Z,
                ))?;
            }
            Ok(())
        }
    }

    fn customization(hook: Hook, cgroup: Cgroup) -> LaunchCustomization {
        LaunchCustomization {
            pre_exec: Some(Box::new(move || hook.apply())),
            containment: Some(Arc::new(cgroup)),
        }
    }

    pub(crate) fn worker_launch(
        support: &LinuxSupport,
        config: &ProtectedLaunchConfig,
        binding: &Binding,
    ) -> Result<LaunchCustomization, String> {
        let (cgroup, procs) = create_cgroup(
            &support.cgroup_root,
            &format!("receipts-{}-worker", binding.attempt_id),
        )?;
        let hook = Hook {
            procs,
            uid: binding.worker_uid,
            gid: config.worker_gid,
            cap_last_cap: support.cap_last_cap,
            filter: seccomp_program_x86_64(),
            identity: Identity::Worker {
                gitdir: cstring(&binding.common_dir)?,
            },
        };
        Ok(customization(hook, cgroup))
    }

    struct Controller {
        cgroup_root: PathBuf,
        attempt_id: String,
        uid: u32,
        gid: u32,
        cap_last_cap: u32,
        plan: Vec<(PathBuf, u64)>,
        counter: AtomicU64,
        gate: Arc<Gate>,
    }

    impl ControllerLaunch for Controller {
        fn cleanup_unproven(&self) {
            self.gate.mark_controller_unproven();
        }

        fn customization(&self) -> Result<LaunchCustomization, String> {
            let mut landlock = Vec::new();
            for (path, access) in &self.plan {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(O_PATH)
                    .open(path)
                {
                    Ok(file) => landlock.push((OwnedFd::from(file), *access)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(format!("{}: {error}", path.display())),
                }
            }
            let index = self.counter.fetch_add(1, Ordering::Relaxed);
            let (cgroup, procs) = create_cgroup(
                &self.cgroup_root,
                &format!("receipts-{}-ctl-{index}", self.attempt_id),
            )?;
            let hook = Hook {
                procs,
                uid: self.uid,
                gid: self.gid,
                cap_last_cap: self.cap_last_cap,
                filter: seccomp_program_x86_64(),
                identity: Identity::Controller { landlock },
            };
            Ok(customization(hook, cgroup))
        }
    }

    pub(crate) fn controller_launch(
        support: &LinuxSupport,
        config: &ProtectedLaunchConfig,
        binding: &Binding,
    ) -> Result<Arc<dyn ControllerLaunch>, String> {
        Ok(Arc::new(Controller {
            cgroup_root: support.cgroup_root.clone(),
            attempt_id: binding.attempt_id.clone(),
            uid: config.controller_uid,
            gid: config.controller_gid,
            cap_last_cap: support.cap_last_cap,
            plan: controller_landlock_plan(binding),
            counter: AtomicU64::new(0),
            gate: binding.gate.clone(),
        }))
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod imp {
    use std::path::Path;
    use std::sync::Arc;

    use crate::execution::LaunchCustomization;
    use crate::git::ControllerLaunch;
    use crate::protected::{Binding, EstablishError, ProtectedLaunchConfig};

    const REFUSAL: &str = "protected launch is qualified only for Linux x86_64";

    #[derive(Debug)]
    pub(crate) struct LinuxSupport;

    pub(crate) fn probe(_: &Path) -> Result<LinuxSupport, String> {
        Err(REFUSAL.into())
    }
    pub(crate) fn uid_in_use(_: u32) -> Result<bool, String> {
        Err(REFUSAL.into())
    }
    pub(crate) fn establish_custody(
        _: &ProtectedLaunchConfig,
        _: &Binding,
    ) -> Result<(), EstablishError> {
        Err(EstablishError::Refused(REFUSAL.into()))
    }
    pub(crate) fn worker_launch(
        _: &LinuxSupport,
        _: &ProtectedLaunchConfig,
        _: &Binding,
    ) -> Result<LaunchCustomization, String> {
        Err(REFUSAL.into())
    }
    pub(crate) fn controller_launch(
        _: &LinuxSupport,
        _: &ProtectedLaunchConfig,
        _: &Binding,
    ) -> Result<Arc<dyn ControllerLaunch>, String> {
        Err(REFUSAL.into())
    }
    pub(crate) fn release_custody(_: &Binding) -> Result<(), String> {
        Err(REFUSAL.into())
    }
}

pub(crate) use imp::*;
