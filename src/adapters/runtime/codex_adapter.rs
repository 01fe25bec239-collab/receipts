//! Concrete post-admission binding. This adapter does not authenticate dispatch.
//! Sandbox flags are requests, not proof of filesystem or network confinement.
use crate::*;
use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::execution::{ChildEnvPolicy, MAX_STDIN_BYTES};
use receipts_workspace_execution::{WorkspaceHandle, WorkspaceState};
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
}
impl CodexAdapter {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            env: None,
        }
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
}
impl Drop for CodexAttemptHandle {
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
    }
}
/// Collection errors are payload-free. Provider interpretation remains available
/// through `outcome.protocol()` and does not establish Receipts acceptance.
pub struct CodexAttemptResult {
    pub outcome: CodexLiveOutcome,
}
impl CodexAttemptResult {
    /// Protocol evidence only; never Receipts acceptance.
    pub fn protocol_termination(&self) -> CodexProtocolTermination {
        self.outcome
            .protocol()
            .map(|p| p.termination())
            .unwrap_or(CodexProtocolTermination::Indeterminate)
    }
    pub fn failure(&self) -> Option<FailureClass> {
        use receipts_workspace_execution::execution::LiveProcessTerminalCause;
        let process = self.outcome.process();
        match process.terminal_cause() {
            LiveProcessTerminalCause::TimedOut => Some(FailureClass::Timeout),
            LiveProcessTerminalCause::Cancelled => Some(FailureClass::Unknown),
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
    prompt.write_str("Execute the complete capsule below. All fields are instructions or context; read referenced repository sections at the supplied start SHA. Respect criteria, non-goals, write boundaries, verification, stop conditions and handoff requirements. The capsule is a JSON object; decode JSON strings exactly as data. Sandbox selection is not proof of confinement.\n")
        .map_err(|_| FailureClass::PolicyBlocked)?;
    match task {
        RuntimeCapsuleFamily::Task(c) => {
            if Path::new(c.worktree()) != workspace.worktree_path()
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
        // Complete Review wire/temporal context is not available in this binding.
        // Repair support awaits enforcement of its inherited task constraints.
        RuntimeCapsuleFamily::Review(_) | RuntimeCapsuleFamily::Repair(_) => {
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
        RuntimeCapabilities::Unknown
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
        let attempt = Arc::new(
            start_codex_live_attempt(&request)
                .map_err(|e| RawFailure::from(&e).classify_failure())?,
        );
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
        })
    }
    fn cancel(&self, handle: &CodexAttemptHandle, _: &CancelReason) {
        handle.attempt.cancel();
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        error.classify_failure()
    }
}
