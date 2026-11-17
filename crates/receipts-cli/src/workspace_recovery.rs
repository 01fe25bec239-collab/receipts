//! Trusted D4 composition. The supervisor owns the original process, Workspace
//! pins and exclusive Core writer. Socket credentials select this owner, never
//! a caller-provided PID or reset SHA. Same-user hostile code is outside the
//! existing in-process/custody boundary; deploy workers under a separate identity.
use receipts_orchestration::orchestration::*;
use receipts_review_integration::{
    ReviewClaim, ReviewClaimBinding, ReviewRecordProposal, ReviewRecordSink, SEALED_REVIEW_KIND,
    SealedReview,
};
use receipts_state::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn error(e: impl ToString) -> io::Error {
    io::Error::other(e.to_string())
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn unit(binding: &ReviewClaimBinding) -> Result<CoreReviewUnitV1, String> {
    let s = binding.scope();
    CoreReviewUnitV1::new(
        s.project_id(),
        s.task_id(),
        s.subject_sha(),
        s.request_sha256(),
        s.generation(),
    )
    .map_err(|e| format!("{e:?}"))
}
struct Content(ReviewRecordProposal);
impl CoreContentResolverV1 for Content {
    fn resolve(&self, project: &str, reference: &str) -> Option<Vec<u8>> {
        (project == self.0.project_id)
            .then(|| {
                self.0
                    .content
                    .iter()
                    .find(|(r, _)| r == reference)
                    .map(|(_, b)| b.clone())
            })
            .flatten()
    }
}
struct Codec(ReviewClaimBinding);
impl CoreRecoveryCodecV1 for Codec {
    fn kind(&self) -> &str {
        SEALED_REVIEW_KIND
    }
    fn decode(&self, project: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String> {
        let sealed = SealedReview::from_bytes(bytes)?;
        let p = sealed.proposal()?;
        if p.binding() != &self.0 || p.project_id != project || p.events.len() != p.event_keys.len()
        {
            return Err("foreign sealed review binding".into());
        }
        Ok(CoreRecoveredReviewV1 {
            input: CorePublicationInputV1 {
                project_id: p.project_id,
                records: p.decision_records,
                events: p
                    .events
                    .into_iter()
                    .zip(p.event_keys)
                    .map(|(e, k)| CoreEventV1::new(e, k))
                    .collect(),
                availability: vec![],
            },
            settlement: sealed.settlement()?,
        })
    }
}
/// Production adapter over the same public ports exercised by CoreReviewSink.
/// Publication always rebuilds State's seal, including after controller restart.
struct PublicationSink<'a> {
    writer: &'a mut CoreStateWriter,
    clock: &'a dyn TrustedClockV1,
}
impl ReviewRecordSink for PublicationSink<'_> {
    fn claim(&mut self, b: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
        match self
            .writer
            .claim_review(self.clock, &unit(b)?)
            .map_err(|e| e.to_string())?
        {
            CoreReviewClaimV1::Acquired => Ok(ReviewClaim::Acquired),
            CoreReviewClaimV1::Sealed(s) => {
                if s.seal.recovery_kind != SEALED_REVIEW_KIND {
                    return Err("foreign seal kind".into());
                }
                let review = SealedReview::from_bytes(&s.seal.recovery_data)?;
                if review.binding()? != *b {
                    return Err("foreign seal scope".into());
                }
                Ok(ReviewClaim::Sealed(review))
            }
            CoreReviewClaimV1::InFlight(_) => Ok(ReviewClaim::InFlight),
            CoreReviewClaimV1::Terminal(r) => {
                Ok(ReviewClaim::Terminal(r.outcome_kind.unwrap_or_default()))
            }
            CoreReviewClaimV1::Abandoned(_) => Ok(ReviewClaim::Abandoned),
        }
    }
    fn seal(&mut self, b: &ReviewClaimBinding, s: &SealedReview) -> Result<(), String> {
        let p = s.proposal()?;
        if p.binding() != b {
            return Err("foreign seal proposal".into());
        }
        self.writer
            .seal_review(
                self.clock,
                &Content(p),
                &Codec(b.clone()),
                &unit(b)?,
                s.to_bytes(),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn publish(&mut self, b: &ReviewClaimBinding, p: &ReviewRecordProposal) -> Result<(), String> {
        let u = unit(b)?;
        let stored = self
            .writer
            .sealed_review(&u)
            .map_err(|e| e.to_string())?
            .ok_or("missing durable seal")?;
        if stored.seal.recovery_kind != SEALED_REVIEW_KIND
            || SealedReview::from_bytes(&stored.seal.recovery_data)?.proposal()? != *p
        {
            return Err("proposal differs from durable seal".into());
        }
        self.writer
            .publish_sealed_review(self.clock, &Content(p.clone()), &Codec(b.clone()), &u)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Explicit admitted source/compiler/policy; not inferred from worker output.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorConfig {
    pub state: PathBuf,
    pub repository: PathBuf,
    pub worktree: PathBuf,
    pub project: String,
    pub graph: String,
    pub task: String,
    pub workspace: String,
    pub branch: String,
    pub base: String,
    pub compiler: String,
    pub policy: String,
    pub source: String,
    pub role: String,
    pub allowed_write_paths: Vec<String>,
    pub forbidden_write_paths: Vec<String>,
}
impl SupervisorConfig {
    fn identity(&self) -> io::Result<CoreAdmissionIdentity> {
        Ok(CoreAdmissionIdentity::new(
            GraphCompilerRefV1::parse(&self.compiler).map_err(error)?,
            self.policy.clone(),
        )
        .ok_or_else(|| error("invalid admitted policy"))?
        .with_project(
            &self.project,
            GraphSourceEvidenceRefV1::parse(&self.source).map_err(error)?,
        ))
    }
}
/// Canonical system time, with a stable source contract. Existing stores with
/// another clock contract refuse rather than silently rebinding their watermark.
pub struct SupervisorClock;
impl SupervisorClock {
    pub fn at(time: SystemTime) -> io::Result<String> {
        let elapsed = time.duration_since(UNIX_EPOCH).map_err(error)?;
        let text = receipts_host_integration::claude::utc_from_unix_millis(
            i64::try_from(elapsed.as_millis()).map_err(error)?,
        )
        .as_str()
        .to_owned();
        Ok(format!(
            "{}000000Z",
            text.strip_suffix('Z')
                .ok_or_else(|| error("invalid clock"))?
        ))
    }
}
impl TrustedClockV1 for SupervisorClock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: Self::at(SystemTime::now()).map_err(|e| {
                StateError::InternalQueryFailed {
                    detail: e.to_string(),
                }
            })?,
            clock_source_id: "receipts-supervisor-system-clock-v1".into(),
            clock_contract_version: "1".into(),
        })
    }
}

#[cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod platform {
    use super::*;

    use receipts_review_integration::{ReviewClaimScope, ReviewOperation};
    use receipts_workspace_execution::{
        WorkspaceCheckpointKind, WorkspaceDateTimeV1, WorkspaceProvisionRequest,
        execution::*,
        recovery_custody::{RecoveryCaptureRequest, RecoveryCheckpoint, WorkspaceRecoveryCustody},
    };
    use serde_json::{Value, json};
    use std::{
        os::unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
        },
        process::{Child, Command, Stdio},
        sync::mpsc::Receiver,
    };
    const MAX_FRAME: usize = 2 * 1024 * 1024;
    const IO_WAIT: Duration = Duration::from_millis(200);
    fn now() -> io::Result<String> {
        Ok(SupervisorClock
            .sample()
            .map_err(error)?
            .canonical_utc_timestamp)
    }
    fn future(ms: u64) -> io::Result<String> {
        SupervisorClock::at(
            SystemTime::now()
                .checked_add(Duration::from_millis(ms))
                .ok_or_else(|| error("deadline overflow"))?,
        )
    }
    fn random() -> io::Result<[u8; 32]> {
        let mut v = [0; 32];
        File::open("/dev/urandom")?.read_exact(&mut v)?;
        Ok(v)
    }
    fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        File::open(path.parent().ok_or_else(|| error("missing parent"))?)?.sync_all()
    }
    fn private_root(root: &Path) -> io::Result<File> {
        if fs::canonicalize(root)? != root {
            return Err(error("noncanonical supervisor root"));
        }
        let m = fs::symlink_metadata(root)?;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        if !m.is_dir() || m.mode() & 0o077 != 0 || m.uid() != unsafe { geteuid() } {
            return Err(error(
                "supervisor root must belong exclusively to the trusted identity",
            ));
        }
        File::open(root)
    }
    fn read_private(root: &Path, name: &str, max: usize) -> io::Result<Vec<u8>> {
        let _root = private_root(root)?;
        let path = root.join(name);
        let m = fs::symlink_metadata(&path)?;
        if !m.is_file()
            || m.nlink() != 1
            || m.mode() & 0o077 != 0
            || m.len() > max as u64
            || m.uid() != _root.metadata()?.uid()
        {
            return Err(error("invalid supervisor record custody"));
        }
        #[cfg(target_os = "macos")]
        let flags = 0x0004;
        #[cfg(target_os = "linux")]
        let flags = 0x0800;
        let mut f = OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(&path)?;
        let opened = f.metadata()?;
        if !opened.is_file() || (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
            return Err(error("changed supervisor record"));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut f)
            .take(max as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err(error("oversize supervisor record"));
        }
        Ok(bytes)
    }
    fn read_exact_until(
        stream: &mut UnixStream,
        mut buffer: &mut [u8],
        deadline: Instant,
    ) -> io::Result<()> {
        while !buffer.is_empty() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "supervisor frame deadline")
                })?;
            match stream.read(buffer) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "supervisor frame EOF",
                    ));
                }
                Ok(n) => buffer = &mut buffer[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(remaining.min(Duration::from_millis(1)));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn write_until(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "supervisor frame deadline")
                })?;
            match stream.write(bytes) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "supervisor frame write stalled",
                    ));
                }
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(remaining.min(Duration::from_millis(1)));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> io::Result<()> {
        let b = serde_json::to_vec(value).map_err(error)?;
        if b.len() > MAX_FRAME {
            return Err(error("oversize supervisor frame"));
        }
        let deadline = Instant::now() + IO_WAIT;
        write_until(stream, &(b.len() as u32).to_be_bytes(), deadline)?;
        write_until(stream, &b, deadline)
    }
    fn read_frame<T: serde::de::DeserializeOwned>(
        stream: &mut UnixStream,
        deadline: Instant,
    ) -> io::Result<T> {
        let mut n = [0; 4];
        read_exact_until(stream, &mut n, deadline)?;
        let n = u32::from_be_bytes(n) as usize;
        if n > MAX_FRAME {
            return Err(error("oversize supervisor frame"));
        }
        let mut b = vec![0; n];
        read_exact_until(stream, &mut b, deadline)?;
        serde_json::from_slice(&b).map_err(error)
    }
    #[derive(Serialize, Deserialize)]
    #[serde(tag = "op", deny_unknown_fields)]
    enum Request {
        Inspect,
        Launch {
            attempt: String,
            number: String,
            executable: PathBuf,
            args: Vec<String>,
            window_ms: u64,
            timeout_ms: u64,
        },
        Finalize,
        Claim {
            operation: ReviewOperation,
            scope: ReviewClaimScope,
        },
        Seal {
            bytes: Vec<u8>,
        },
        Publish {
            journal: Vec<u8>,
        },
        CloseHandoff,
        PrepareRecovery {
            recovery: String,
        },
        PrepareReset {
            recovery: String,
        },
        Recover {
            recovery: String,
        },
        Stop,
    }
    #[derive(Serialize, Deserialize)]
    struct Reply {
        data: Option<Value>,
        error: Option<String>,
    }
    /// Capability to reconnect to one surviving owner. No PID or desired reset SHA.
    #[derive(Clone)]
    pub struct SupervisorClient {
        root: PathBuf,
        capability: [u8; 32],
    }
    impl SupervisorClient {
        pub fn connect(root: &Path) -> io::Result<Self> {
            Ok(Self {
                root: root.into(),
                capability: read_private(root, "capability", 32)?
                    .try_into()
                    .map_err(|_| error("invalid capability"))?,
            })
        }
        fn request(&self, r: &Request) -> io::Result<Value> {
            // Encode before connecting: the bounded peer read must not wait on serialization.
            let payload = serde_json::to_vec(r).map_err(error)?;
            if payload.len() > MAX_FRAME {
                return Err(error("oversize supervisor frame"));
            }
            let mut stream = UnixStream::connect(self.root.join("socket"))?;
            stream.set_nonblocking(true)?;
            let deadline = Instant::now() + Duration::from_secs(30);
            write_until(&mut stream, &self.capability, deadline)?;
            write_until(&mut stream, &(payload.len() as u32).to_be_bytes(), deadline)?;
            write_until(&mut stream, &payload, deadline)?;
            let response: Reply = read_frame(&mut stream, deadline)?;
            response.data.ok_or_else(|| {
                error(
                    response
                        .error
                        .unwrap_or_else(|| "supervisor refused".into()),
                )
            })
        }
        pub fn inspect(&self) -> io::Result<Value> {
            self.request(&Request::Inspect)
        }
        pub fn launch(&self, launch: SupervisorLaunch) -> io::Result<Value> {
            self.request(&Request::Launch {
                attempt: launch.attempt,
                number: launch.number,
                executable: launch.executable,
                args: launch.args,
                window_ms: launch
                    .expected_window
                    .as_millis()
                    .try_into()
                    .map_err(error)?,
                timeout_ms: launch.timeout.as_millis().try_into().map_err(error)?,
            })
        }
        pub fn finalize(&self) -> io::Result<Value> {
            self.request(&Request::Finalize)
        }
        pub fn close_handoff(&self) -> io::Result<Value> {
            self.request(&Request::CloseHandoff)
        }
        pub fn prepare_recovery(&self, recovery: &str) -> io::Result<Value> {
            self.request(&Request::PrepareRecovery {
                recovery: recovery.into(),
            })
        }
        pub fn prepare_reset(&self, recovery: &str) -> io::Result<Value> {
            self.request(&Request::PrepareReset {
                recovery: recovery.into(),
            })
        }
        pub fn recover(&self, recovery: &str) -> io::Result<Value> {
            self.request(&Request::Recover {
                recovery: recovery.into(),
            })
        }
        pub fn stop(&self) -> io::Result<Value> {
            self.request(&Request::Stop)
        }
    }
    pub struct SupervisorLaunch {
        pub attempt: String,
        pub number: String,
        pub executable: PathBuf,
        pub args: Vec<String>,
        pub expected_window: Duration,
        pub timeout: Duration,
    }
    impl ReviewRecordSink for SupervisorClient {
        fn claim(&mut self, b: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
            let r = self
                .request(&Request::Claim {
                    operation: b.operation(),
                    scope: b.scope().clone(),
                })
                .map_err(|e| e.to_string())?;
            match r["claim"].as_str() {
                Some("ACQUIRED") => Ok(ReviewClaim::Acquired),
                Some("IN_FLIGHT") => Ok(ReviewClaim::InFlight),
                Some("ABANDONED") => Ok(ReviewClaim::Abandoned),
                Some("TERMINAL") => Ok(ReviewClaim::Terminal(
                    r["outcome"].as_str().unwrap_or_default().into(),
                )),
                Some("SEALED") => {
                    let bytes: Vec<u8> =
                        serde_json::from_value(r["bytes"].clone()).map_err(|e| e.to_string())?;
                    Ok(ReviewClaim::Sealed(SealedReview::from_bytes(&bytes)?))
                }
                _ => Err("invalid supervisor claim reply".into()),
            }
        }
        fn seal(&mut self, b: &ReviewClaimBinding, s: &SealedReview) -> Result<(), String> {
            if s.binding()? != *b {
                return Err("foreign seal scope".into());
            }
            self.request(&Request::Seal {
                bytes: s.to_bytes(),
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
        fn publish(
            &mut self,
            b: &ReviewClaimBinding,
            p: &ReviewRecordProposal,
        ) -> Result<(), String> {
            if p.binding() != b {
                return Err("foreign proposal scope".into());
            }
            self.request(&Request::Publish {
                journal: p.to_journal(),
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
    }
    /// Only trusted composition constructs this bootstrap. It creates a fresh private
    /// root; the command refuses replay and provisions a NEW workspace, never adoption.
    pub fn spawn_supervisor(
        executable: &Path,
        root: &Path,
        config: &SupervisorConfig,
    ) -> io::Result<(Child, SupervisorClient)> {
        if !executable.is_absolute() || !root.is_absolute() {
            return Err(error("absolute supervisor paths required"));
        }
        fs::DirBuilder::new().mode(0o700).create(root)?;
        let capability = random()?;
        create(
            &root.join("config"),
            &serde_json::to_vec(config).map_err(error)?,
        )?;
        create(&root.join("capability"), &capability)?;
        let mut child = Command::new(executable)
            .arg("__workspace-supervisor")
            .arg(root)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(File::create(root.join("supervisor.stderr"))?)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&capability)?;
        }
        let client = SupervisorClient {
            root: root.into(),
            capability,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() {
            if let Some(status) = child.try_wait()? {
                let mut detail = String::new();
                File::open(root.join("supervisor.stderr"))?
                    .take(8192)
                    .read_to_string(&mut detail)?;
                return Err(error(format!(
                    "supervisor failed to start: {status}: {detail}"
                )));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error("supervisor startup deadline; custody retained"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Err(e) = client.inspect() {
            let _ = client.stop();
            let deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait()?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if child.try_wait()?.is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
            return Err(e);
        }
        Ok((child, client))
    }
    struct Active {
        id: String,
        handle: receipts_workspace_execution::WorkspaceHandle,
        live: LiveProcessAttempt,
        report: CleanupEvidenceReport,
        requests: Receiver<CleanupEvidenceRequest>,
        terminal: Option<LiveProcessOutcome>,
        admission: Option<String>,
        handoff_revision: i64,
        finalized: bool,
    }
    struct Server {
        active: Option<Active>,
        custody: WorkspaceRecoveryCustody,
        writer: CoreStateWriter,
        root: PathBuf,
        root_pin: File,
        config: SupervisorConfig,
        capability: [u8; 32],
        incarnation: String,
        sequence: u64,
        resolved: bool,
        target: Option<WorkspaceAcceptedTargetV1>,
        assurance: Option<String>,
    }
    impl Server {
        fn verify_root(&self) -> io::Result<()> {
            let current = private_root(&self.root)?.metadata()?;
            let original = self.root_pin.metadata()?;
            if (current.dev(), current.ino()) != (original.dev(), original.ino()) {
                return Err(error("supervisor root replaced"));
            }
            Ok(())
        }
        fn journal(&mut self, kind: &str, data: Value) -> io::Result<()> {
            self.verify_root()?;
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| error("journal exhausted"))?;
            create(
                &self.root.join(format!("journal-{}", self.sequence)),
                &serde_json::to_vec(
                    &json!({"kind":kind,"incarnation":self.incarnation,"at":now()?,"data":data}),
                )
                .map_err(error)?,
            )
        }
        fn capture(
            &mut self,
            attempt: &str,
            kind: WorkspaceCheckpointKind,
            streams: &[(&str, &CapturedStream)],
        ) -> io::Result<RecoveryCheckpoint> {
            let id = format!("{attempt}-{}", self.sequence + 1);
            let cp = self.custody.capture(
                RecoveryCaptureRequest {
                    project: &self.config.project,
                    attempt,
                    checkpoint_id: &id,
                    kind,
                    output_directory: &self.root.join("outputs"),
                    timestamp: WorkspaceDateTimeV1::try_new(now()?).map_err(error)?,
                },
                streams,
            )?;
            self.journal("CHECKPOINT_PREPARED",json!({"checkpoint":id,"sha256":hash(cp.record.to_json().map_err(error)?.as_bytes())}))?;
            for output in &cp.outputs {
                self.writer
                    .append_output_ref(output.clone())
                    .map_err(error)?;
            }
            self.writer
                .append_checkpoint(
                    CheckpointRecord::parse_json(
                        &self.config.project,
                        &cp.record.to_json().map_err(error)?,
                    )
                    .map_err(error)?,
                )
                .map_err(error)?;
            Ok(cp)
        }
        fn subject(&self, active: &Active) -> WorkspaceRecoverySubjectV1 {
            WorkspaceRecoverySubjectV1 {
                project_id: self.config.project.clone(),
                graph_id: self.config.graph.clone(),
                attempt_id: active.id.clone(),
            }
        }
        fn authority(
            &self,
            active: &Active,
            recovery: &str,
            revision: i64,
        ) -> WorkspaceRecoveryAuthorityV1 {
            WorkspaceRecoveryAuthorityV1 {
                subject: self.subject(active),
                recovery_id: recovery.into(),
                expected_revision: revision,
                provisioning_identity: self.custody.identity().into(),
                supervisor_identity: self.incarnation.clone(),
            }
        }
        fn recovery_evidence(
            &mut self,
            active: &Active,
            recovery: &str,
        ) -> io::Result<WorkspaceRecoveryEvidenceV1> {
            let cp = if let Some(t) = &active.terminal {
                self.capture(
                    &active.id,
                    WorkspaceCheckpointKind::RecoveryCapture,
                    &[("stdout", t.stdout()), ("stderr", t.stderr())],
                )?
            } else {
                let out = active.live.snapshot_output().map_err(error)?;
                self.capture(
                    &active.id,
                    WorkspaceCheckpointKind::RecoveryCapture,
                    &[("stdout", out.stdout()), ("stderr", out.stderr())],
                )?
            };
            Ok(WorkspaceRecoveryEvidenceV1 {
                recovery_id: recovery.into(),
                checkpoint_id: cp.record.checkpoint_id().into(),
                checkpoint_sha256: hash(cp.record.to_json().map_err(error)?.as_bytes()),
                output_refs: cp.outputs.iter().map(|o| o.output_ref().into()).collect(),
            })
        }
        fn handoff(
            &mut self,
            active: &mut Active,
            disposition: WorkspaceHandoffDispositionV1,
        ) -> io::Result<()> {
            self.custody.verify()?;
            let bytes=serde_json::to_vec(&json!({"attempt":active.id,"workspace":self.config.workspace,"incarnation":self.incarnation,"provisioning":self.custody.identity(),"disposition":disposition,"finalized":active.finalized})).map_err(error)?;
            let (digest, path) = receipts_workspace_execution::output_store::store_output(
                &self.root.join("outputs"),
                &bytes,
            )?;
            self.journal(
                "HANDOFF",
                json!({"attempt":active.id,"digest":digest,"path":path}),
            )?;
            if active.admission.is_some() {
                active.handoff_revision = self
                    .writer
                    .record_workspace_handoff_v1(
                        &SupervisorClock,
                        &WorkspaceHandoffRecordV1 {
                            subject: self.subject(active),
                            expected_revision: active.handoff_revision,
                            disposition,
                            evidence_ref: format!("handoff-{}", active.id),
                            evidence_sha256: digest,
                            provisioning_identity: self.custody.identity().into(),
                        },
                    )
                    .map_err(error)?;
            }
            Ok(())
        }
        fn launch(
            &mut self,
            id: String,
            number: String,
            exe: PathBuf,
            args: Vec<String>,
            window: u64,
            timeout: u64,
        ) -> io::Result<Value> {
            if self.assurance.is_some() || self.active.as_ref().is_some_and(|a| !a.finalized) {
                return Err(error("prior attempt/custody is unresolved"));
            }
            if window == 0 || window > timeout {
                return Err(error("invalid bounded launch windows"));
            }
            self.custody.verify()?;
            let handle = self.custody.attempt_handle()?;
            if self
                .target
                .as_ref()
                .is_some_and(|t| t.accepted_commit != handle.base_sha().as_str())
            {
                return Err(error("launch is not at the last accepted commit"));
            }
            let role = self
                .writer
                .reader()
                .find_logical_role(&self.config.role)
                .map_err(error)?
                .ok_or_else(|| error("missing trusted role"))?;
            if role.project_id != self.config.project || role.status != LogicalRoleStatus::Active {
                return Err(error("inactive or foreign role"));
            }
            let binding = self
                .writer
                .reader()
                .find_executor_binding(
                    role.active_binding_id
                        .as_deref()
                        .ok_or_else(|| error("missing active binding"))?,
                )
                .map_err(error)?
                .ok_or_else(|| error("missing binding"))?;
            if binding.released_at.is_some() {
                return Err(error("released binding"));
            }
            let request =
                ProcessRunRequest::new(&exe, args, handle.worktree_path(), handle.worktree_path())
                    .map_err(error)?;
            let policy = ProcessTimeoutPolicy::new(
                Duration::from_millis(timeout),
                Duration::from_millis(100),
            )
            .map_err(error)?;
            self.journal("LAUNCH_PREPARED",json!({"attempt":id,"provisioning":self.custody.identity(),"executable":exe,"start":handle.base_sha().as_str()}))?;
            self.writer
                .append_attempt_started(
                    AttemptStart::new(
                        AttemptId::parse(&id).map_err(error)?,
                        &self.config.project,
                        &self.config.graph,
                        &self.config.task,
                        &number,
                        AttemptRole::RuntimeA3,
                        &binding.runtime_id,
                        &self.config.workspace,
                        handle.base_sha().as_str(),
                        &now()?,
                    )
                    .map_err(error)?,
                )
                .map_err(error)?;
            // Registration is durable; any later failure must prevent another launch.
            self.assurance = Some("launch registration incomplete; custody quarantined".into());
            let admission = if let Some(target) = &self.target {
                Some(
                    self.writer
                        .admit_workspace_recovery_v1(
                            &SupervisorClock,
                            &WorkspaceRecoveryAdmissionV1 {
                                subject: WorkspaceRecoverySubjectV1 {
                                    project_id: self.config.project.clone(),
                                    graph_id: self.config.graph.clone(),
                                    attempt_id: id.clone(),
                                },
                                workspace_id: self.config.workspace.clone(),
                                role_id: self.config.role.clone(),
                                binding_id: binding.binding_id.clone(),
                                lease_deadline: binding.lease_expires_at.clone(),
                                expected_execution_deadline: future(window)?,
                                context_epoch: role.current_context_epoch.as_str().into(),
                                provisioning_identity: self.custody.identity().into(),
                                supervisor_identity: self.incarnation.clone(),
                                accepted_anchor_id: target.anchor_id.clone(),
                            },
                        )
                        .map_err(error)?,
                )
            } else {
                None
            };
            self.capture(&id, WorkspaceCheckpointKind::Progress, &[])?;
            let (barrier, requests) = CleanupEvidenceBarrier::channel();
            let report = barrier.report();
            let live = match start_live_process_attempt_with_cleanup_evidence(
                &request, &policy, barrier,
            ) {
                Ok(a) => a,
                Err(e) => {
                    self.journal("LAUNCH_FAILED",json!({"attempt":id,"error":e.to_string(),"evidence":format!("{:?}",report.status())}))?;
                    return Err(error(e));
                }
            };
            let mut active = Active {
                id,
                handle,
                live,
                report,
                requests,
                terminal: None,
                admission,
                handoff_revision: 0,
                finalized: false,
            };
            // From this point errors must retain the owning attempt for emergency cleanup.
            let result = self
                .handoff(&mut active, WorkspaceHandoffDispositionV1::Pending)
                .and_then(|_| self.journal("LAUNCHED", json!({"attempt":active.id})));
            self.active = Some(active);
            if let Err(e) = result {
                self.assurance = Some(e.to_string());
                if let Some(mut active) = self.active.take() {
                    let _ = collect_locally(&self.root, &mut active);
                    self.active = Some(active);
                }
                return Err(e);
            }
            self.assurance = None;
            self.inspect()
        }
        fn tick(&mut self) -> io::Result<()> {
            if self.assurance.is_some() {
                return Ok(());
            }
            let Some(mut active) = self.active.take() else {
                return Ok(());
            };
            let result = (|| {
                while let Ok(request) = active.requests.try_recv() {
                    let cp = self.capture(
                        &active.id,
                        WorkspaceCheckpointKind::PreTermination,
                        &[
                            ("stdout", request.output.stdout()),
                            ("stderr", request.output.stderr()),
                        ],
                    );
                    match cp {
                        Ok(_) => {
                            if !request.acknowledge() {
                                return Err(error("cleanup acknowledgement expired"));
                            }
                        }
                        Err(e) => {
                            drop(request);
                            return Err(e);
                        }
                    }
                }
                if active.live.is_terminating_or_closed()
                    && active.report.status() != CleanupEvidenceStatus::Pending
                    && active.terminal.is_none()
                {
                    let terminal = active.live.wait_collect().map_err(error)?;
                    let evidence = terminal.cleanup_evidence_status();
                    self.journal("COLLECTED",json!({"attempt":active.id,"cause":format!("{:?}",terminal.terminal_cause()),"cleanup_evidence":format!("{evidence:?}")}))?;
                    active.terminal = Some(terminal);
                    if evidence != CleanupEvidenceStatus::Acknowledged {
                        return Err(error(
                            "owned process cleaned up without acknowledged evidence",
                        ));
                    }
                }
                Ok(())
            })();
            self.active = Some(active);
            if let Err(e) = &result {
                self.assurance = Some(e.to_string());
            }
            result
        }
        fn finalize(&mut self, active: &mut Active) -> io::Result<Value> {
            if self.assurance.is_some()
                || active.report.status() != CleanupEvidenceStatus::Acknowledged
            {
                return Err(error("unverified terminal evidence"));
            }
            let terminal = active
                .terminal
                .as_ref()
                .ok_or_else(|| error("attempt not collected"))?;
            let cp = self.capture(
                &active.id,
                WorkspaceCheckpointKind::Progress,
                &[("stdout", terminal.stdout()), ("stderr", terminal.stderr())],
            )?;
            let head = self.custody.head()?;
            let evidence = receipts_workspace_execution::finalize_workspace_attempt(
                receipts_workspace_execution::WorkspaceAttemptFinalizationRequest {
                    handle: &active.handle,
                    workspace_id: &self.config.workspace,
                    task_id: Some(&self.config.task),
                    attempt_id: Some(&active.id),
                    expected_worktree: active.handle.worktree_path(),
                    expected_branch: active.handle.branch(),
                    start_sha: active.handle.base_sha().as_str(),
                    final_sha: &head,
                    allowed_write_paths: &self.config.allowed_write_paths,
                    forbidden_write_paths: &self.config.forbidden_write_paths,
                    checkpoint: cp.capture.core(),
                    terminal,
                },
            )
            .map_err(error)?;
            self.custody.verify()?;
            let record = FinalizationRecordV1 {
                project_id: self.config.project.clone(),
                graph_id: self.config.graph.clone(),
                attempt_id: active.id.clone(),
                workspace_id: self.config.workspace.clone(),
                checkpoint_id: cp.record.checkpoint_id().into(),
                final_sha: evidence.final_sha().as_str().into(),
                write_scope_verdict: "PASS".into(),
                check_results: vec![],
                output_refs: cp.outputs.iter().map(|o| o.output_ref().into()).collect(),
            };
            self.writer
                .finish_with_finalization_v1(
                    &record,
                    &AttemptFinish::new(
                        AttemptId::parse(&active.id).map_err(error)?,
                        &now()?,
                        Some(&record.final_sha),
                        None,
                        None,
                    )
                    .map_err(error)?,
                )
                .map_err(error)?;
            active.finalized = true;
            self.handoff(active, WorkspaceHandoffDispositionV1::Eligible)?;
            Ok(json!({"final_sha":head,"checkpoint":record.checkpoint_id,"finalized":true}))
        }
        fn publish(&mut self, journal: Vec<u8>) -> io::Result<Value> {
            let proposal = ReviewRecordProposal::from_journal(&journal).map_err(error)?;
            PublicationSink {
                writer: &mut self.writer,
                clock: &SupervisorClock,
            }
            .publish(proposal.binding(), &proposal)
            .map_err(error)?;
            if proposal.binding().operation() == ReviewOperation::Acceptance {
                for decision in &proposal.decision_records {
                    if decision.kind() != DecisionRecordKind::IntegrationDecision {
                        continue;
                    }
                    let value: Value =
                        serde_json::from_str(decision.record_json()).map_err(error)?;
                    if value["outcome"] != "ACCEPT" {
                        continue;
                    }
                    let binding_result = (|| -> io::Result<()> {
                        let active = self
                            .active
                            .as_ref()
                            .ok_or_else(|| error("acceptance has no original workspace attempt"))?;
                        if !active.finalized || self.assurance.is_some() {
                            return Err(error(
                                "acceptance has no verified terminal/workspace custody",
                            ));
                        }
                        let (commit, tree) = self.custody.accepted_tree()?;
                        let target = WorkspaceAcceptedTargetV1 {
                            subject: self.subject(active),
                            anchor_id: format!(
                                "anchor-{}",
                                unit(proposal.binding()).map_err(error)?.claim_id()
                            ),
                            workspace_id: self.config.workspace.clone(),
                            accepted_commit: commit,
                            accepted_tree: tree,
                            acceptance_claim_id: unit(proposal.binding())
                                .map_err(error)?
                                .claim_id(),
                            decision_id: decision.record_id().into(),
                            provisioning_identity: self.custody.identity().into(),
                        };
                        self.writer
                            .bind_workspace_acceptance_v1(&target)
                            .map_err(error)?;
                        self.journal(
                            "ACCEPTED_WORKSPACE",
                            serde_json::to_value(&target).map_err(error)?,
                        )?;
                        self.target = Some(target);
                        Ok(())
                    })();
                    if let Err(e) = binding_result {
                        self.assurance = Some(e.to_string());
                        return Err(e);
                    }
                }
            }
            Ok(json!({"published":true,"anchor":self.target}))
        }
        fn inspect(&self) -> io::Result<Value> {
            self.verify_root()?;
            self.custody.verify()?;
            Ok(
                json!({"incarnation":self.incarnation,"provisioning_identity":self.custody.identity(),"store_id":self.writer.store_id(),"head":self.custody.head()?,"anchor":self.target,"assurance_failure":self.assurance,"attempt":self.active.as_ref().map(|a|json!({"id":a.id,"collected":a.terminal.is_some(),"finalized":a.finalized,"cleanup_evidence":format!("{:?}",a.report.status()),"admission":a.admission}))}),
            )
        }
        fn prepare(
            &mut self,
            active: &Active,
            recovery: &str,
        ) -> io::Result<WorkspaceRecoveryRecordV1> {
            self.custody.verify()?;
            if self.assurance.is_some() {
                return Err(error("cleanup assurance failed; custody quarantined"));
            }
            let hash = active
                .admission
                .as_ref()
                .ok_or_else(|| error("no admitted recoverable launch"))?;
            let mut r = self
                .writer
                .claim_workspace_recovery_v1(
                    &SupervisorClock,
                    &WorkspaceRecoveryClaimRequestV1 {
                        subject: self.subject(active),
                        recovery_id: recovery.into(),
                        admission_sha256: hash.clone(),
                    },
                )
                .map_err(error)?;
            if !r.active {
                return Ok(r);
            }
            if r.phase == "CLAIMED" || r.phase == "SIGNAL_PREPARED" {
                let e = self.recovery_evidence(active, recovery)?;
                r = self
                    .writer
                    .acknowledge_workspace_recovery_v1(
                        &SupervisorClock,
                        &self.authority(active, recovery, r.revision),
                        WorkspaceRecoveryEvidenceStageV1::Captured,
                        &e,
                    )
                    .map_err(error)?;
            }
            if r.phase == "CAPTURED" && active.terminal.is_none() {
                self.custody.verify()?;
                let auth = self.authority(active, recovery, r.revision);
                let action = self
                    .writer
                    .begin_workspace_recovery_action_v1(
                        &SupervisorClock,
                        &auth,
                        WorkspaceRecoveryActionKindV1::Signal,
                    )
                    .map_err(error)?;
                r.revision = action.revision();
                r.phase = "SIGNAL_PREPARED".into();
                drop(action);
            }
            Ok(r)
        }
        fn quiesce(
            &mut self,
            active: &mut Active,
            recovery: &str,
        ) -> io::Result<WorkspaceRecoveryRecordV1> {
            let mut r = self.prepare(active, recovery)?;
            if !r.active || matches!(r.phase.as_str(), "QUIESCENT" | "RESET_PREPARED") {
                return Ok(r);
            }
            if active.terminal.is_none() {
                // Reconcile prepared-but-unperformed work; a receipt never repeats a signal.
                let e = self.recovery_evidence(active, recovery)?;
                r = self
                    .writer
                    .acknowledge_workspace_recovery_v1(
                        &SupervisorClock,
                        &self.authority(active, recovery, r.revision),
                        WorkspaceRecoveryEvidenceStageV1::Captured,
                        &e,
                    )
                    .map_err(error)?;
                self.custody.verify()?;
                let auth = self.authority(active, recovery, r.revision);
                let action = self
                    .writer
                    .begin_workspace_recovery_action_v1(
                        &SupervisorClock,
                        &auth,
                        WorkspaceRecoveryActionKindV1::Signal,
                    )
                    .map_err(error)?;
                let cleaned = collect_locally(&self.root, active);
                if let Err(e) = cleaned {
                    drop(action);
                    self.assurance = Some(e.to_string());
                    return Err(e);
                }
                self.custody.verify()?;
                r = action.finish_signal(&SupervisorClock, &e).map_err(error)?;
            }
            let e = self.recovery_evidence(active, recovery)?;
            self.writer
                .acknowledge_workspace_recovery_v1(
                    &SupervisorClock,
                    &self.authority(active, recovery, r.revision),
                    WorkspaceRecoveryEvidenceStageV1::Quiescent,
                    &e,
                )
                .map_err(error)
        }
        fn recover(
            &mut self,
            active: &mut Active,
            recovery: &str,
            prepare_only: bool,
        ) -> io::Result<Value> {
            let mut r = self.quiesce(active, recovery)?;
            if !r.active {
                self.resolved = true;
                return Ok(json!({"phase":r.phase,"revision":r.revision,"active":false}));
            }
            let target = self
                .target
                .clone()
                .ok_or_else(|| error("missing accepted physical target"))?;
            self.custody.verify()?;
            if r.phase == "RESET_PREPARED"
                && self
                    .custody
                    .readback(&target.accepted_commit, &target.accepted_tree)
                    .is_ok()
            {
                let e = self.recovery_evidence(active, recovery)?;
                r = self
                    .writer
                    .resolve_workspace_recovery_v1(
                        &SupervisorClock,
                        &self.authority(active, recovery, r.revision),
                        &WorkspaceRecoveryResetObservationV1 {
                            evidence: e,
                            observed_commit: target.accepted_commit.clone(),
                            index_tree: target.accepted_tree.clone(),
                            tracked_tree: target.accepted_tree.clone(),
                        },
                    )
                    .map_err(error)?;
            } else {
                if r.phase == "RESET_PREPARED" {
                    let e = self.recovery_evidence(active, recovery)?;
                    r = self
                        .writer
                        .acknowledge_workspace_recovery_v1(
                            &SupervisorClock,
                            &self.authority(active, recovery, r.revision),
                            WorkspaceRecoveryEvidenceStageV1::Quiescent,
                            &e,
                        )
                        .map_err(error)?;
                }
                self.custody.verify()?;
                let auth = self.authority(active, recovery, r.revision);
                let action = self
                    .writer
                    .begin_workspace_recovery_action_v1(
                        &SupervisorClock,
                        &auth,
                        WorkspaceRecoveryActionKindV1::Reset,
                    )
                    .map_err(error)?;
                r.revision = action.revision();
                r.phase = "RESET_PREPARED".into();
                if prepare_only {
                    drop(action);
                    return Ok(json!({"phase":r.phase,"revision":r.revision,"active":true}));
                }
                self.custody.reset(&target, &action)?;
                // The action retained the exclusive writer through physical readback.
                // Quiescence/pins remain owned while post-action evidence is persisted.
                drop(action);
                let e = self.recovery_evidence(active, recovery)?;
                self.custody
                    .readback(&target.accepted_commit, &target.accepted_tree)?;
                r = self
                    .writer
                    .resolve_workspace_recovery_v1(
                        &SupervisorClock,
                        &self.authority(active, recovery, r.revision),
                        &WorkspaceRecoveryResetObservationV1 {
                            evidence: e,
                            observed_commit: target.accepted_commit.clone(),
                            index_tree: target.accepted_tree.clone(),
                            tracked_tree: target.accepted_tree.clone(),
                        },
                    )
                    .map_err(error)?;
            }
            self.resolved = true;
            self.journal("RESOLVED",json!({"attempt":active.id,"recovery":recovery,"revision":r.revision,"commit":target.accepted_commit,"tree":target.accepted_tree}))?;
            Ok(
                json!({"phase":r.phase,"revision":r.revision,"active":r.active,"head":self.custody.head()?}),
            )
        }
        fn request(&mut self, request: Request) -> io::Result<(Value, bool)> {
            if matches!(request, Request::Stop) {
                let custody = self.verify_root().and_then(|_| self.custody.verify());
                if let Some(mut active) = self.active.take() {
                    let cleanup = if custody.is_ok() {
                        collect_locally(&self.root, &mut active)
                    } else {
                        collect_without_evidence(&mut active)
                    };
                    self.active = Some(active);
                    if let Err(e) = cleanup {
                        self.assurance = Some(e.to_string());
                    }
                }
                if let Err(e) = custody {
                    self.assurance = Some(e.to_string());
                }
                if let Err(e) = self.journal(
                    "SUPERVISOR_STOP",
                    json!({"assurance_failure":self.assurance}),
                ) {
                    self.assurance = Some(e.to_string());
                }
                if self.resolved
                    && self.assurance.is_none()
                    && let Err(e) = self.custody.release()
                {
                    self.assurance = Some(e.to_string());
                }
                return Ok((
                    json!({"stopped":true,"assurance_failure":self.assurance}),
                    true,
                ));
            }
            self.verify_root()?;
            self.custody.verify()?;
            match request {
                Request::Inspect => Ok((self.inspect()?, false)),
                Request::Launch {
                    attempt,
                    number,
                    executable,
                    args,
                    window_ms,
                    timeout_ms,
                } => Ok((
                    self.launch(attempt, number, executable, args, window_ms, timeout_ms)?,
                    false,
                )),
                Request::Claim { operation, scope } => {
                    let c = PublicationSink {
                        writer: &mut self.writer,
                        clock: &SupervisorClock,
                    }
                    .claim(&ReviewClaimBinding::new(operation, scope))
                    .map_err(error)?;
                    let value = match c {
                        ReviewClaim::Acquired => json!({"claim":"ACQUIRED"}),
                        ReviewClaim::InFlight => json!({"claim":"IN_FLIGHT"}),
                        ReviewClaim::Abandoned => json!({"claim":"ABANDONED"}),
                        ReviewClaim::Terminal(outcome) => {
                            json!({"claim":"TERMINAL","outcome":outcome})
                        }
                        ReviewClaim::Sealed(s) => json!({"claim":"SEALED","bytes":s.to_bytes()}),
                    };
                    Ok((value, false))
                }
                Request::Seal { bytes } => {
                    let s = SealedReview::from_bytes(&bytes).map_err(error)?;
                    PublicationSink {
                        writer: &mut self.writer,
                        clock: &SupervisorClock,
                    }
                    .seal(&s.binding().map_err(error)?, &s)
                    .map_err(error)?;
                    Ok((json!({"sealed":true}), false))
                }
                Request::Publish { journal } => Ok((self.publish(journal)?, false)),
                Request::Stop => unreachable!(),
                operation => {
                    let mut active = self
                        .active
                        .take()
                        .ok_or_else(|| error("no owned attempt"))?;
                    let result=match operation {
                    Request::Finalize=>self.finalize(&mut active),
                    Request::CloseHandoff=>self.handoff(&mut active,WorkspaceHandoffDispositionV1::ClosedWithoutHandoff).map(|_|json!({"handoff":"CLOSED_WITHOUT_HANDOFF"})),
                    Request::PrepareRecovery {recovery}=>self.prepare(&active,&recovery).map(|r|json!({"phase":r.phase,"revision":r.revision,"active":r.active})),
                    Request::PrepareReset {recovery}=>self.recover(&mut active,&recovery,true),
                    Request::Recover {recovery}=>self.recover(&mut active,&recovery,false),
                    _=>unreachable!(),
                };
                    self.active = Some(active);
                    result.map(|v| (v, false))
                }
            }
        }
    }
    fn collect_without_evidence(active: &mut Active) -> io::Result<()> {
        if active.terminal.is_some() {
            return Ok(());
        }
        if active.terminal.is_none() {
            active.live.cancel();
            let terminal = active.live.wait_collect().map_err(error)?;
            active.terminal = Some(terminal);
        }
        Err(error(
            "owned process cleanup collected; evidence/custody assurance unavailable",
        ))
    }
    fn collect_locally(root: &Path, active: &mut Active) -> io::Result<()> {
        if active.terminal.is_some() {
            return Ok(());
        }
        active.live.cancel();
        let acknowledged = if active.report.status() == CleanupEvidenceStatus::Pending {
            match active
                .requests
                .recv_timeout(CLEANUP_EVIDENCE_TIMEOUT + Duration::from_millis(100))
            {
                Ok(request) => {
                    let persisted = (|| {
                        let mut artifacts = Vec::new();
                        for (name, stream) in [
                            ("stdout", request.output.stdout()),
                            ("stderr", request.output.stderr()),
                        ] {
                            let mut bytes = stream.head().to_vec();
                            bytes.extend_from_slice(stream.tail());
                            let (digest, path) =
                                receipts_workspace_execution::output_store::store_output(
                                    &root.join("outputs"),
                                    &bytes,
                                )?;
                            artifacts.push(json!({"stream":name,"digest":digest,"path":path}));
                        }
                        create(&root.join(format!("cleanup-{}",active.id)),&serde_json::to_vec(&json!({"attempt":active.id,"reason":format!("{:?}",request.reason),"artifacts":artifacts})).map_err(error)?)
                    })();
                    match persisted {
                        Ok(()) => {
                            if request.acknowledge() {
                                Ok(())
                            } else {
                                Err(error("emergency cleanup acknowledgement expired"))
                            }
                        }
                        Err(e) => {
                            drop(request);
                            Err(e)
                        }
                    }
                }
                Err(e) => Err(error(e)),
            }
        } else {
            Ok(())
        };
        let terminal = active.live.wait_collect().map_err(error)?;
        let status = terminal.cleanup_evidence_status();
        active.terminal = Some(terminal);
        acknowledged?;
        if status != CleanupEvidenceStatus::Acknowledged {
            return Err(error(
                "emergency cleanup completed without durable evidence acknowledgement",
            ));
        }
        Ok(())
    }
    impl Drop for Server {
        fn drop(&mut self) {
            if let Some(mut active) = self.active.take() {
                let result = if self
                    .verify_root()
                    .and_then(|_| self.custody.verify())
                    .is_ok()
                {
                    collect_locally(&self.root, &mut active)
                } else {
                    collect_without_evidence(&mut active)
                };
                let _=self.journal("OWNER_EXIT",json!({"attempt":active.id,"cleanup":result.as_ref().err().map(ToString::to_string),"evidence":format!("{:?}",active.report.status())}));
                if let Err(e) = result {
                    eprintln!("receipts: cleanup assurance failure: {e}");
                }
            }
        }
    }
    /// Internal entry, authenticated before any provisioning. Replaying a bootstrap
    /// or losing the supervisor cannot recreate original process/custody authority.
    pub fn run_internal_supervisor(root: &Path) -> io::Result<()> {
        let root_pin = private_root(root)?;
        let capability: [u8; 32] = read_private(root, "capability", 32)?
            .try_into()
            .map_err(|_| error("invalid capability"))?;
        // One fixed capability frame; trailing stdin bytes are never commands.
        let mut supplied = [0; 32];
        std::io::stdin().read_exact(&mut supplied)?;
        if supplied != capability {
            return Err(error("internal supervisor bootstrap refused"));
        }
        create(&root.join("started"), hash(&capability).as_bytes())?;
        let config: SupervisorConfig =
            serde_json::from_slice(&read_private(root, "config", 64 * 1024)?).map_err(error)?;
        let writer = CoreStateWriter::open(&config.state, config.identity()?).map_err(error)?;
        if writer
            .reader()
            .load_current_graph_v1(&config.project, &config.graph)
            .map_err(error)?
            .is_none()
        {
            return Err(error("missing admitted production graph"));
        }
        let listener = UnixListener::bind(root.join("socket"))?;
        listener.set_nonblocking(true)?;
        let custody = WorkspaceRecoveryCustody::provision(
            &WorkspaceProvisionRequest::new(
                &config.repository,
                &config.workspace,
                Some(&config.task),
                &config.branch,
                &config.worktree,
                &config.base,
            )
            .map_err(error)?,
        )?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join("outputs"))?;
        let mut server = Server {
            active: None,
            custody,
            writer,
            root: root.into(),
            root_pin,
            config,
            capability,
            incarnation: hash(&random()?),
            sequence: 0,
            resolved: false,
            target: None,
            assurance: None,
        };
        server.journal("PROVISIONED",json!({"handle":receipts_workspace_execution::WorkspaceHandleWire::try_from(server.custody.handle()).map_err(error)?,"provisioning_identity":server.custody.identity(),"store_id":server.writer.store_id()}))?;
        create(&root.join("ready"), server.incarnation.as_bytes())?;
        loop {
            if let Err(e) = server.tick() {
                eprintln!("receipts: cleanup assurance failure: {e}");
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Explicit nonblocking I/O uses one monotonic budget, including partial frames.
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    let deadline = Instant::now() + IO_WAIT;
                    let mut token = [0; 32];
                    if read_exact_until(&mut stream, &mut token, deadline).is_err()
                        || token != server.capability
                    {
                        continue;
                    }
                    let request = read_frame(&mut stream, deadline);
                    if let Err(e) = &request {
                        eprintln!("receipts: supervisor request read refused: {:?}", e.kind());
                    }
                    let (reply, stop) = match request.and_then(|r| server.request(r)) {
                        Ok((data, stop)) => (
                            Reply {
                                data: Some(data),
                                error: None,
                            },
                            stop,
                        ),
                        Err(e) => (
                            Reply {
                                data: None,
                                error: Some(e.to_string()),
                            },
                            false,
                        ),
                    };
                    let _ = write_frame(&mut stream, &reply);
                    if stop {
                        return if server.assurance.is_some() {
                            Err(error("cleanup assurance failure; custody retained"))
                        } else {
                            Ok(())
                        };
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        }
    }
}
#[cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
pub use platform::*;
