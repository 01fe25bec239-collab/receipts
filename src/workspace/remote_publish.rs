//! D12 publication for trusted composition; never expose this boundary to workers.
use crate::{CommitSha, WorkspaceRemotePublishPolicy as Wire, git, validate_branch_name};
use std::{ffi::OsStr, path::Path};

/// Paths are admitted by trusted composition, outside the worker repository.
/// Git configuration supplies existing credential helpers, TLS and SSH settings;
/// HOME and the agent socket are explicit, never inherited from the environment.
/// No credential value belongs in this structure or in the endpoint URL.
pub struct TrustedTransportSettings<'a> {
    pub git_config: &'a Path,
    pub home: Option<&'a Path>,
    pub ssh_auth_sock: Option<&'a Path>,
}

fn transport_capture(
    repository: &Path,
    args: &[&OsStr],
    settings: Option<&TrustedTransportSettings<'_>>,
) -> Result<git::GitCapture, ()> {
    match settings {
        Some(settings) => git::publication_capture(repository, args, settings),
        None => git::capture(repository, "publication", args).map_err(|_| ()),
    }
}
fn protocol(remote: &str) -> &'static str {
    if remote.starts_with("https://") {
        "protocol.https.allow=always"
    } else if remote.starts_with("ssh://") {
        "protocol.ssh.allow=always"
    } else {
        "protocol.file.allow=always"
    }
}
fn transport_allowed(
    request: &PublicationRequest<'_>,
    settings: Option<&TrustedTransportSettings<'_>>,
) -> bool {
    if !receipts_state::remote_endpoint_allowed(request.remote) {
        // Preserve named owned-file remotes for the legacy transport helper.
        // Network endpoints must come directly from trusted composition.
        if settings.is_some()
            || request.remote.is_empty()
            || !request
                .remote
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return false;
        }
        let key = format!("remote.{}.url", request.remote);
        let Ok(url) = git::capture(
            request.repository,
            "file remote",
            &[
                OsStr::new("config"),
                OsStr::new("--get-all"),
                OsStr::new(&key),
            ],
        ) else {
            return false;
        };
        return url.success
            && url.stdout.lines().count() == 1
            && Path::new(url.stdout.trim()).is_absolute();
    }
    if !request.remote.starts_with('/') && settings.is_none() {
        return false;
    }
    // Git also treats a full URL as a remote name. Refuse both rewriting and
    // remote subsections that shadow the immutable execution endpoint.
    let Ok(listing) = transport_capture(
        request.repository,
        &[
            OsStr::new("config"),
            OsStr::new("--null"),
            OsStr::new("--name-only"),
            OsStr::new("--get-regexp"),
            OsStr::new(r"^(url|remote)\."),
        ],
        settings,
    ) else {
        return false;
    };
    if !listing.success {
        return listing.exit_status == "exit status: 1" && listing.raw_stdout().is_empty();
    }
    let prefix = format!("remote.{}.", request.remote);
    let Some(fields) = listing.raw_stdout().strip_suffix(&[0]) else {
        return false;
    };
    fields
        .split(|byte| *byte == 0)
        .all(|key| !key.starts_with(b"url.") && !key.starts_with(prefix.as_bytes()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRemoteMode {
    LocalOnly,
    PushA2Branches,
    PushAcceptedA3,
    PushAllCheckpoints,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationBranchClass {
    Workstream,
    Task,
    Checkpoint,
}
/// Supplied by trusted composition after verifying exact review acceptance.
/// This inert input is not an issuer authentication boundary.
pub struct PublicationRequest<'a> {
    pub repository: &'a Path,
    pub remote: &'a str,
    pub branch: &'a str,
    pub commit: &'a str,
    pub wire: Wire,
    pub mode: ProjectRemoteMode,
    pub class: PublicationBranchClass,
    pub accepted_commit: Option<&'a str>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PublicationOutcome {
    LocalOnly,
    Published,
    Refused,
    RemoteFailed,
}

pub fn publish(request: &PublicationRequest<'_>) -> PublicationOutcome {
    publish_locked(request, || {})
}
pub(super) fn publish_locked(
    request: &PublicationRequest<'_>,
    after_lock: impl FnOnce(),
) -> PublicationOutcome {
    publish_transport(request, None, after_lock)
}
fn publish_transport(
    request: &PublicationRequest<'_>,
    settings: Option<&TrustedTransportSettings<'_>>,
    after_lock: impl FnOnce(),
) -> PublicationOutcome {
    use ProjectRemoteMode as M;
    use PublicationOutcome as O;
    if request.wire == Wire::LocalOnly && request.mode == M::LocalOnly {
        return O::LocalOnly;
    }
    if !policy_allowed(request) || !transport_allowed(request, settings) {
        return O::Refused;
    }
    if git::refuse_untrusted_repository_config(request.repository, "publish").is_err()
        || !publication_config_allowed(request.repository)
    {
        return O::Refused;
    }
    let source = format!("refs/heads/{}", request.branch);
    let Ok(_guard) = BranchLock::acquire(request.repository, &source) else {
        return O::Refused;
    };
    after_lock();
    let Ok(head) = git::capture(
        request.repository,
        "publish anchor",
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(&source),
        ],
    ) else {
        return O::Refused;
    };
    if !head.success || head.stdout.trim() != request.commit {
        return O::Refused;
    }
    let refspec = format!("{source}:{source}");
    // The fixed argv has no force mode. Git's receive side enforces fast-forward.
    match transport_capture(
        request.repository,
        &[
            OsStr::new("-c"),
            OsStr::new(protocol(request.remote)),
            OsStr::new("push"),
            OsStr::new("--porcelain"),
            OsStr::new("--"),
            OsStr::new(request.remote),
            OsStr::new(&refspec),
        ],
        settings,
    ) {
        Ok(result) if result.success => O::Published,
        _ => O::RemoteFailed,
    }
}

// Publication permits repository remote URLs/fetch mappings, but no command,
// signing, proxy, recursive-push or transport-policy overrides. Fixed command
// scope entries come from the hardened builder, never repository configuration.
fn publication_config_allowed(repository: &Path) -> bool {
    let Ok(listing) = git::capture(
        repository,
        "inspect publication configuration",
        &[
            OsStr::new("config"),
            OsStr::new("--null"),
            OsStr::new("--show-scope"),
            OsStr::new("--name-only"),
            OsStr::new("--get-regexp"),
            OsStr::new(
                r"^(remote|url|protocol|push|credential|http|gpg|ssh|submodule)\.|^core\.(sshcommand|gitproxy|alternaterefscommand|askpass)$",
            ),
        ],
    ) else {
        return false;
    };
    if !listing.success {
        return listing.exit_status == "exit status: 1" && listing.raw_stdout().is_empty();
    }
    let Some(fields) = listing.raw_stdout().strip_suffix(&[0]) else {
        return false;
    };
    let fields: Vec<_> = fields.split(|byte| *byte == 0).collect();
    fields.len().is_multiple_of(2)
        && fields.chunks_exact(2).all(|entry| {
            entry[0] == b"command"
                || (matches!(entry[0], b"local" | b"worktree")
                    && entry[1].starts_with(b"remote.")
                    && (entry[1].ends_with(b".url") || entry[1].ends_with(b".fetch")))
        })
}

// Git writers honor this per-ref lock, including when the ref is packed. It
// closes the anchor-check/push race for cooperating Git writers, not hostile
// filesystem mutation by a principal with repository write authority.
struct BranchLock {
    path: std::path::PathBuf,
    file: std::fs::File,
}
impl BranchLock {
    fn acquire(repository: &Path, reference: &str) -> Result<Self, ()> {
        let common = git::capture(
            repository,
            "publication common directory",
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--path-format=absolute"),
                OsStr::new("--git-common-dir"),
            ],
        )
        .map_err(|_| ())?
        .require_success("publication common directory")
        .map_err(|_| ())?;
        #[cfg(unix)]
        let root = {
            use std::os::unix::ffi::OsStrExt;
            let bytes = common.raw_stdout().strip_suffix(b"\n").ok_or(())?;
            std::fs::canonicalize(OsStr::from_bytes(bytes)).map_err(|_| ())?
        };
        #[cfg(not(unix))]
        let root = std::fs::canonicalize(common.stdout.trim()).map_err(|_| ())?;
        let path = root.join(format!("{reference}.lock"));
        std::fs::create_dir_all(path.parent().ok_or(())?).map_err(|_| ())?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| ())?;
        Ok(Self { path, file })
    }
}
impl Drop for BranchLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(owned), Ok(current)) =
                (self.file.metadata(), std::fs::symlink_metadata(&self.path))
                && owned.dev() == current.dev()
                && owned.ino() == current.ino()
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

/// Trusted composition only. File remotes use this compatibility entry point.
/// The acceptance reference must name the exact evidence checked by the caller;
/// neither this entry point nor State turns that reference into acceptance.
pub fn publish_durable(
    store: &mut receipts_state::TrustedCoreStore,
    admission: receipts_state::StateWriteAdmission,
    intent: &receipts_state::RemotePublicationIntentV1,
    request: &PublicationRequest<'_>,
    clock: &dyn receipts_state::TrustedClockV1,
) -> Result<receipts_state::RemotePublicationOutcomeV1, receipts_state::StateError> {
    publish_durable_with_transport(store, admission, intent, request, clock, None)
}
/// Durable publication with explicitly trusted network execution settings.
pub fn publish_durable_with_transport(
    store: &mut receipts_state::TrustedCoreStore,
    admission: receipts_state::StateWriteAdmission,
    intent: &receipts_state::RemotePublicationIntentV1,
    request: &PublicationRequest<'_>,
    clock: &dyn receipts_state::TrustedClockV1,
    settings: Option<&TrustedTransportSettings<'_>>,
) -> Result<receipts_state::RemotePublicationOutcomeV1, receipts_state::StateError> {
    use receipts_state::{RemotePublicationOutcomeV1 as O, SetupRefusalV1, StateError};
    let refused = || StateError::SetupRefused {
        reason: SetupRefusalV1::InvalidShape,
    };
    let repository = std::fs::canonicalize(request.repository).map_err(|_| refused())?;
    let remote = if request.remote.starts_with('/') {
        std::fs::canonicalize(request.remote)
            .map_err(|_| refused())?
            .to_str()
            .ok_or_else(refused)?
            .to_owned()
    } else {
        request.remote.to_owned()
    };
    let wire = match request.wire {
        Wire::LocalOnly => "LOCAL_ONLY",
        Wire::PushOnAccept => "PUSH_ON_ACCEPT",
        Wire::PushAlways => "PUSH_ALWAYS",
    };
    let mode = match request.mode {
        ProjectRemoteMode::LocalOnly => "LOCAL_ONLY",
        ProjectRemoteMode::PushA2Branches => "PUSH_A2_BRANCHES",
        ProjectRemoteMode::PushAcceptedA3 => "PUSH_ACCEPTED_A3",
        ProjectRemoteMode::PushAllCheckpoints => "PUSH_ALL_CHECKPOINTS",
    };
    let class = match request.class {
        PublicationBranchClass::Workstream => "WORKSTREAM",
        PublicationBranchClass::Task => "TASK",
        PublicationBranchClass::Checkpoint => "CHECKPOINT",
    };
    if repository.to_str() != Some(&intent.repository)
        || remote != intent.remote
        || intent.deletion_authorization.is_some()
        || intent.source_ref != format!("refs/heads/{}", request.branch)
        || intent.destination_ref != intent.source_ref
        || intent.commit != request.commit
        || intent.wire != wire
        || intent.mode != mode
        || intent.branch_class != class
    {
        return Err(refused());
    }
    let reservation = store.reserve_remote_publication_v1(admission, intent)?;
    let history = store
        .reader()
        .read_remote_publication_v1(&intent.project_id, &intent.operation_id)?
        .ok_or_else(refused)?;
    let blocked = if reservation == receipts_state::SetupWriteOutcomeV1::Replayed
        && history.outcome() == O::Uncertain
    {
        O::Uncertain
    } else {
        O::Refused
    };
    let mut sequence = history.results.len() as u64;
    let mut record = |outcome, observed: Option<&str>, evidence| {
        sequence += 1;
        let admission = store.admit(receipts_state::StateWriteSubjectV1 {
            operation: receipts_state::StateWriteOperationV1::RemotePublication,
            project_id: intent.project_id.clone(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        });
        store.append_remote_publication_result_v1(
            admission,
            &intent.project_id,
            &intent.operation_id,
            sequence,
            outcome,
            observed,
            evidence,
            clock,
        )?;
        Ok::<_, receipts_state::StateError>(outcome)
    };
    if request.wire == Wire::LocalOnly && request.mode == ProjectRemoteMode::LocalOnly {
        return record(blocked, None, "d12:local-only");
    }
    if !policy_allowed(request) || !transport_allowed(request, settings) {
        return record(blocked, None, "d12:policy-refused");
    }
    // Readback also runs before a retry: an interrupted/lost push response is
    // never evidence of remote failure. An unavailable read leaves uncertainty.
    if git::refuse_untrusted_repository_config(request.repository, "reconcile publication").is_err()
        || !publication_config_allowed(request.repository)
    {
        return record(blocked, None, "d12:configuration-refused");
    }
    match exact_remote_ref(request, &intent.destination_ref, settings) {
        Ok(Some(observed)) if observed == request.commit => {
            return record(O::Published, Some(&observed), "d12:exact-remote-readback");
        }
        Err(()) => return record(O::Uncertain, None, "d12:remote-readback-unavailable"),
        _ => {}
    }
    record(O::Uncertain, None, "d12:push-started")?;
    let outcome = publish_transport(request, settings, || {});
    if outcome == PublicationOutcome::Refused {
        return record(O::Refused, None, "d12:policy-refused");
    }
    match exact_remote_ref(request, &intent.destination_ref, settings) {
        Ok(Some(observed)) if observed == request.commit => {
            record(O::Published, Some(&observed), "d12:exact-remote-readback")
        }
        Ok(observed) => record(
            O::RemoteFailed,
            observed.as_deref(),
            "d12:exact-remote-mismatch",
        ),
        Err(()) => record(O::Uncertain, None, "d12:remote-readback-unavailable"),
    }
}
fn exact_remote_ref(
    request: &PublicationRequest<'_>,
    reference: &str,
    settings: Option<&TrustedTransportSettings<'_>>,
) -> Result<Option<String>, ()> {
    let output = transport_capture(
        request.repository,
        &[
            OsStr::new("-c"),
            OsStr::new(protocol(request.remote)),
            OsStr::new("ls-remote"),
            // A symbolic alias must never authorize mutation of its target.
            OsStr::new("--symref"),
            OsStr::new("--refs"),
            OsStr::new("--"),
            OsStr::new(request.remote),
            OsStr::new(reference),
        ],
        settings,
    )
    .map_err(|_| ())?;
    if !output.success {
        return Err(());
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let mut lines = output.stdout.lines();
    let line = lines.next().ok_or(())?;
    let (commit, actual) = line.split_once('\t').ok_or(())?;
    if actual != reference || CommitSha::parse(commit).is_err() || lines.next().is_some() {
        return Err(());
    }
    Ok(Some(commit.into()))
}

fn policy_allowed(request: &PublicationRequest<'_>) -> bool {
    use ProjectRemoteMode as M;
    use PublicationBranchClass as C;
    let eligible = match (request.wire, request.mode, request.class) {
        (Wire::PushOnAccept, M::PushA2Branches, C::Workstream)
        | (Wire::PushOnAccept, M::PushAcceptedA3, C::Workstream | C::Task) => {
            request.accepted_commit == Some(request.commit)
        }
        (Wire::PushAlways, M::PushAllCheckpoints, C::Checkpoint) => true,
        _ => false,
    };
    if !eligible
        || request.branch == "main"
        || request.branch == "master"
        || validate_branch_name(request.branch).is_err()
        || CommitSha::parse(request.commit).is_err()
        || request.remote.is_empty()
        || request.remote.starts_with('-')
    {
        return false;
    }
    true
}

/// Consumes explicit user direction. Only trusted composition may issue the State
/// capability; no worker input may call that issuer. Local branches are retained.
pub fn delete_remote_branch(
    store: &mut receipts_state::TrustedCoreStore,
    authorization: receipts_state::RemoteDeletionAuthorizationV1,
    request: &PublicationRequest<'_>,
    protected_refs: &[&str],
    clock: &dyn receipts_state::TrustedClockV1,
    settings: Option<&TrustedTransportSettings<'_>>,
) -> Result<receipts_state::RemotePublicationOutcomeV1, receipts_state::StateError> {
    deletion_run(
        store,
        authorization.intent(),
        request,
        protected_refs,
        clock,
        settings,
        true,
    )
}

/// Reopening an operation permits readback only; it never reuses authorization
/// for another destructive command, even when the remote still has the old SHA.
pub fn reconcile_remote_deletion(
    store: &mut receipts_state::TrustedCoreStore,
    project: &str,
    operation: &str,
    request: &PublicationRequest<'_>,
    clock: &dyn receipts_state::TrustedClockV1,
    settings: Option<&TrustedTransportSettings<'_>>,
) -> Result<receipts_state::RemotePublicationOutcomeV1, receipts_state::StateError> {
    let record = store
        .reader()
        .read_remote_publication_v1(project, operation)?
        .ok_or(receipts_state::StateError::SetupRefused {
            reason: receipts_state::SetupRefusalV1::InvalidShape,
        })?;
    deletion_run(store, &record.intent, request, &[], clock, settings, false)
}

#[allow(clippy::too_many_arguments)]
fn deletion_run(
    store: &mut receipts_state::TrustedCoreStore,
    intent: &receipts_state::RemotePublicationIntentV1,
    request: &PublicationRequest<'_>,
    protected_refs: &[&str],
    clock: &dyn receipts_state::TrustedClockV1,
    settings: Option<&TrustedTransportSettings<'_>>,
    execute: bool,
) -> Result<receipts_state::RemotePublicationOutcomeV1, receipts_state::StateError> {
    use receipts_state::{RemotePublicationOutcomeV1 as O, SetupRefusalV1, StateError};
    let invalid = || StateError::SetupRefused {
        reason: SetupRefusalV1::InvalidShape,
    };
    let reference = format!("refs/heads/{}", request.branch);
    let repository = std::fs::canonicalize(request.repository).map_err(|_| invalid())?;
    let remote = if request.remote.starts_with('/') {
        std::fs::canonicalize(request.remote)
            .map_err(|_| invalid())?
            .to_str()
            .ok_or_else(invalid)?
            .to_owned()
    } else {
        request.remote.to_owned()
    };
    if intent.deletion_authorization.is_none()
        || repository.to_str() != Some(&intent.repository)
        || remote != intent.remote
        || reference != intent.destination_ref
        || intent.commit != request.commit
    {
        return Err(invalid());
    }
    let record = store
        .reader()
        .read_remote_publication_v1(&intent.project_id, &intent.operation_id)?
        .ok_or_else(invalid)?;
    if record.intent != *intent {
        return Err(invalid());
    }
    if !execute && record.outcome() != O::Uncertain {
        return Ok(record.outcome());
    }
    let started = record
        .results
        .iter()
        .any(|r| r.evidence_ref == "d12:delete-started");
    // Local obstacles cannot establish the outcome of a started remote deletion.
    let local_refusal = if started && record.outcome() == O::Uncertain {
        O::Uncertain
    } else {
        O::Refused
    };
    let mut sequence = record.results.len() as u64;
    let mut append = |outcome, observed: Option<&str>, evidence| {
        sequence += 1;
        let admission = store.admit(receipts_state::StateWriteSubjectV1 {
            operation: receipts_state::StateWriteOperationV1::RemotePublication,
            project_id: intent.project_id.clone(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        });
        store.append_remote_publication_result_v1(
            admission,
            &intent.project_id,
            &intent.operation_id,
            sequence,
            outcome,
            observed,
            evidence,
            clock,
        )?;
        Ok::<_, StateError>(outcome)
    };
    if matches!(request.branch, "main" | "master")
        || protected_refs.contains(&reference.as_str())
        || validate_branch_name(request.branch).is_err()
    {
        return append(local_refusal, None, "d12:deletion-policy-refused");
    }
    if !transport_allowed(request, settings)
        || git::refuse_untrusted_repository_config(request.repository, "delete branch").is_err()
        || !publication_config_allowed(request.repository)
    {
        return append(local_refusal, None, "d12:deletion-configuration-refused");
    }
    let Ok(_guard) = BranchLock::acquire(request.repository, &reference) else {
        return append(local_refusal, None, "d12:deletion-ref-locked");
    };
    match exact_remote_ref(request, &reference, settings) {
        Err(()) => return append(O::Uncertain, None, "d12:deletion-readback-unavailable"),
        Ok(None) if started => return append(O::Deleted, None, "d12:deletion-readback-absent"),
        Ok(Some(observed))
            if observed == request.commit && execute && record.results.is_empty() => {}
        Ok(observed) => {
            return append(
                O::Refused,
                observed.as_deref(),
                "d12:deletion-moved-or-unused",
            );
        }
    }
    append(O::Uncertain, None, "d12:delete-started")?;
    // Conditional deletion closes the readback/delete race at the receive side.
    // This lease is deletion-only; publication still never uses a force option.
    let lease = format!("--force-with-lease={reference}:{}", request.commit);
    let refspec = format!(":{reference}");
    let _ = transport_capture(
        request.repository,
        &[
            OsStr::new("-c"),
            OsStr::new(protocol(request.remote)),
            OsStr::new("push"),
            OsStr::new("--porcelain"),
            OsStr::new(&lease),
            OsStr::new("--"),
            OsStr::new(request.remote),
            OsStr::new(&refspec),
        ],
        settings,
    );
    match exact_remote_ref(request, &reference, settings) {
        Ok(None) => append(O::Deleted, None, "d12:deletion-readback-absent"),
        Ok(observed) => append(
            O::RemoteFailed,
            observed.as_deref(),
            "d12:deletion-still-present",
        ),
        Err(()) => append(O::Uncertain, None, "d12:deletion-readback-unavailable"),
    }
}
