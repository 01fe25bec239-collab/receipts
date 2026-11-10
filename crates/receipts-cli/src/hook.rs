//! Hook-only CLI bridge: `receipts hook claude <Event>` (HI-W2-2/HI-W2-3).
//!
//! Composition only. Host parses and maps the untrusted hook input, the
//! spool is Host's append-only inbox, Workspace owns worktree semantics and
//! the admitted probe runner owns process execution. Nothing here opens
//! State or holds a State dependency.
//!
//! Exit/stdout contract with Claude Code command hooks:
//! * observer hooks always exit 0 (fail open) with empty stdout; a refusal is
//!   one bounded stderr line;
//! * `WorktreeCreate` prints exactly `<absolute path>\n` and exits 0, or exits
//!   1 with a stderr reason, which aborts creation (never exit 2);
//! * `WorktreeRemove` exits 0; removal requires admitted live ownership and policy.

use receipts_host_integration::private_fs::{HostDirectory, lock};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use receipts_host_integration::claude::{
    ClaudeHookDetail, ClaudeHookEvent, ClaudeHookInvocation, ClaudeHostEnvironment,
    parse_claude_hook, utc_now as now_utc, worktree_create_stdout, worktree_target,
};
use receipts_host_integration::wire::{
    host_capability_report_from_json, host_capability_report_to_json,
};
use receipts_host_integration::{
    ClaudeHostAdapter, HostAdapter, HostCapabilityFreshnessDisposition, HostCapabilityProbeStatus,
    HostCapabilityReport, HostCapabilityReportSelectionCompositionInputs,
    HostCapabilityReportSelectionCompositionOutcome, HostCapabilityStaleReason,
    HostDetectionSignals, HostEventSpool, HostSessionActivationRequest, NormalizedHostEvent,
    NormalizedHostEventId, NormalizedHostEventSourceClass,
    compose_host_capability_report_selection, emit_validated_normalized_host_event,
    prepare_host_session,
};
use receipts_workspace_execution::execution::{ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider};
use receipts_workspace_execution::{
    WorktreeCreateMode, WorktreeCreateRequest, validate_branch_name,
};

use crate::WorkspaceHostProbeRunner;

/// Capability report file inside the plugin data directory.
pub const CAPABILITY_REPORT_FILE: &str = "host-capability-report.json";
const SMALL_FILE_MAX_BYTES: u64 = 1_048_576;

/// Host-set context, read from exact variable names by the binary.
#[derive(Debug, Clone, Default)]
pub struct ClaudeHookContext {
    /// `CLAUDECODE=1`.
    pub claudecode: bool,
    /// `CLAUDE_PROJECT_DIR`: the spool's project root.
    pub project_dir: Option<PathBuf>,
    /// `CLAUDE_PLUGIN_ROOT`: the launching plugin's directory.
    pub plugin_root: Option<PathBuf>,
    /// `CLAUDE_PLUGIN_DATA`: host-scoped cache directory.
    pub plugin_data: Option<PathBuf>,
    /// `RECEIPTS_CLAUDE_EXECUTABLE`: absolute `claude` for probes. Without
    /// it no probe runs (there is no PATH lookup).
    pub claude_executable: Option<PathBuf>,
}

/// What the process must write and return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

fn note(out: &mut HookOutput, message: impl std::fmt::Display) {
    out.stderr.push_str(&format!("receipts: {message}\n"));
}

/// Runs one Claude hook to completion. Never panics on host input.
pub fn run_claude_hook(event: &str, stdin: &[u8], ctx: &ClaudeHookContext) -> HookOutput {
    run_claude_hook_with_composition(event, stdin, ctx, &NoHostComposition)
}

/// Only the admitted composition may supply task/cleanup authority. Never implemented by a hook decoder.
pub trait HostCompositionPort {
    /// Retain a fresh proof in trusted composition if later cleanup is desired.
    fn plain_created(
        &self,
        _project: &Path,
        _owned: receipts_workspace_execution::OwnedPlainWorktree,
    ) -> Result<(), &'static str> {
        Ok(())
    }

    fn task(
        &self,
        _project: &Path,
        _invocation: &ClaudeHookInvocation,
    ) -> Result<Option<receipts_workspace_execution::WorktreeCreateTask>, &'static str> {
        Ok(None)
    }
    fn plain_removal(
        &self,
        _project: &Path,
        _path: &Path,
    ) -> Result<Option<receipts_workspace_execution::OwnedPlainWorktree>, &'static str> {
        Ok(None)
    }
    /// Return independently admitted Core/worker facts, never a reclassified hook payload.
    fn translated_event(
        &self,
        _project: &Path,
        _invocation: &ClaudeHookInvocation,
    ) -> Result<Option<(NormalizedHostEventSourceClass, NormalizedHostEvent)>, &'static str> {
        Ok(None)
    }
}
struct NoHostComposition;
impl HostCompositionPort for NoHostComposition {}

pub fn run_claude_hook_with_composition(
    event: &str,
    stdin: &[u8],
    ctx: &ClaudeHookContext,
    composition: &dyn HostCompositionPort,
) -> HookOutput {
    // Invalidate currency before parsing/probing; even early failures cannot retain a current report.
    let refresh =
        (event == "SessionStart" && ctx.plugin_data.is_some()).then(|| RefreshStorage::begin(ctx));
    let mut out = run_hook_inner(
        event,
        stdin,
        ctx,
        composition,
        refresh.as_ref().and_then(|r| r.as_ref().ok()),
    );
    if let Some(Err(error)) = refresh {
        note(&mut out, error);
    }
    out
}

fn run_hook_inner(
    event: &str,
    stdin: &[u8],
    ctx: &ClaudeHookContext,
    composition: &dyn HostCompositionPort,
    refresh: Option<&RefreshStorage>,
) -> HookOutput {
    let blocking = ClaudeHookEvent::parse(event).is_some_and(ClaudeHookEvent::is_blocking);
    let mut out = HookOutput {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: 0,
    };
    let invocation = match parse_claude_hook(event, stdin) {
        Ok(invocation) => invocation,
        Err(error) => {
            note(&mut out, error);
            out.exit_code = i32::from(blocking);
            return out;
        }
    };
    let project = match verified_project(ctx, invocation.cwd()) {
        Ok(project) => project,
        Err(message) => {
            note(&mut out, message);
            out.exit_code = i32::from(blocking);
            return out;
        }
    };
    let spool = HostEventSpool::for_project(&project).ok();
    match invocation.detail() {
        ClaudeHookDetail::WorktreeCreate { name } => {
            worktree_create(
                &invocation,
                name,
                &project,
                spool.as_ref(),
                composition,
                &mut out,
            );
        }
        ClaudeHookDetail::WorktreeRemove { worktree_path } => {
            let removed = composition
                .plain_removal(&project, worktree_path)
                .and_then(|owned| {
                    let owned =
                        owned.ok_or("no admitted plain-worktree ownership; worktree retained")?;
                    owned
                        .remove(&project, worktree_path)
                        .map_err(|_| "plain-worktree cleanup failed or unverified; inspect path and registration")
                });
            match removed {
                Ok(()) => {
                    if let (Some(spool), Some(event)) = (
                        &spool,
                        invocation.workspace_removed_event(
                            worktree_path,
                            new_event_id(),
                            now_utc(),
                        ),
                    ) {
                        emit(
                            spool,
                            NormalizedHostEventSourceClass::CoreDriven,
                            &event,
                            &mut out,
                        );
                    }
                }
                Err(message) => note(
                    &mut out,
                    format!("{message}; worktree path {}", worktree_path.display()),
                ),
            }
        }
        _ => {
            if invocation.event() == ClaudeHookEvent::SessionStart
                && let Some(refresh) = refresh
                && let Err(message) = record_capabilities(ctx, &project, refresh)
            {
                note(&mut out, message);
            }
            if let Some(spool) = &spool {
                match invocation.observed_event(new_event_id(), now_utc()) {
                    Some(event) => emit(
                        spool,
                        NormalizedHostEventSourceClass::HostHook,
                        &event,
                        &mut out,
                    ),
                    None => match composition.translated_event(&project, &invocation) {
                        Ok(Some((class, event)))
                            if invocation.admits_translation(class, &event) =>
                        {
                            emit(spool, class, &event, &mut out)
                        }
                        Ok(Some(_)) => note(
                            &mut out,
                            "trusted event binding/source mismatch; not spooled",
                        ),
                        Ok(None) => note(
                            &mut out,
                            "trusted Core/worker supplier unavailable; not spooled",
                        ),
                        Err(message) => note(&mut out, message),
                    },
                }
            } else {
                note(
                    &mut out,
                    "no absolute CLAUDE_PROJECT_DIR; event not spooled",
                );
            }
        }
    }
    out
}

fn verified_project(ctx: &ClaudeHookContext, cwd: &Path) -> Result<PathBuf, &'static str> {
    let project = ctx
        .project_dir
        .as_ref()
        .ok_or("no absolute CLAUDE_PROJECT_DIR; event not spooled")?;
    let dir =
        HostDirectory::open(project).map_err(|_| "unsafe CLAUDE_PROJECT_DIR; event not spooled")?;
    HostDirectory::open(cwd).map_err(|_| "unsafe hook cwd; event not spooled")?;
    if !cwd.starts_with(project) {
        return Err("hook cwd/project mismatch; event not spooled");
    }
    // A nested repository is a different project, even inside this directory.
    if cwd
        .ancestors()
        .any(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
    {
        let (root, _) = repository_head(cwd)
            .map_err(|_| "hook repository identity unavailable; event not spooled")?;
        if root != *project {
            return Err("hook repository/project mismatch; event not spooled");
        }
    }
    Ok(dir.path().to_path_buf())
}

fn emit(
    spool: &HostEventSpool,
    class: NormalizedHostEventSourceClass,
    event: &NormalizedHostEvent,
    out: &mut HookOutput,
) {
    let adapter = ClaudeHostAdapter::new(
        ClaudeHostEnvironment::default(),
        spool.clone(),
        conservative_report(),
    );
    match emit_validated_normalized_host_event(&adapter, class, event) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => note(out, error),
        Err(error) => note(out, error),
    }
}

fn worktree_create(
    invocation: &ClaudeHookInvocation,
    name: &str,
    project: &Path,
    spool: Option<&HostEventSpool>,
    composition: &dyn HostCompositionPort,
    out: &mut HookOutput,
) {
    let fail = |out: &mut HookOutput, message: String| {
        note(out, format!("WorktreeCreate refused: {message}"));
        out.exit_code = 1;
    };
    let (root, head) = match repository_head(invocation.cwd()) {
        Ok(found) => found,
        Err(message) => return fail(out, message),
    };
    let (target, branch) = worktree_target(&root, name);
    if root != project {
        return fail(out, "repository/project mismatch".into());
    }
    let parent = HostDirectory::open(project)
        .and_then(|d| d.child(OsStr::new(".claude"), true))
        .and_then(|d| d.child(OsStr::new("worktrees"), true));
    let parent = match parent {
        Ok(p) => p,
        Err(_) => return fail(out, "unsafe worktree parent".into()),
    };
    let task = match composition.task(project, invocation) {
        Ok(task) => task,
        Err(e) => return fail(out, e.into()),
    };
    if parent.verify().is_err() {
        return fail(out, "worktree parent changed".into());
    }
    // Workspace refusals are final; never retry a rejected task operation as plain.
    let response = WorktreeCreateRequest::new(&root, &target, &branch, &head, task)
        .and_then(|request| request.create());
    let response = match response {
        Ok(response) => response,
        Err(error) => return fail(out, error.to_string()),
    };
    if let Some(owned) = response.plain_ownership()
        && let Err(message) = composition.plain_created(project, owned.clone())
    {
        note(out, message);
    }
    let Some(stdout) = worktree_create_stdout(response.path()) else {
        return fail(
            out,
            "created path cannot be reported to the host".to_owned(),
        );
    };
    out.stdout = stdout;
    let mode = match response.mode() {
        WorktreeCreateMode::TaskProvisioned => "TASK_PROVISIONED",
        WorktreeCreateMode::TaskAttached => "TASK_ATTACHED",
        WorktreeCreateMode::PlainCreated => "PLAIN_CREATED",
        WorktreeCreateMode::PlainAttached => "PLAIN_ATTACHED",
    };
    let task_bound = matches!(
        response.mode(),
        WorktreeCreateMode::TaskProvisioned | WorktreeCreateMode::TaskAttached
    );
    // Spooling is best-effort: the worktree exists and is reported anyway.
    match (
        spool,
        invocation.workspace_created_event(
            response.path(),
            mode,
            task_bound,
            new_event_id(),
            now_utc(),
        ),
    ) {
        (Some(spool), Some(event)) => emit(
            spool,
            NormalizedHostEventSourceClass::CoreDriven,
            &event,
            out,
        ),
        _ => note(out, "WORKSPACE_CREATED not spooled"),
    }
}

/// Read-only discovery of the Git toplevel containing `cwd` and its HEAD
/// commit, from Git's own files (no process). Workspace re-verifies the base
/// with hardened Git before creating anything.
pub fn repository_head(cwd: &Path) -> Result<(PathBuf, String), String> {
    let cwd = fs::canonicalize(cwd).map_err(|_| "cwd is not an accessible directory".to_owned())?;
    let root = cwd
        .ancestors()
        .find(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
        .ok_or("cwd is not inside a Git repository")?
        .to_path_buf();
    let dot_git = root.join(".git");
    let git_dir = if fs::symlink_metadata(&dot_git)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        dot_git
    } else {
        let text = small_text(&dot_git)?;
        let target = text
            .strip_prefix("gitdir: ")
            .map(str::trim_end)
            .ok_or(".git file is not a gitdir link")?;
        fs::canonicalize(root.join(target))
            .map_err(|_| "gitdir link does not resolve".to_owned())?
    };
    let common = match small_text(&git_dir.join("commondir")) {
        Ok(rel) => fs::canonicalize(git_dir.join(rel.trim_end()))
            .map_err(|_| "commondir does not resolve".to_owned())?,
        Err(_) => git_dir.clone(),
    };
    let head = small_text(&git_dir.join("HEAD"))?;
    let head = head.trim_end();
    let sha = match head.strip_prefix("ref: refs/heads/") {
        None => head.to_owned(),
        Some(branch) => {
            validate_branch_name(branch).map_err(|r| format!("HEAD branch is invalid: {r}"))?;
            let loose = [&git_dir, &common]
                .iter()
                .find_map(|dir| small_text(&dir.join("refs/heads").join(branch)).ok());
            match loose {
                Some(sha) => sha.trim_end().to_owned(),
                None => small_text(&common.join("packed-refs"))
                    .ok()
                    .and_then(|packed| {
                        packed.lines().find_map(|line| {
                            let (sha, name) = line.split_once(' ')?;
                            (name == format!("refs/heads/{branch}")).then(|| sha.to_owned())
                        })
                    })
                    .ok_or("HEAD branch has no commit yet")?,
            }
        }
    };
    let hex = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !matches!(sha.len(), 40 | 64) || !hex(&sha) {
        return Err("HEAD does not name a commit".to_owned());
    }
    Ok((root, sha))
}

/// Bounded read of a small regular non-symlink file.
fn small_text(path: &Path) -> Result<String, String> {
    let parent = path.parent().ok_or("file has no parent")?;
    let dir = HostDirectory::open_read(parent).map_err(|_| "unsafe file parent")?;
    let bytes = dir
        .read(
            path.file_name().ok_or("file has no name")?,
            false,
            SMALL_FILE_MAX_BYTES,
        )
        .map_err(|_| "small file unreadable or unsafe")?;
    String::from_utf8(bytes).map_err(|_| "small file is not UTF-8".into())
}

/// The conservative report used before any observation: FAILED probe,
/// SUPERVISED, reason UNKNOWN — derived by the frozen selector, not by hand.
pub fn conservative_report() -> HostCapabilityReport {
    let inputs = HostCapabilityReportSelectionCompositionInputs {
        report_validity_proven_current: true,
        host_id: "claude-code".into(),
        host_version: None,
        probe_status: HostCapabilityProbeStatus::Failed,
        validity_fingerprint: None,
        hook_definition_digest: None,
        relevant_config_digest: None,
        stale_reason: HostCapabilityStaleReason::ProbeFailed,
        plugin_supported: None,
        plugin_installed: None,
        manifest_path: None,
        supports_skills: None,
        supports_commands: None,
        supports_subagents: None,
        supports_mcp: None,
        hooks_supported: None,
        hooks_configured: None,
        hook_trust_required: None,
        hooks_trusted: None,
        hooks_enabled: None,
        hooks_allowed_by_admin_policy: None,
        hook_events: None,
        blocking_hook_events: None,
        hook_coverage_class: receipts_host_integration::HostCapabilityHookCoverageClass::Unknown,
        required_hook_coverage_satisfied: None,
        mode_override: None,
        plugin_data_path: None,
        sandbox_modes: None,
        evidence_label: None,
        source_claim_id: None,
    };
    let Ok(HostCapabilityReportSelectionCompositionOutcome::Selected { report }) =
        compose_host_capability_report_selection(&inputs)
    else {
        unreachable!("the frozen selector prescribes SUPERVISED for a current FAILED probe")
    };
    let now = now_utc();
    HostCapabilityReport::new(*report, now.clone(), now)
}

/// SessionStart capability refresh with coupled cache consumption. Writes the
/// selected report (0600, atomic rename); failed refresh retains explicit
/// failed currency. Never writes host settings.
struct RefreshStorage {
    dir: HostDirectory,
    _lock: fs::File,
    cached: Option<HostCapabilityReport>,
}
impl RefreshStorage {
    fn begin(ctx: &ClaudeHookContext) -> Result<Self, String> {
        let data = ctx
            .plugin_data
            .as_deref()
            .ok_or("capability currency unavailable (no CLAUDE_PLUGIN_DATA)")?;
        let result = (|| -> Result<Self, String> {
            let dir =
                HostDirectory::create_path(data).map_err(|_| "unsafe plugin data directory")?;
            let file = dir
                .open_file(OsStr::new(".capability.lock"), true, true)
                .map_err(|_| "unsafe capability lock")?;
            lock(&file).map_err(|_| "cannot lock capability cache")?;
            dir.verify_file(OsStr::new(".capability.lock"), &file, true)
                .map_err(|_| "capability lock replaced")?;
            let cached = match dir.read(
                OsStr::new(CAPABILITY_REPORT_FILE),
                true,
                SMALL_FILE_MAX_BYTES,
            ) {
                Ok(bytes) => std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|t| host_capability_report_from_json(t).ok()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err("unsafe or unreadable capability report".into()),
            };
            dir.replace(
                OsStr::new(CAPABILITY_REPORT_FILE),
                host_capability_report_to_json(&conservative_report()).as_bytes(),
            )
            .map_err(|_| "cannot persist failed/stale currency")?;
            Ok(Self {
                dir,
                _lock: file,
                cached,
            })
        })();
        result.map_err(|e| format!("capability currency invalidation failed: {e}; prior report may remain, do not treat it as current"))
    }
}

fn record_capabilities(
    ctx: &ClaudeHookContext,
    project: &Path,
    storage: &RefreshStorage,
) -> Result<HostCapabilityFreshnessDisposition, String> {
    let exe = ctx
        .claude_executable
        .as_deref()
        .ok_or("capability report not refreshed (no RECEIPTS_CLAUDE_EXECUTABLE)")?;
    let env = ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription)
        .map_err(|_| "probe environment policy unavailable".to_owned())?;
    let runner = WorkspaceHostProbeRunner::new(exe, env)
        .map_err(|e| format!("claude executable not admitted: {e:?}"))?;
    HostDirectory::open(project).map_err(|_| "unsafe capability project")?;
    storage
        .dir
        .verify()
        .map_err(|_| "plugin data directory changed")?;
    let cached = &storage.cached;
    let spool = HostEventSpool::for_project(project).map_err(|e| e.to_string())?;
    let adapter = ClaudeHostAdapter::new(
        ClaudeHostEnvironment {
            claudecode: ctx.claudecode,
        },
        spool,
        conservative_report(),
    );
    let outcome = prepare_host_session(
        &adapter,
        &HostSessionActivationRequest {
            detection_signals: HostDetectionSignals {
                claude_detected: adapter.detect(),
                codex_detected: false,
            },
            explicit_host_override: Some(adapter.id()),
            project_root: project.to_path_buf(),
            mode_override: None,
            cached: cached.as_ref().map(HostCapabilityReport::core),
            hook_execution: None,
        },
        &runner,
    )
    .map_err(|e| e.to_string())?;
    let entry = outcome.capability_refresh.cache_entry.as_ref();
    let Some(entry) = entry else {
        return Err("capability observation incomplete; failed/stale currency retained".into());
    };
    let now = now_utc();
    let probed_at = match (&cached, outcome.freshness_disposition) {
        (Some(cached), HostCapabilityFreshnessDisposition::ReuseReport) => {
            cached.probed_at().clone()
        }
        _ => now.clone(),
    };
    let report = HostCapabilityReport::new(entry.report().clone(), probed_at, now);
    storage
        .dir
        .replace(
            OsStr::new(CAPABILITY_REPORT_FILE),
            host_capability_report_to_json(&report).as_bytes(),
        )
        .map_err(|_| "cannot install capability report; currency remains failed/stale")?;
    Ok(outcome.freshness_disposition)
}

/// ULID: 48-bit millisecond time and 80 random bits, Crockford base32.
pub fn new_event_id() -> NormalizedHostEventId {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
        & ((1 << 48) - 1);
    let mut random = [0u8; 10];
    let filled = fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut random));
    if filled.is_err() {
        // ponytail: fallback entropy (pid/time) only if /dev/urandom is
        // unreadable; ids stay well formed, uniqueness weakens to best effort.
        let seed = (std::process::id() as u128) << 64
            | SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
        random.copy_from_slice(&seed.to_be_bytes()[6..]);
    }
    let value = millis << 80
        | random
            .iter()
            .fold(0u128, |acc, b| acc << 8 | u128::from(*b));
    let text: String = (0..26)
        .rev()
        .map(|i| ALPHABET[((value >> (5 * i)) & 31) as usize] as char)
        .collect();
    NormalizedHostEventId::try_new(text).expect("Crockford alphabet, 26 characters")
}
