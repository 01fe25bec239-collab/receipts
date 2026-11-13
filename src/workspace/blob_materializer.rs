//! Immutable Git blob reads through the hardened, bounded Git runner.
use crate::{CommitSha, git, validate_write_path};
use receipts_state::context_rehydration::{
    ExternalSourceMaterialization, RepositorySnapshotMaterializer, RepositorySnapshotRefV1,
    SourceMaterializationFailure,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Trusted composition binds one project/repository to a local object store.
/// No checkout, filter, hook or remote operation is used.
pub struct GitBlobMaterializer {
    root: PathBuf,
    project: String,
    repository: String,
    materialized_at: String,
}
fn failure() -> SourceMaterializationFailure {
    SourceMaterializationFailure {
        code: "GIT_BLOB_REFUSED".into(),
        materializer_id: Some("workspace-git-blob-v1".into()),
        provenance: None,
        materialized_at: None,
        failure_detail: None,
    }
}
impl GitBlobMaterializer {
    pub fn new(
        root: &Path,
        project: &str,
        repository: &str,
        materialized_at: &str,
    ) -> Result<Self, SourceMaterializationFailure> {
        if project.is_empty()
            || repository.is_empty()
            || crate::WorkspaceDateTimeV1::try_new(materialized_at).is_err()
        {
            return Err(failure());
        }
        let root = std::fs::canonicalize(root).map_err(|_| failure())?;
        git::refuse_untrusted_repository_config(&root, "materializer configuration")
            .map_err(|_| failure())?;
        Ok(Self {
            root,
            project: project.into(),
            repository: repository.into(),
            materialized_at: materialized_at.into(),
        })
    }
    fn read(&self, args: &[&str]) -> Result<Vec<u8>, SourceMaterializationFailure> {
        git::refuse_untrusted_repository_config(&self.root, "materializer configuration")
            .map_err(|_| failure())?;
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        let capture = git::capture(&self.root, "materialize blob", &args)
            .map_err(|_| failure())?
            .require_success("materialize blob")
            .map_err(|_| failure())?;
        Ok(capture.raw_stdout().to_vec())
    }
    fn object(
        &self,
        reference: &RepositorySnapshotRefV1,
    ) -> Result<String, SourceMaterializationFailure> {
        if reference.project_id != self.project
            || reference.repository_id != self.repository
            || CommitSha::parse(&reference.commit_sha).is_err()
            || validate_write_path(&reference.logical_relative_path).is_err()
            || reference.logical_relative_path.contains(['\0', '\n', '\r'])
        {
            return Err(failure());
        }
        let kind = self.read(&["cat-file", "-t", &reference.commit_sha])?;
        if kind != b"commit\n" {
            return Err(failure());
        }
        let spec = format!(
            "{}:{}",
            reference.commit_sha, reference.logical_relative_path
        );
        let oid = self.read(&["rev-parse", "--verify", &spec])?;
        let oid = std::str::from_utf8(&oid).map_err(|_| failure())?.trim();
        CommitSha::parse(oid).map_err(|_| failure())?;
        if self.read(&["cat-file", "-t", oid])? != b"blob\n" {
            return Err(failure());
        }
        Ok(oid.into())
    }
}
impl RepositorySnapshotMaterializer for GitBlobMaterializer {
    fn known_length(
        &mut self,
        reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        let oid = self.object(reference)?;
        let bytes = self.read(&["cat-file", "-s", &oid])?;
        let size = std::str::from_utf8(&bytes)
            .map_err(|_| failure())?
            .trim()
            .parse()
            .map_err(|_| failure())?;
        Ok(Some(size))
    }
    fn materialize(
        &mut self,
        reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        let oid = self.object(reference)?;
        // The existing runner rejects truncated capture before any bytes reach State.
        let bytes = self.read(&["cat-file", "blob", &oid])?;
        for chunk in bytes.chunks(receipts_state::context_rehydration::MAX_STREAM_CHUNK_BYTES) {
            accept_chunk(chunk)?;
        }
        Ok(ExternalSourceMaterialization {
            materializer_id: "workspace-git-blob-v1".into(),
            provenance: format!(
                "commit:{};blob:{};path:{}",
                reference.commit_sha, oid, reference.logical_relative_path
            ),
            materialized_at: self.materialized_at.clone(),
        })
    }
}
