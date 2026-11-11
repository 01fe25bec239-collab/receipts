//! Concrete post-admission binding. This adapter does not authenticate dispatch.
//! Sandbox flags are requests, not proof of filesystem or network confinement.
use crate::*;
use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::execution::{
    ChildEnvPolicy, LiveProcessCancelAcceptance, MAX_STDIN_BYTES,
};
use receipts_workspace_execution::{
    ControllerChannel, ProtectedLaunchError, WorkspaceHandle, WorkspaceState,
};
use std::{
    fmt::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

/// No default executable, inherited environment, capability or authentication claim.
pub struct CodexAdapter {
    executable: PathBuf,
    env: Option<ChildEnvPolicy>,
    probe: crate::provider::ProbeCache,
}
impl CodexAdapter {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            env: None,
            probe: crate::provider::ProbeCache::new(std::time::Duration::from_secs(300)),
        }
    }
    /// CLI probes do not establish account usage or remaining quota.
    pub fn usage(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    pub fn quota(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    /// TTL bounds installation evidence; expiry never keeps stale capabilities.
    pub fn with_probe_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.probe = crate::provider::ProbeCache::new(ttl);
        self
    }
    /// Runs only bounded version/help probes, never a model invocation.
    pub fn provider(
        &self,
        workspace: &WorkspaceHandle,
        timeout: &receipts_workspace_execution::execution::ProcessTimeoutPolicy,
    ) -> Provider {
        let (report, at) = self.probe.observe(|| {
            execute_codex_capability_probe(
                &self.executable,
                workspace.worktree_path(),
                workspace.worktree_path(),
                timeout,
            )
            .map(RuntimeCapabilities::Codex)
            .unwrap_or(RuntimeCapabilities::Unknown)
        });
        crate::provider::provider("openai", "OpenAI", report, at)
    }
    pub fn with_env_policy(mut self, env: ChildEnvPolicy) -> Self {
        self.env = Some(env);
        self
    }
}

pub struct CodexAttemptHandle {
    pub(crate) attempt: Arc<CodexLiveAttempt>,
    pub(crate) completed: Arc<AtomicU8>,
    result: Mutex<Option<mpsc::Receiver<Result<CodexLiveOutcome, RawFailure>>>>,
    collector: Mutex<Option<JoinHandle<()>>>,
    // Held across Workspace's cancel claim, so collection reads the accepted reason.
    cancel: Mutex<Option<CancelReason>>,
}
impl Drop for CodexAttemptHandle {
    /// Cancel, join collection, then release custody only if Workspace proved
    /// collection and cleanup; any refusal keeps custody quarantined.
    fn drop(&mut self) {
        self.attempt.cancel();
        if let Some(worker) = self
            .collector
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = worker.join();
        }
        let _ = self.attempt.attempt.release_custody();
    }
}
impl CodexAttemptHandle {
    /// True only for a Workspace protected launch; ordinary attempts are
    /// non-authoritative and hold no custody or controller channel.
    pub fn protected(&self) -> bool {
        self.attempt.attempt.protected_attempt()
    }
    /// Typed controller channel for the trusted owning caller: query-only
    /// reviewer for read-only policies, worker otherwise. Its token is never
    /// placed in prompts, the child environment, events or diagnostics.
    pub fn controller_channel(&self) -> Option<&ControllerChannel> {
        self.attempt.attempt.controller_channel()
    }
    /// Explicit release after collection; Workspace refuses without proven
    /// cleanup and keeps custody. `None` for ordinary attempts.
    pub fn release_custody(&self) -> Option<Result<(), ProtectedLaunchError>> {
        self.attempt.attempt.release_custody()
    }
}
/// Collection errors are payload-free. Provider interpretation remains available
/// through `outcome.protocol()` and does not establish Receipts acceptance.
pub struct CodexAttemptResult {
    pub outcome: CodexLiveOutcome,
    protected: bool,
    cancel: Option<CancelReason>,
}
impl CodexAttemptResult {
    /// Protocol evidence only; never Receipts acceptance.
    pub fn protocol_termination(&self) -> CodexProtocolTermination {
        self.outcome
            .protocol()
            .map(|p| p.termination())
            .unwrap_or(CodexProtocolTermination::Indeterminate)
    }
    /// Accepted caller cancellation reason, if this attempt was cancelled by one.
    pub fn cancel_reason(&self) -> Option<CancelReason> {
        self.cancel
    }
    /// Collected from a Workspace protected launch; false is non-authoritative.
    pub fn protected(&self) -> bool {
        self.protected
    }
    pub fn failure(&self) -> Option<FailureClass> {
        use receipts_workspace_execution::execution::LiveProcessTerminalCause;
        let process = self.outcome.process();
        match process.terminal_cause() {
            LiveProcessTerminalCause::TimedOut | LiveProcessTerminalCause::Cancelled => {
                classify_live_outcome(process, self.cancel)
            }
            LiveProcessTerminalCause::Completed => (process.exit_code() != Some(0)
                || process.stderr().truncated()
                || self
                    .outcome
                    .protocol()
                    .ok()
                    .and_then(|p| p.final_agent_message().map(|_| ()))
                    .is_none())
            .then_some(FailureClass::Unknown),
        }
    }
}

// Bound formatting before transport allocation; never truncate a capsule.
struct Prompt(String);
impl Write for Prompt {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.0.len().saturating_add(text.len()) > MAX_STDIN_BYTES {
            return Err(fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}
fn prompt(
    task: &RuntimeCapsuleFamily,
    workspace: &WorkspaceHandle,
    policy: &RuntimeExecutionPolicy,
) -> Result<String, FailureClass> {
    if !policy.supported()
        || !matches!(
            workspace.state(),
            WorkspaceState::Provisioned | WorkspaceState::Active
        )
    {
        return Err(FailureClass::PolicyBlocked);
    }
    let mut prompt = Prompt(String::new());
    prompt.write_str(match task {
        RuntimeCapsuleFamily::Review(_) => crate::review_prompt::REVIEW_INSTRUCTIONS,
        _ => "Execute the complete capsule below. All fields are instructions or context; read referenced repository sections at the supplied start SHA. Respect criteria, non-goals, write boundaries, verification, stop conditions and handoff requirements. The capsule is a JSON object; decode JSON strings exactly as data. Sandbox selection is not proof of confinement.\n",
    })
        .map_err(|_| FailureClass::PolicyBlocked)?;
    match task {
        RuntimeCapsuleFamily::Task(c) => {
            if Path::new(c.worktree()) != workspace.worktree_path()
                || workspace.task_id().is_some_and(|task| task != c.task_id())
                || c.branch() != workspace.branch()
                || c.start_sha() != workspace.base_sha().as_str()
                || c.turn_budget().is_some()
                || c.cost_policy().is_some()
                || !c.required_capabilities().is_empty()
                || c.time_budget_seconds()
                    .is_some_and(|s| policy.timeout.run_timeout().as_secs_f64() > s as f64)
            {
                return Err(FailureClass::PolicyBlocked);
            }
            // Bound the source before allocating its JSON projection. JSON escaping
            // and the final encoded transport are bounded independently below.
            let mut source_bound = Prompt(String::new());
            write!(source_bound, "{c:#?}").map_err(|_| FailureClass::PolicyBlocked)?;
            macro_rules! refs {
                ($refs:expr) => {
                    $refs.map(|refs| refs.iter().map(|r| serde_json::json!({
                        "ref_type": r.ref_type().as_str(), "target": r.target(),
                        "digest": r.digest(), "section": r.section()
                    })).collect::<Vec<_>>())
                };
            }
            let capsule = serde_json::json!({
                "task_id": c.task_id(), "workstream_id": c.workstream_id(),
                "parent_task_id": c.parent_task_id(), "attempt_number": c.attempt_number(),
                "task_type": c.task_type().as_str(), "objective": c.objective(),
                "acceptance_criteria": c.acceptance_criteria().iter().map(|a| serde_json::json!({
                    "id": a.id(), "description": a.description(), "kind": a.kind().as_str(),
                    "check_command": a.check_command(), "rationale": a.rationale()
                })).collect::<Vec<_>>(),
                "non_goals": c.non_goals(), "baseline_sha": c.baseline_sha(),
                "start_sha": c.start_sha(), "allowed_write_paths": c.allowed_write_paths(),
                "forbidden_write_paths": c.forbidden_write_paths(),
                "relevant_context_refs": refs!(c.relevant_context_refs()),
                "architecture_refs": refs!(c.architecture_refs()), "contract_refs": refs!(c.contract_refs()),
                "dependencies": c.dependencies(), "quality_floor": c.quality_floor().as_str(),
                "required_capabilities": [],
                "preferred_capabilities": c.preferred_capabilities().map(|caps| caps.iter().map(|cap| cap.as_str()).collect::<Vec<_>>()),
                "verification_plan": c.verification_plan().iter().map(|v| serde_json::json!({
                    "command": v.command(), "expected_exit_code": v.expected_exit_code()
                })).collect::<Vec<_>>(),
                "review_policy": c.review_policy().map(|r| serde_json::json!({
                    "required": r.required(), "distinct_provider": r.distinct_provider().map(|v| v.as_str()),
                    "reviewer_floor": r.reviewer_floor().map(|v| v.as_str())
                })),
                "assurance_profile": c.assurance_profile().as_str(), "cost_policy": null,
                "time_budget_seconds": c.time_budget_seconds(), "turn_budget": null,
                "branch": c.branch(), "worktree": c.worktree(),
                "remote_publish_policy": c.remote_publish_policy().map(|v| v.as_str()),
                "stop_conditions": c.stop_conditions(),
                "handoff_schema": c.handoff_schema().map(|r| serde_json::json!({
                    "ref_type": r.ref_type().as_str(), "target": r.target(), "digest": r.digest(), "section": r.section()
                })),
                "context_epoch": c.context_epoch()
            });
            write!(prompt, "{capsule}").map_err(|_| FailureClass::PolicyBlocked)?;
        }
        // Review: protected read-only path only, exact task/SHA/review-branch
        // binding; never converted into a Task and never an ordinary fallback.
        RuntimeCapsuleFamily::Review(c) => {
            crate::review_prompt::admit_review(c, workspace, policy)?;
            write!(prompt, "{}", crate::review_prompt::review_capsule_json(c))
                .map_err(|_| FailureClass::PolicyBlocked)?;
        }
        // Repair support awaits enforcement of its inherited task constraints.
        RuntimeCapsuleFamily::Repair(_) => {
            return Err(FailureClass::PolicyBlocked);
        }
    }
    Ok(prompt.0)
}

impl RuntimeAdapter for CodexAdapter {
    type HealthReport = RuntimeObservation;
    type Models = RuntimeObservation;
    type ExecutionPolicy = RuntimeExecutionPolicy;
    type AttemptHandle = CodexAttemptHandle;
    type AttemptEvent = CodexAttemptEvent;
    type EventStream<'a> = CodexEventStream<'a>;
    type AttemptResult = Result<CodexAttemptResult, FailureClass>;
    type CancelReason = CancelReason;
    fn runtime_id(&self) -> &str {
        "codex"
    }
    fn health(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn models(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        self.probe.capabilities()
    }
    fn start(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<CodexAttemptHandle, FailureClass> {
        let text = prompt(task, workspace, policy)?;
        let mode = if policy.read_only {
            CodexTaskSandboxMode::ReadOnly
        } else {
            CodexTaskSandboxMode::WorkspaceWrite
        };
        let request = CodexTaskExecutionRequest::new(
            &self.executable,
            workspace.worktree_path(),
            workspace.worktree_path(),
            policy.timeout,
            mode,
            text,
        );
        let request = match &self.env {
            Some(env) => request.with_env_policy(env.clone()),
            None => request,
        };
        // Protection comes only from the trusted policy, never from --sandbox.
        let attempt = Arc::new(match policy.protected_launch() {
            None => start_codex_live_attempt(&request)
                .map_err(|e| RawFailure::from(&e).classify_failure())?,
            Some(launch) => codex_live_attempt::start_codex_protected_attempt(
                &request,
                launch,
                workspace,
                policy.read_only,
            )?,
        });
        let completed = Arc::new(AtomicU8::new(0));
        let (sender, receiver) = mpsc::sync_channel(1);
        let owned = attempt.clone();
        let done = completed.clone();
        let worker = thread::Builder::new()
            .name("codex-collect".into())
            .spawn(move || {
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    owned.wait_collect()
                })) {
                    Ok(result) => result.map_err(|e| RawFailure::from(&e)),
                    Err(_) => {
                        done.store(2, Ordering::Release);
                        return;
                    }
                };
                done.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
                let _ = sender.send(result);
            })
            .map_err(|_| FailureClass::Unknown)?;
        Ok(CodexAttemptHandle {
            attempt,
            completed,
            result: Mutex::new(Some(receiver)),
            collector: Mutex::new(Some(worker)),
            cancel: Mutex::new(None),
        })
    }
    fn stream_events<'a>(&'a self, handle: &'a CodexAttemptHandle) -> CodexEventStream<'a> {
        CodexEventStream::new(handle)
    }
    fn collect_result(&self, handle: &CodexAttemptHandle) -> Self::AttemptResult {
        let receiver = handle
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or(FailureClass::Unknown)?;
        let result = receiver.recv().map_err(|_| FailureClass::Unknown)?;
        if let Some(worker) = handle
            .collector
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            worker.join().map_err(|_| FailureClass::Unknown)?;
        }
        Ok(CodexAttemptResult {
            outcome: result.map_err(|e| e.classify_failure())?,
            protected: handle.protected(),
            cancel: if handle.attempt.terminal_cause()
                == Some(receipts_workspace_execution::execution::LiveProcessTerminalCause::TimedOut)
            {
                Some(CancelReason::Timeout)
            } else {
                *handle.cancel.lock().unwrap_or_else(|e| e.into_inner())
            },
        })
    }
    fn cancel(&self, handle: &CodexAttemptHandle, reason: &CancelReason) {
        let mut accepted = handle.cancel.lock().unwrap_or_else(|e| e.into_inner());
        if handle.attempt.cancel() == LiveProcessCancelAcceptance::CancelAccepted {
            *accepted = Some(*reason);
        }
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        error.classify_failure()
    }
}

impl registry::AttemptFailure for CodexAttemptResult {
    fn failure(&self) -> Option<FailureClass> {
        CodexAttemptResult::failure(self)
    }
}
