//! D12 publication for trusted composition; never expose this boundary to workers.
use crate::{CommitSha, WorkspaceRemotePublishPolicy as Wire, git, validate_branch_name};
use std::{ffi::OsStr, path::Path};

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
    use ProjectRemoteMode as M;
    use PublicationBranchClass as C;
    use PublicationOutcome as O;
    if request.wire == Wire::LocalOnly && request.mode == M::LocalOnly {
        return O::LocalOnly;
    }
    let eligible = match (request.wire, request.mode, request.class) {
        (Wire::PushOnAccept, M::PushA2Branches, C::Workstream)
        | (Wire::PushOnAccept, M::PushAcceptedA3, C::Workstream | C::Task) => {
            request.accepted_commit == Some(request.commit)
        }
        (Wire::PushAlways, M::PushAllCheckpoints, _) => true,
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
        return O::Refused;
    }
    if git::refuse_untrusted_repository_config(request.repository, "publish").is_err() {
        return O::Refused;
    }
    let source = format!("refs/heads/{}", request.branch);
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
    match git::capture(
        request.repository,
        "publish",
        &[
            OsStr::new("-c"),
            OsStr::new("protocol.file.allow=always"),
            OsStr::new("push"),
            OsStr::new("--porcelain"),
            OsStr::new("--"),
            OsStr::new(request.remote),
            OsStr::new(&refspec),
        ],
    ) {
        Ok(result) if result.success => O::Published,
        _ => O::RemoteFailed,
    }
}
