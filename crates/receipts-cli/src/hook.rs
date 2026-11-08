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
//! * `WorktreeRemove` removes nothing (see [`ClaudeHookDetail::WorktreeRemove`]
//!   handling below) and exits 0.

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
    let spool = spool_for(ctx);
    match invocation.detail() {
        ClaudeHookDetail::WorktreeCreate { name } => {
            worktree_create(&invocation, name, spool.as_ref(), &mut out);
        }
        ClaudeHookDetail::WorktreeRemove { worktree_path } => {
            // Q-02 (2.1.246): with a custom WorktreeCreate, Claude leaves
            // removal to this hook. Workspace exposes no verified removal for
            // plain worktrees, so nothing is deleted: the tree and its work
            // are preserved and named. No WORKSPACE_REMOVED is emitted.
            note(
                &mut out,
                format!(
                    "worktree kept at {}; remove it with `git worktree remove` when finished",
                    worktree_path.display()
                ),
            );
        }
        _ => {
            if invocation.event() == ClaudeHookEvent::SessionStart
                && let Err(message) = record_capabilities(&invocation, ctx)
            {
                note(&mut out, message);
            }
            if let Some(spool) = &spool {
                match invocation.observed_event(new_event_id(), now_utc()) {
                    Some(event) => emit(spool, &event, &mut out),
                    None => note(&mut out, "event not representable; not spooled"),
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

fn spool_for(ctx: &ClaudeHookContext) -> Option<HostEventSpool> {
    let dir = ctx.project_dir.as_ref().filter(|p| p.is_absolute())?;
    HostEventSpool::for_project(&fs::canonicalize(dir).ok()?).ok()
}

fn emit(spool: &HostEventSpool, event: &NormalizedHostEvent, out: &mut HookOutput) {
    let adapter = ClaudeHostAdapter::new(
        ClaudeHostEnvironment::default(),
        spool.clone(),
        conservative_report(),
    );
    let class = match event.event_type() {
        receipts_host_integration::NormalizedHostEventType::WorkspaceCreated => {
            NormalizedHostEventSourceClass::CoreDriven
        }
        _ => NormalizedHostEventSourceClass::HostHook,
    };
    match emit_validated_normalized_host_event(&adapter, class, event) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => note(out, error),
        Err(error) => note(out, error),
    }
}

fn worktree_create(
    invocation: &ClaudeHookInvocation,
    name: &str,
    spool: Option<&HostEventSpool>,
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
    if let Some(parent) = target.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return fail(out, format!("cannot create {}: {error}", parent.display()));
    }
    // No core task context exists in W2 (HI-W4-1 supplies it): the no-task
    // Workspace path is the fallback. Workspace refusals (conflicting path,
    // other branch, task-bound worktree) are returned as-is, never retried
    // through another path.
    let response = WorktreeCreateRequest::new(&root, &target, &branch, &head, None)
        .and_then(|request| request.create());
    let response = match response {
        Ok(response) => response,
        Err(error) => return fail(out, error.to_string()),
    };
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
        (Some(spool), Some(event)) => emit(spool, &event, out),
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
    let meta = fs::symlink_metadata(path).map_err(|_| format!("{} unreadable", path.display()))?;
    if !meta.is_file() || meta.len() > SMALL_FILE_MAX_BYTES {
        return Err(format!("{} is not a small regular file", path.display()));
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|f| f.take(SMALL_FILE_MAX_BYTES).read_to_string(&mut text))
        .map_err(|_| format!("{} unreadable", path.display()))?;
    Ok(text)
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
/// selected report (0600, atomic rename) or removes a report that no longer
/// holds; never writes host settings.
fn record_capabilities(
    invocation: &ClaudeHookInvocation,
    ctx: &ClaudeHookContext,
) -> Result<HostCapabilityFreshnessDisposition, String> {
    let (Some(exe), Some(data), Some(project)) =
        (&ctx.claude_executable, &ctx.plugin_data, &ctx.project_dir)
    else {
        return Err("capability report not recorded (no RECEIPTS_CLAUDE_EXECUTABLE, CLAUDE_PLUGIN_DATA or CLAUDE_PROJECT_DIR)".into());
    };
    if !data.is_absolute() {
        return Err("CLAUDE_PLUGIN_DATA is not absolute".into());
    }
    let env = ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription)
        .map_err(|_| "probe environment policy unavailable".to_owned())?;
    let runner = WorkspaceHostProbeRunner::new(exe, env)
        .map_err(|e| format!("claude executable not admitted: {e:?}"))?;
    let project = fs::canonicalize(project).map_err(|_| "CLAUDE_PROJECT_DIR unresolvable")?;
    let path = data.join(CAPABILITY_REPORT_FILE);
    let cached = small_text(&path)
        .ok()
        .and_then(|text| host_capability_report_from_json(&text).ok());
    let hook_execution = ctx
        .plugin_root
        .as_deref()
        .and_then(|root| invocation.execution_evidence(root));
    let spool = HostEventSpool::for_project(&project).map_err(|e| e.to_string())?;
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
            project_root: project,
            mode_override: None,
            cached: cached.as_ref().map(HostCapabilityReport::core),
            hook_execution,
        },
        &runner,
    )
    .map_err(|e| e.to_string())?;
    let entry = outcome.capability_refresh.cache_entry.as_ref();
    let Some(entry) = entry else {
        let _ = fs::remove_file(&path);
        return Err("capability observation incomplete; cached report removed".into());
    };
    let now = now_utc();
    let probed_at = match (&cached, outcome.freshness_disposition) {
        (Some(cached), HostCapabilityFreshnessDisposition::ReuseReport) => {
            cached.probed_at().clone()
        }
        _ => now.clone(),
    };
    let report = HostCapabilityReport::new(entry.report().clone(), probed_at, now);
    write_private_atomically(data, &path, &host_capability_report_to_json(&report))?;
    Ok(outcome.freshness_disposition)
}

fn write_private_atomically(dir: &Path, path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|_| "cannot create plugin data directory")?;
    let temp = dir.join(format!(".{CAPABILITY_REPORT_FILE}.{}", std::process::id()));
    let _ = fs::remove_file(&temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|_| "cannot create capability report")?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_data())
        .map_err(|_| "cannot write capability report")?;
    fs::rename(&temp, path).map_err(|_| "cannot install capability report".into())
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
