//! Fixed source paths and port-run read-only probes; no settings writes or shell.
use super::*;
use crate::host_probe_port::{HostProbeCommandV1, HostProbeProcessPort};
use std::{
    fs::{self},
    io::Read,
    path::{Path, PathBuf},
};

/// Reads fixed source paths itself; runs probes only through the admitted
/// composition's port (no PATH lookup, no direct process spawn here).
pub(super) struct LocalSources<'a>(pub(super) &'a dyn HostProbeProcessPort);

/// The bounded read-only probe Host asks the composition to run.
pub(super) fn probe_command(project_root: &Path, args: &[&str]) -> HostProbeCommandV1 {
    HostProbeCommandV1 {
        args: args.iter().map(|a| String::from(*a)).collect(),
        current_dir: project_root.to_path_buf(),
        timeout: HOST_PROBE_TIMEOUT,
        stdout_max_bytes: HOST_PROBE_STDOUT_MAX_BYTES,
        stderr_max_bytes: HOST_PROBE_STDERR_MAX_BYTES,
    }
}

impl Sources for LocalSources<'_> {
    fn plugins(&self, project_root: &Path) -> Result<Vec<u8>, Error> {
        crate::private_fs::HostDirectory::open(project_root).map_err(|_| Error::NotRegularFile)?;
        self.0
            .run(&probe_command(project_root, &["plugin", "list", "--json"]))
    }

    fn plugin_hooks(&self, install_path: &Path) -> Result<Vec<u8>, Error> {
        read_document(&install_path.join("hooks/hooks.json"))
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

pub(super) fn regular_path(path: &Path) -> Result<(), Error> {
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

/// `(O_NONBLOCK | O_NOFOLLOW, ELOOP)` for one `(OS, ARCH)`, only where the
/// values were checked against local authoritative definitions: the macOS
/// SDK `sys/fcntl.h` / `sys/errno.h` (`0x4 | 0x100`, `62`) and libc 0.2.189
/// Linux gnu/musl/uclibc and Android (`O_NONBLOCK` `0x800`, `ELOOP` `40`;
/// `O_NOFOLLOW` `0x20000` on x86/x86_64 but `0x8000` on arm/aarch64). This
/// is the target set of `PROJECT_OPEN_FLAGS` in the execution env policy.
/// Other targets (including Windows, the BSDs and Linux mips/sparc/powerpc/
/// riscv64, whose values vary or were not checked) are refused, not guessed.
pub(super) fn no_follow_open(os: &str, arch: &str) -> Option<(i32, i32)> {
    match (os, arch) {
        ("macos", "x86_64" | "aarch64") => Some((0x4 | 0x100, 62)),
        ("linux" | "android", "x86" | "x86_64") => Some((0x800 | 0x2_0000, 40)),
        ("linux" | "android", "arm" | "aarch64") => Some((0x800 | 0x8000, 40)),
        _ => None,
    }
}

/// This target's no-follow open, or `Unsupported` before any filesystem access.
pub(super) fn open_flags() -> Result<(i32, i32), Error> {
    no_follow_open(std::env::consts::OS, std::env::consts::ARCH).ok_or(Error::Unsupported)
}

pub(super) fn read_document(path: &Path) -> Result<Vec<u8>, Error> {
    let flags = open_flags()?;
    regular_path(path)?;
    open_document(path, flags)
}

#[cfg(not(unix))]
pub(super) fn open_document(_: &Path, _: (i32, i32)) -> Result<Vec<u8>, Error> {
    Err(Error::Unsupported)
}

/// Opens the entry `regular_path` accepted. A final-component race must not
/// turn this into a blocking FIFO open or follow a replacement symlink.
#[cfg(unix)]
pub(super) fn open_document(path: &Path, (flags, eloop): (i32, i32)) -> Result<Vec<u8>, Error> {
    let _ = (flags, eloop); // Checked target table remains the supported-platform gate.
    let parent = path.parent().ok_or(Error::NotRegularFile)?;
    let dir = crate::private_fs::HostDirectory::open_read(parent).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Missing
        } else {
            Error::NotRegularFile
        }
    })?;
    let name = path.file_name().ok_or(Error::NotRegularFile)?;
    let file = dir.open_file(name, false, false).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Missing
        } else {
            Error::NotRegularFile
        }
    })?;
    let mut bytes = Vec::new();
    (&file)
        .take((HOST_STRUCTURED_OBSERVATION_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    dir.verify_file(name, &file, false)
        .map_err(|_| Error::NotRegularFile)?;
    if bytes.len() > HOST_STRUCTURED_OBSERVATION_MAX_BYTES {
        return Err(Error::StructuredObservationTooLarge);
    }
    Ok(bytes)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn drop_ins(path: &Path) -> Result<Vec<PathBuf>, Error> {
    let dir = crate::private_fs::HostDirectory::open_read(path).map_err(io_error)?;
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
    dir.verify().map_err(|_| Error::NotRegularFile)?;
    paths.sort();
    Ok(paths)
}
