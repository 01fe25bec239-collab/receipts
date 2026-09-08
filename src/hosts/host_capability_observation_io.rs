//! Fixed source paths and one read-only command; no settings writes or shell.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

pub(super) struct LocalSources;
impl Sources for LocalSources {
    fn plugins(&self, project_root: &Path) -> Result<Vec<u8>, Error> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let mut command = std::process::Command::new("claude");
            command
                .args(["plugin", "list", "--json"])
                .current_dir(project_root);
            process::capture(command, HOST_PROBE_TIMEOUT)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = project_root;
            Err(Error::Unsupported)
        }
    }

    fn configurations(
        &self,
        request: &HostCapabilityObservationRequest,
    ) -> Vec<(Source, Result<Vec<u8>, Error>)> {
        let mut results = Vec::new();
        let mut add = |source, path: Option<PathBuf>| {
            results.push((
                source,
                path.as_deref()
                    .ok_or(Error::Unavailable)
                    .and_then(read_document),
            ));
        };
        match request.host {
            HostId::Headless => {}
            HostId::Codex => {
                let root = config_root("CODEX_HOME", ".codex");
                add(
                    Source::CodexUserHooks,
                    root.as_ref().map(|p| p.join("hooks.json")),
                );
                add(Source::CodexUserConfig, root.map(|p| p.join("config.toml")));
                add(
                    Source::CodexProjectHooks,
                    Some(request.project_root.join(".codex/hooks.json")),
                );
                add(
                    Source::CodexProjectConfig,
                    Some(request.project_root.join(".codex/config.toml")),
                );
                add(Source::CodexLocalRequirements, codex_managed());
            }
            HostId::ClaudeCode => {
                let root = config_root("CLAUDE_CONFIG_DIR", ".claude");
                add(
                    Source::ClaudeUserSettings,
                    root.map(|p| p.join("settings.json")),
                );
                add(
                    Source::ClaudeProjectSettings,
                    Some(request.project_root.join(".claude/settings.json")),
                );
                add(
                    Source::ClaudeLocalSettings,
                    Some(request.project_root.join(".claude/settings.local.json")),
                );
                let managed = claude_managed();
                add(
                    Source::ClaudeManagedSettings,
                    managed.as_ref().map(|p| p.join("managed-settings.json")),
                );
                // Windows drop-in semantics have not been established in this slice.
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                match managed {
                    Some(root) => match drop_ins(&root.join("managed-settings.d")) {
                        Ok(paths) => results.extend(
                            paths
                                .into_iter()
                                .map(|p| (Source::ClaudeManagedDropIn, read_document(&p))),
                        ),
                        Err(error) => results.push((Source::ClaudeManagedDropIn, Err(error))),
                    },
                    None => results.push((Source::ClaudeManagedDropIn, Err(Error::Unavailable))),
                }
            }
        }
        results
    }
}

// Only exact path-location variables; never enumerate or retain the environment.
fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
fn config_root(override_name: &str, suffix: &str) -> Option<PathBuf> {
    // An invalid explicit override cannot silently fall back to a different source.
    if std::env::var_os(override_name).is_some() {
        return absolute_env(override_name);
    }
    #[cfg(windows)]
    let home = absolute_env("USERPROFILE");
    #[cfg(not(windows))]
    let home = absolute_env("HOME");
    home.map(|p| p.join(suffix))
}
fn claude_managed() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some("/Library/Application Support/ClaudeCode".into())
    } else if cfg!(target_os = "linux") {
        Some("/etc/claude-code".into())
    } else if cfg!(windows) {
        Some(r"C:\Program Files\ClaudeCode".into())
    } else {
        None
    }
}
fn codex_managed() -> Option<PathBuf> {
    if cfg!(unix) {
        Some("/etc/codex/requirements.toml".into())
    } else if cfg!(windows) {
        absolute_env("ProgramData").map(|p| p.join(r"OpenAI\Codex\requirements.toml"))
    } else {
        None
    }
}
fn io_error(error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => Error::Missing,
        std::io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        _ => Error::Unavailable,
    }
}

fn regular_path(path: &Path) -> Result<(), Error> {
    // Fixed paths only; do not follow symlink redirects in any component. This
    // also rejects named pipes/devices before open. Metadata is not a size bound.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::NotRegularFile);
        }
    }
    if !fs::symlink_metadata(path).map_err(io_error)?.is_file() {
        return Err(Error::NotRegularFile);
    }
    Ok(())
}

pub(super) fn read_document(path: &Path) -> Result<Vec<u8>, Error> {
    regular_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    // A final-component race must not turn the regular file open into a
    // blocking FIFO open or follow a replacement symlink.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "macos")]
        options.custom_flags(0x0004 | 0x0100); // O_NONBLOCK | O_NOFOLLOW
        #[cfg(target_os = "linux")]
        options.custom_flags(0x0800 | 0x20000); // O_NONBLOCK | O_NOFOLLOW
    }
    let file = options.open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(Error::NotRegularFile);
    }
    let mut bytes = Vec::new();
    file.take((HOST_STRUCTURED_OBSERVATION_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > HOST_STRUCTURED_OBSERVATION_MAX_BYTES {
        return Err(Error::StructuredObservationTooLarge);
    }
    Ok(bytes)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn drop_ins(path: &Path) -> Result<Vec<PathBuf>, Error> {
    // Reject redirects for directories too. Bound all entries examined, including
    // irrelevant names; do not scan a large directory searching for 32 matches.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::NotRegularFile);
        }
    }
    let mut paths = Vec::new();
    for (index, entry) in fs::read_dir(path).map_err(io_error)?.enumerate() {
        if index == HOST_MANAGED_DROP_IN_MAX_FILES {
            return Err(Error::TooManyFiles);
        }
        let entry = entry.map_err(io_error)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) mod process {
    use super::*;
    use std::{
        os::fd::AsRawFd,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    // POSIX poll operates only on our two pipe descriptors. No process-tree
    // semantics, runtime dependency, signal-group control, or background reader.
    #[repr(C)]
    struct PollFd {
        fd: std::ffi::c_int,
        events: std::ffi::c_short,
        revents: std::ffi::c_short,
    }
    #[cfg(target_os = "macos")]
    type Nfds = std::ffi::c_uint;
    #[cfg(target_os = "linux")]
    type Nfds = std::ffi::c_ulong;
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, count: Nfds, timeout: std::ffi::c_int) -> std::ffi::c_int;
    }

    fn drain(
        pipe: &mut (impl Read + AsRawFd),
        retained: &mut Vec<u8>,
        count: &mut usize,
        overflow: Error,
        eof: &mut bool,
        keep: bool,
    ) -> Result<(), Error> {
        if *eof {
            return Ok(());
        }
        let mut descriptor = PollFd {
            fd: pipe.as_raw_fd(),
            events: 1,
            revents: 0,
        };
        // SAFETY: live, initialized single pollfd; correct platform nfds_t;
        // timeout zero; the exclusively owned pipe has no competing reader.
        let ready = unsafe { poll(&mut descriptor, 1, 0) };
        if ready < 0 {
            return Err(Error::ProbeFailed);
        }
        if ready == 0 {
            return Ok(());
        }
        let mut buffer = [0_u8; 8192];
        let read = pipe.read(&mut buffer).map_err(|_| Error::ProbeFailed)?;
        if read == 0 {
            *eof = true;
            return Ok(());
        }
        *count = count.saturating_add(read);
        let limit = if keep {
            HOST_PROBE_STDOUT_MAX_BYTES
        } else {
            HOST_PROBE_STDERR_MAX_BYTES
        };
        if *count > limit {
            return Err(overflow);
        }
        if keep {
            retained.extend_from_slice(&buffer[..read]);
        }
        Ok(())
    }

    pub(in crate::host_capability_observation) fn capture(
        mut command: Command,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        let mut child = command.spawn().map_err(|_| Error::ProbeFailed)?;
        let mut stdout = child.stdout.take().expect("stdout was configured as piped");
        let mut stderr = child.stderr.take().expect("stderr was configured as piped");
        let mut bytes = Vec::new();
        let mut discarded = Vec::new();
        let (mut out_count, mut err_count) = (0, 0);
        let (mut out_eof, mut err_eof) = (false, false);
        let result = loop {
            if start.elapsed() >= timeout {
                break Err(Error::ProbeTimeout);
            }
            if let Err(error) = drain(
                &mut stdout,
                &mut bytes,
                &mut out_count,
                Error::ProbeStdoutTooLarge,
                &mut out_eof,
                true,
            ) {
                break Err(error);
            }
            if let Err(error) = drain(
                &mut stderr,
                &mut discarded,
                &mut err_count,
                Error::ProbeStderrTooLarge,
                &mut err_eof,
                false,
            ) {
                break Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => break Err(Error::ProbeFailed),
                Ok(Some(_)) if out_eof && err_eof => break Ok(bytes),
                Err(_) => break Err(Error::ProbeFailed),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if result.is_err() {
            // Close pipes before direct-child cleanup. A descendant holding a
            // descriptor cannot keep a reader thread or join alive: there is none.
            drop(stdout);
            drop(stderr);
            let _ = child.kill();
            let cleanup = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Err(_) => return Err(Error::ProbeCleanupFailed),
                    _ if cleanup.elapsed() >= Duration::from_secs(1) => {
                        return Err(Error::ProbeCleanupFailed);
                    }
                    _ => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
        result
    }
}
