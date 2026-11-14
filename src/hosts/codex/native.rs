//! Native observer delivery under the same trusted-owner custody as the spool.
//! Enrollment is an explicit local operator action, never a hook-payload claim.
use crate::{HostEventSpool, NormalizedHostEventId, private_fs::HostDirectory};
use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    executable: PathBuf,
    executable_digest: String,
    project: PathBuf,
    plugin: PathBuf,
    plugin_digest: String,
}
const FILE: &str = "codex-native.json";
const FILES: [&str; 3] = [
    ".codex-plugin/plugin.json",
    "hooks/hooks.json",
    "bin/receipts-hook",
];
fn digest_file(path: &Path) -> Result<String, &'static str> {
    let parent = HostDirectory::open_read(path.parent().ok_or("missing parent")?)
        .map_err(|_| "unsafe identity parent")?;
    use std::io::Read;
    let name = path.file_name().ok_or("missing filename")?;
    let mut file = parent
        .open_file(name, false, false)
        .map_err(|_| "unsafe identity file")?;
    let limit = 512 * 1024 * 1024;
    if file.metadata().map_err(|_| "identity unavailable")?.len() > limit {
        return Err("identity oversized");
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65_536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|_| "identity read failed")?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err("identity oversized");
        }
        hash.update(&buffer[..n]);
    }
    parent
        .verify_file(name, &file, false)
        .map_err(|_| "identity changed")?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn plugin_digest(root: &Path) -> Result<String, &'static str> {
    let mut digest = Sha256::new();
    for file in FILES {
        digest.update(file);
        digest.update(digest_file(&root.join(file))?);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
/// Pins operator-reviewed executable and installed plugin bytes for one project.
/// Does not approve hooks or change Codex policy. Re-enroll after an update.
pub fn enroll(
    directory: &Path,
    executable: &Path,
    project: &Path,
    plugin: &Path,
) -> Result<(), &'static str> {
    for path in [directory, executable, project, plugin] {
        if !path.is_absolute() || std::fs::canonicalize(path).ok().as_deref() != Some(path) {
            return Err("enrollment paths must be canonical and absolute");
        }
    }
    if directory.starts_with(project) || directory.starts_with(plugin) {
        return Err("enrollment must be outside project and plugin");
    }
    HostDirectory::open(project).map_err(|_| "unsafe project")?;
    let dir = HostDirectory::open(directory).map_err(|_| "unsafe enrollment directory")?;
    let record = Enrollment {
        executable: executable.into(),
        executable_digest: digest_file(executable)?,
        project: project.into(),
        plugin: plugin.into(),
        plugin_digest: plugin_digest(plugin)?,
    };
    let bytes = serde_json::to_vec(&record).map_err(|_| "enrollment encoding failed")?;
    dir.replace(OsStr::new(FILE), &bytes)
        .map_err(|_| "enrollment write failed")
}

/// This handle binds enrollment to live OS ancestry; delivery rechecks pinned bytes.
/// Same-user hostile processes are outside custody, as with HostEventSpool.
pub struct NativeCodexDelivery {
    record: Enrollment,
    directory: PathBuf,
}
impl NativeCodexDelivery {
    pub fn admit(directory: &Path) -> Result<Self, &'static str> {
        let dir = HostDirectory::open(directory).map_err(|_| "native enrollment unavailable")?;
        let bytes = dir
            .read(OsStr::new(FILE), true, 16_384)
            .map_err(|_| "native enrollment unavailable")?;
        let record: Enrollment =
            serde_json::from_slice(&bytes).map_err(|_| "invalid native enrollment")?;
        if directory.starts_with(&record.project) || directory.starts_with(&record.plugin) {
            return Err("unsafe enrollment location");
        }
        verify_parent(&record.executable, &record.plugin)?;
        Ok(Self {
            record,
            directory: directory.into(),
        })
    }
    pub fn deliver(
        &self,
        event: &str,
        input: &[u8],
        id: NormalizedHostEventId,
        at: OrchestrationDateTimeV1,
    ) -> Result<(), &'static str> {
        // Recheck identity and ancestry immediately before accepting untrusted input.
        if digest_file(&self.record.executable)? != self.record.executable_digest
            || plugin_digest(&self.record.plugin)? != self.record.plugin_digest
        {
            return Err("native identity changed");
        }
        let (pid, birth) = verify_parent(&self.record.executable, &self.record.plugin)?;
        let parsed = crate::claude::parse_claude_hook(event, input)
            .map_err(|_| "invalid Codex hook input")?;
        if parsed.cwd() != self.record.project {
            return Err("Codex hook project mismatch");
        }
        let event = super::hook_map::normalize_codex_hook(event, input, id, at)
            .map_err(|_| "invalid Codex observer event")?;
        let storage =
            HostDirectory::open(&self.directory).map_err(|_| "native storage unavailable")?;
        let guard = storage
            .open_file(OsStr::new("session.lock"), true, true)
            .map_err(|_| "native lock unavailable")?;
        crate::private_fs::lock(&guard).map_err(|_| "native lock failed")?;
        let key = format!("session-{pid}-{birth}");
        let session = storage.read(OsStr::new(&key), true, 200);
        if event.event_type() != crate::NormalizedHostEventType::HostSessionStarted
            && session.as_deref().ok() != Some(parsed.session_id().as_bytes())
        {
            return Err("Codex hook session mismatch or missing SessionStart");
        }
        HostEventSpool::for_project(&self.record.project)
            .map_err(|_| "Codex sink unavailable")?
            .append(&event)
            .map_err(|_| "Codex sink refused")?;
        storage
            .replace(OsStr::new(&key), parsed.session_id().as_bytes())
            .map_err(|_| "native session write failed")
    }
}

#[derive(Debug)]
struct Process {
    parent: i32,
    birth: String,
    executable: PathBuf,
    args: Vec<String>,
}
fn verify_parent(executable: &Path, plugin: &Path) -> Result<(i32, String), &'static str> {
    // SAFETY: getppid has no preconditions.
    let pid = unsafe { libc::getppid() };
    verify_parent_from(pid, executable, plugin, process)
}
fn verify_parent_from(
    mut pid: i32,
    executable: &Path,
    plugin: &Path,
    inspect: impl Fn(i32) -> Result<Process, &'static str>,
) -> Result<(i32, String), &'static str> {
    for depth in 0..3 {
        let process = inspect(pid)?;
        if process.executable == executable {
            if process.args.iter().any(|a| a.contains("bypass-hook-trust")) {
                return Err("Codex trust bypass refused");
            }
            return Ok((pid, process.birth));
        }
        // Only Codex's fixed command-hook shell may intervene. Tool shells,
        // wrappers, unknown ancestry and standalone CLI calls fail closed.
        let expected = format!("\"{}/bin/receipts-hook\" ", plugin.display());
        if depth > 1
            || !matches!(process.executable.to_str(), Some("/bin/sh" | "/bin/dash"))
            || !process.args.windows(2).any(|a| {
                a[0] == "-c"
                    && [
                        "SessionStart",
                        "SessionEnd",
                        "PostToolUse",
                        "PreCompact",
                        "PostCompact",
                    ]
                    .iter()
                    .any(|event| {
                        a[1] == format!("{expected}{event}")
                            || a[1] == format!("\"${{PLUGIN_ROOT}}/bin/receipts-hook\" {event}")
                    })
            })
        {
            return Err("not a native Codex hook process");
        }
        pid = process.parent;
    }
    Err("native Codex parent unavailable")
}

#[cfg(target_os = "linux")]
fn process(pid: i32) -> Result<Process, &'static str> {
    let base = PathBuf::from(format!("/proc/{pid}"));
    let executable =
        std::fs::read_link(base.join("exe")).map_err(|_| "parent identity unavailable")?;
    let stat =
        std::fs::read_to_string(base.join("stat")).map_err(|_| "parent status unavailable")?;
    let parent = stat
        .rsplit_once(')')
        .and_then(|(_, s)| s.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("parent status invalid")?;
    let birth = stat
        .rsplit_once(')')
        .and_then(|(_, s)| s.split_whitespace().nth(19))
        .ok_or("parent birth unavailable")?
        .to_owned();
    let bytes = std::fs::read(base.join("cmdline")).map_err(|_| "parent arguments unavailable")?;
    if bytes.len() > 131_072 {
        return Err("parent arguments oversized");
    }
    let args = bytes
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .map(|b| String::from_utf8(b.to_vec()).map_err(|_| "parent arguments invalid"))
        .collect::<Result<_, _>>()?;
    Ok(Process {
        birth,
        parent,
        executable,
        args,
    })
}
#[cfg(target_os = "macos")]
fn process(pid: i32) -> Result<Process, &'static str> {
    use std::os::unix::ffi::OsStrExt;
    let mut path = vec![0u8; 4096];
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: buffers are valid for the supplied sizes; OS fills process metadata.
    let n = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    let size = std::mem::size_of_val(&info);
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    if n <= 0 || read != size as i32 {
        return Err("parent identity unavailable");
    }
    let executable = PathBuf::from(OsStr::from_bytes(
        path.split(|b| *b == 0)
            .next()
            .ok_or("parent path invalid")?,
    ));
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut bytes = vec![0u8; 131_072];
    let mut len = bytes.len();
    // SAFETY: sysctl receives valid MIB and output buffer, with no input value.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || len < 4
    {
        return Err("parent arguments unavailable");
    }
    bytes.truncate(len);
    let argc = i32::from_ne_bytes(bytes[..4].try_into().map_err(|_| "parent argc invalid")?);
    if !(1..=4096).contains(&argc) {
        return Err("parent argc invalid");
    }
    let tail = &bytes[4..];
    let end = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or("parent arguments invalid")?;
    let tail = &tail[end..];
    let start = tail
        .iter()
        .position(|b| *b != 0)
        .ok_or("parent arguments absent")?;
    let args = tail[start..]
        .split(|b| *b == 0)
        .take(argc as usize)
        .map(|b| String::from_utf8(b.to_vec()).map_err(|_| "parent arguments invalid"))
        .collect::<Result<Vec<_>, _>>()?;
    if args.len() != argc as usize {
        return Err("parent arguments incomplete");
    }
    Ok(Process {
        birth: format!("{}-{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        parent: info.pbi_ppid as i32,
        executable,
        args,
    })
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process(_: i32) -> Result<Process, &'static str> {
    Err("native ancestry unsupported")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_lineage_requires_fixed_shell_and_refuses_trust_bypass() {
        let inspect = |shell: &str, args: Vec<String>, bypass: bool| {
            verify_parent_from(
                10,
                Path::new("/installed/codex"),
                Path::new("/plugin"),
                |pid| {
                    Ok(if pid == 10 {
                        Process {
                            parent: 9,
                            birth: "one".into(),
                            executable: shell.into(),
                            args: args.clone(),
                        }
                    } else {
                        Process {
                            parent: 1,
                            birth: "two".into(),
                            executable: "/installed/codex".into(),
                            args: if bypass {
                                vec!["--dangerously-bypass-hook-trust".into()]
                            } else {
                                vec![]
                            },
                        }
                    })
                },
            )
        };
        let args = vec![
            "sh".into(),
            "-c".into(),
            "\"${PLUGIN_ROOT}/bin/receipts-hook\" SessionStart".into(),
        ];
        assert!(inspect("/bin/sh", args.clone(), false).is_ok());
        assert!(inspect("/bin/sh", args.clone(), true).is_err());
        assert!(inspect("/bin/zsh", args.clone(), false).is_err());
        let mut forged = args.clone();
        forged[2].push_str("; arbitrary-command");
        assert!(inspect("/bin/sh", forged, false).is_err());
        assert!(
            inspect(
                "/bin/sh",
                vec![
                    "sh".into(),
                    "-c".into(),
                    "receipts hook codex SessionStart".into()
                ],
                false
            )
            .is_err()
        );
    }
}
