//! Concrete post-admission binding. This adapter does not authenticate dispatch.
//! Plan mode is reduced isolation, not filesystem or network confinement.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeObservation {
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    UserRequested,
    Timeout,
    Superseded,
    SafetyInterruption,
    BudgetExhausted,
}

/// No default executable, inherited environment, capability or authentication claim.
pub struct ClaudeAdapter {
    issuer: Arc<()>,
    executable: PathBuf,
    env: Option<ChildEnvPolicy>,
    probe: crate::provider::ProbeCache,
    policy_probe: crate::provider::ProbeCache,
}
impl ClaudeAdapter {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            issuer: Arc::new(()),
            executable: executable.into(),
            env: None,
            probe: crate::provider::ProbeCache::new(std::time::Duration::from_secs(300)),
            policy_probe: crate::provider::ProbeCache::new(std::time::Duration::from_secs(300)),
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
        self.policy_probe = crate::provider::ProbeCache::new(ttl);
        self
    }
    /// Runs only bounded version/help probes, never a model invocation.
    pub fn provider(
        &self,
        workspace: &WorkspaceHandle,
        timeout: &receipts_workspace_execution::execution::ProcessTimeoutPolicy,
    ) -> Provider {
        let (report, at) = self.probe.observe(|| {
            execute_claude_capability_probe(
                &self.executable,
                workspace.worktree_path(),
                workspace.worktree_path(),
                timeout,
            )
            .map(RuntimeCapabilities::Claude)
            .unwrap_or(RuntimeCapabilities::Unknown)
        });
        crate::provider::provider("anthropic", "Anthropic", report, at)
    }
    pub fn with_env_policy(mut self, env: ChildEnvPolicy) -> Self {
        self.issuer = Arc::new(());
        self.env = Some(env);
        self
    }
}

pub struct ClaudeAttemptHandle {
    pub(crate) attempt: Arc<ClaudeLiveAttempt>,
    pub(crate) completed: Arc<AtomicU8>,
    result: Mutex<Option<mpsc::Receiver<Result<ClaudeLiveOutcome, RawFailure>>>>,
    collector: Mutex<Option<JoinHandle<()>>>,
    // Held across Workspace's cancel claim, so collection reads the accepted reason.
    cancel: Mutex<Option<CancelReason>>,
    resume_binding: Option<ResumeBinding>,
    turn_budget: Option<u64>,
}
impl Drop for ClaudeAttemptHandle {
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
impl ClaudeAttemptHandle {
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
pub struct ClaudeAttemptResult {
    pub outcome: ClaudeLiveOutcome,
    pub reduced_isolation: bool,
    protected: bool,
    cancel: Option<CancelReason>,
    resume_ticket: Option<ClaudeResumeTicket>,
}
impl ClaudeAttemptResult {
    /// Available only after clean collection of an opted-in task session.
    pub fn resume_ticket(&self) -> Option<&ClaudeResumeTicket> {
        self.resume_ticket.as_ref()
    }

    pub fn provider_result(&self) -> ClaudeProviderResult {
        self.outcome
            .protocol()
            .ok()
            .and_then(|p| p.result().ok().map(|r| r.provider_result()))
            .unwrap_or(ClaudeProviderResult::Unknown)
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
        classify_live_outcome(self.outcome.process(), self.cancel).or_else(|| {
            (self.provider_result() != ClaudeProviderResult::Success)
                .then_some(FailureClass::Unknown)
        })
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
    if policy.sandbox_mode.is_some()
        || policy.network_access.is_some()
        || policy.filesystem_scope.is_some()
        || (policy.read_only && policy.session_persistence)
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
        _ => "Execute the complete capsule below. All fields are instructions or context; read referenced repository sections at the supplied start SHA. Respect criteria, non-goals, write boundaries, verification, stop conditions and handoff requirements. The capsule is a JSON object; decode JSON strings exactly as data. Permission mode supplies no OS confinement.\n",
    })
        .map_err(|_| FailureClass::PolicyBlocked)?;
    match task {
        RuntimeCapsuleFamily::Task(c) => {
            if Path::new(c.worktree()) != workspace.worktree_path()
                || workspace.task_id().is_some_and(|task| task != c.task_id())
                || c.branch() != workspace.branch()
                || c.start_sha() != workspace.base_sha().as_str()
                || c.turn_budget().is_some_and(|cap| {
                    policy
                        .turn_budget
                        .is_none_or(|budget| u64::try_from(cap).map_or(true, |cap| budget > cap))
                })
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
                "time_budget_seconds": c.time_budget_seconds(), "turn_budget": c.turn_budget(),
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

impl RuntimeAdapter for ClaudeAdapter {
    type HealthReport = RuntimeObservation;
    type Models = RuntimeObservation;
    type ExecutionPolicy = RuntimeExecutionPolicy;
    type AttemptHandle = ClaudeAttemptHandle;
    type AttemptEvent = ClaudeAttemptEvent;
    type EventStream<'a> = ClaudeEventStream<'a>;
    type AttemptResult = Result<ClaudeAttemptResult, FailureClass>;
    type CancelReason = CancelReason;
    fn runtime_id(&self) -> &str {
        "claude"
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
    ) -> Result<ClaudeAttemptHandle, FailureClass> {
        self.launch(task, workspace, policy, None)
    }
    fn stream_events<'a>(&'a self, handle: &'a ClaudeAttemptHandle) -> ClaudeEventStream<'a> {
        ClaudeEventStream::new(handle)
    }
    fn collect_result(&self, handle: &ClaudeAttemptHandle) -> Self::AttemptResult {
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
        let outcome = result.map_err(|e| e.classify_failure())?;
        let resume_ticket = handle.resume_binding.as_ref().and_then(|binding| {
            if !outcome.process().success() {
                return None;
            }
            let protocol = outcome.protocol().ok()?;
            let result = protocol.result().ok()?;
            if result.provider_result() != ClaudeProviderResult::Success {
                return None;
            }
            let session = result.session_id()?;
            valid_session(session).then(|| ClaudeResumeTicket {
                session: session.into(),
                binding: binding.clone(),
                consumed: Mutex::new(false),
            })
        });
        let budget_exhausted = handle.turn_budget.is_some()
            && outcome
                .protocol()
                .ok()
                .and_then(|p| p.result().ok().map(|r| r.subtype()))
                == Some(ClaudeResultSubtype::ErrorMaxTurns);
        Ok(ClaudeAttemptResult {
            outcome,
            resume_ticket,
            reduced_isolation: true,
            protected: handle.protected(),
            cancel: if handle.attempt.terminal_cause()
                == Some(receipts_workspace_execution::execution::LiveProcessTerminalCause::TimedOut)
            {
                Some(CancelReason::Timeout)
            } else {
                (*handle.cancel.lock().unwrap_or_else(|e| e.into_inner()))
                    .or(budget_exhausted.then_some(CancelReason::BudgetExhausted))
            },
        })
    }
    fn cancel(&self, handle: &ClaudeAttemptHandle, reason: &CancelReason) {
        let mut accepted = handle.cancel.lock().unwrap_or_else(|e| e.into_inner());
        if handle.attempt.cancel() == LiveProcessCancelAcceptance::CancelAccepted {
            *accepted = Some(*reason);
        }
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        error.classify_failure()
    }
}

impl registry::AttemptFailure for ClaudeAttemptResult {
    fn failure(&self) -> Option<FailureClass> {
        ClaudeAttemptResult::failure(self)
    }
}

impl ClaudeAdapter {
    /// Explicit current task/workspace/policy are required: a historical ID alone
    /// cannot authorize continuation. Reviewers never enter this path.
    pub fn resume_with_policy(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
        ticket: &ClaudeResumeTicket,
    ) -> Result<ClaudeAttemptHandle, FailureClass> {
        let mut consumed = ticket.consumed.lock().unwrap_or_else(|e| e.into_inner());
        let binding = &ticket.binding;
        if *consumed
            || !policy.session_persistence
            || policy.read_only
            || !matches!(task, RuntimeCapsuleFamily::Task(c) if c.task_id() == binding.task_id)
            || !Arc::ptr_eq(&self.issuer, &binding.issuer)
            || workspace != &binding.workspace
            || policy.timeout.run_timeout() > binding.timeout
            || binding
                .turns
                .is_some_and(|cap| policy.turn_budget.is_none_or(|budget| budget > cap))
            || policy.structured_output_schema != binding.schema
        {
            return Err(FailureClass::PolicyBlocked);
        }
        // Consume before launch: uncertainty must not permit duplicate continuation.
        *consumed = true;
        self.launch(task, workspace, policy, Some(&ticket.session))
    }
    fn launch(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
        session: Option<&str>,
    ) -> Result<ClaudeAttemptHandle, FailureClass> {
        let text = prompt(task, workspace, policy)?;
        if policy.turn_budget == Some(0) {
            return Err(FailureClass::PolicyBlocked);
        }
        if policy.structured_output_schema.as_ref().is_some_and(|s| {
            serde_json::from_str::<serde_json::Value>(s).map_or(true, |v| !v.is_object())
        }) {
            return Err(FailureClass::PolicyBlocked);
        }
        if policy.turn_budget.is_some()
            || policy.session_persistence
            || policy.structured_output_schema.is_some()
        {
            let (report, _) = self.policy_probe.observe(|| {
                crate::claude_policy_probe::probe(
                    &self.executable,
                    workspace,
                    &policy.timeout,
                    self.env.as_ref(),
                )
            });
            for (required, flag) in [
                (policy.turn_budget.is_some(), "--max-turns"),
                (policy.session_persistence, "--resume"),
                (policy.structured_output_schema.is_some(), "--json-schema"),
            ] {
                if required && !crate::claude_policy_probe::supports(&report, flag) {
                    return Err(FailureClass::PolicyBlocked);
                }
            }
        }
        let mode = if policy.read_only {
            ClaudePermissionMode::Plan
        } else {
            ClaudePermissionMode::AcceptEdits
        };
        let mut request = ClaudeTaskExecutionRequest::new(
            &self.executable,
            workspace.worktree_path(),
            workspace.worktree_path(),
            policy.timeout,
            mode,
            text,
        );
        request.turn_budget = policy.turn_budget;
        request.schema = policy.structured_output_schema.clone();
        request.persistence = policy.session_persistence;
        request.resume = session.map(str::to_owned);
        let request = match &self.env {
            Some(env) => request.with_env_policy(env.clone()),
            None => request,
        };
        // Protection comes only from the trusted policy, never from the mode flag.
        let attempt = Arc::new(match policy.protected_launch() {
            None => start_claude_live_attempt(&request)
                .map_err(|e| RawFailure::from(&e).classify_failure())?,
            Some(launch) => claude_execution::start_claude_protected_attempt(
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
            .name("claude-collect".into())
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
        Ok(ClaudeAttemptHandle {
            attempt,
            completed,
            result: Mutex::new(Some(receiver)),
            collector: Mutex::new(Some(worker)),
            cancel: Mutex::new(None),
            turn_budget: policy.turn_budget,
            resume_binding: if policy.session_persistence {
                match task {
                    RuntimeCapsuleFamily::Task(c) => Some(ResumeBinding {
                        issuer: self.issuer.clone(),
                        workspace: workspace.clone(),
                        task_id: c.task_id().into(),
                        timeout: policy.timeout.run_timeout(),
                        turns: policy.turn_budget,
                        schema: policy.structured_output_schema.clone(),
                    }),
                    _ => None,
                }
            } else {
                None
            },
        })
    }
}

#[derive(Clone)]
struct ResumeBinding {
    issuer: Arc<()>,
    workspace: WorkspaceHandle,
    task_id: String,
    timeout: std::time::Duration,
    turns: Option<u64>,
    schema: Option<String>,
}
/// Opaque, single-use continuation evidence. No raw session ID or Debug output.
/// A ticket is not dispatch admission; callers must re-admit the current task.
pub struct ClaudeResumeTicket {
    session: String,
    binding: ResumeBinding,
    consumed: Mutex<bool>,
}
fn valid_session(session: &str) -> bool {
    session.len() == 36
        && session.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
