//! ST-W2-2 materializer controls on real files: bounded reads, digest,
//! symlink/hard-link/escape/FIFO refusals, the registry's repository-blob
//! port (a fake Git blob port in tests only) and the changed-digest epoch
//! succession through a real rehydration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::context_epoch::ContextEpochTrigger;
use crate::context_manifest::{ContextSourceRefType, SourceClass};
use crate::context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, BoundContextSourceRefV1, ContextRehydrationStatus,
    ContextSourceBindingV1, ExternalSourceMaterialization, RepositorySnapshotMaterializer,
    RepositorySnapshotRefV1, SourceDigestComparison, SourceMaterializationFailure,
    context_source_digest_v1,
};
use crate::logical_role::LogicalRoleType;
use crate::materializers::{
    ARTIFACT_FILE_MATERIALIZER_ID, ArtifactCatalogEntryV1, ArtifactFileMaterializerV1,
    MaterializerRegistryV1,
};
use crate::recovery::ChangedSourceSuccessionRequestV1;
use crate::recovery_tests::{
    ARTIFACT_BYTES, PROJECT, Supplier, T_AFTER, T_BEFORE, authority, digest_of,
    rehydration_request, seeded,
};
use crate::tests::{state_epoch, trusted_clock_at};

struct Root(PathBuf);
impl Root {
    fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-st-w2-artifacts-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("docs")).unwrap();
        Self(path)
    }
    fn write(&self, relative: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(relative), bytes).unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn entry(id: &str, path: &str, bytes: &[u8]) -> ArtifactCatalogEntryV1 {
    ArtifactCatalogEntryV1 {
        artifact_id: id.into(),
        relative_path: path.into(),
        sha256: sha(bytes),
    }
}

fn reference(id: &str) -> ArtifactRefV1 {
    ArtifactRefV1 {
        project_id: PROJECT.into(),
        artifact_id: id.into(),
    }
}

/// Streams one artifact, returning the bytes or the failure code.
fn read(m: &mut ArtifactFileMaterializerV1<'_>, id: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut sink = |chunk: &[u8]| -> Result<(), SourceMaterializationFailure> {
        out.extend_from_slice(chunk);
        Ok(())
    };
    match m.materialize(&reference(id), &mut sink) {
        Ok(done) => {
            assert_eq!(done.materializer_id, ARTIFACT_FILE_MATERIALIZER_ID);
            assert_eq!(
                done.provenance,
                format!("artifact-file:sha256:{}", sha(&out))
            );
            assert_eq!(done.materialized_at, T_BEFORE);
            Ok(out)
        }
        Err(failure) => Err(failure.code),
    }
}

#[test]
fn bounded_artifact_read_streams_exact_bytes_and_checks_digest() {
    let root = Root::new("ok");
    let big = vec![7_u8; 200_000]; // several 64 KiB chunks
    root.write("docs/arch.md", ARTIFACT_BYTES);
    root.write("big.bin", &big);
    let clock = trusted_clock_at(T_BEFORE);
    let mut m = ArtifactFileMaterializerV1::new(
        &root.0,
        PROJECT,
        vec![
            entry("arch", "docs/arch.md", ARTIFACT_BYTES),
            entry("big", "big.bin", &big),
            entry(
                "stale",
                "docs/arch.md",
                b"what the catalog admitted earlier",
            ),
        ],
        1 << 20,
        &clock,
    )
    .unwrap();
    assert_eq!(read(&mut m, "arch").unwrap(), ARTIFACT_BYTES);
    assert_eq!(
        m.known_length(&reference("arch")).unwrap(),
        Some(ARTIFACT_BYTES.len() as u64)
    );
    assert_eq!(read(&mut m, "big").unwrap(), big);
    assert_eq!(
        read(&mut m, "stale").unwrap_err(),
        "ARTIFACT_DIGEST_MISMATCH"
    );
    assert_eq!(
        read(&mut m, "unknown").unwrap_err(),
        "ARTIFACT_NOT_REGISTERED"
    );
    let foreign = ArtifactRefV1 {
        project_id: "project-other".into(),
        artifact_id: "arch".into(),
    };
    assert_eq!(
        m.known_length(&foreign).unwrap_err().code,
        "ARTIFACT_PROJECT_MISMATCH"
    );
    std::fs::remove_file(root.0.join("big.bin")).unwrap();
    assert_eq!(read(&mut m, "big").unwrap_err(), "ARTIFACT_MISSING");
}

#[test]
fn materializer_bounded_read_refuses_oversize() {
    let root = Root::new("oversize");
    root.write("docs/arch.md", &[1_u8; 33]);
    let clock = trusted_clock_at(T_BEFORE);
    let mut m = ArtifactFileMaterializerV1::new(
        &root.0,
        PROJECT,
        vec![entry("arch", "docs/arch.md", &[1_u8; 33])],
        32,
        &clock,
    )
    .unwrap();
    assert_eq!(
        m.known_length(&reference("arch")).unwrap_err().code,
        "ARTIFACT_OVERSIZE"
    );
    assert_eq!(read(&mut m, "arch").unwrap_err(), "ARTIFACT_OVERSIZE");
    // The bound itself is capped by the per-source rehydration limit.
    assert!(
        ArtifactFileMaterializerV1::new(
            &root.0,
            PROJECT,
            Vec::new(),
            crate::context_rehydration::MAX_MATERIALIZED_BYTES_PER_SOURCE + 1,
            &clock,
        )
        .is_err()
    );
}

#[test]
fn materializer_refuses_symlink_escape() {
    let root = Root::new("escape");
    let outside = Root::new("outside");
    outside.write("secret.txt", b"outside the root");
    root.write("docs/arch.md", ARTIFACT_BYTES);
    std::os::unix::fs::symlink(outside.0.join("secret.txt"), root.0.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(&outside.0, root.0.join("linkdir")).unwrap();
    std::fs::hard_link(root.0.join("docs/arch.md"), root.0.join("hard.md")).unwrap();
    let clock = trusted_clock_at(T_BEFORE);
    let mut m = ArtifactFileMaterializerV1::new(
        &root.0,
        PROJECT,
        vec![
            entry("file-link", "link.txt", b"outside the root"),
            entry("dir-link", "linkdir/secret.txt", b"outside the root"),
            entry("hard", "hard.md", ARTIFACT_BYTES),
            entry("dir", "docs", b""),
        ],
        1 << 20,
        &clock,
    )
    .unwrap();
    assert_eq!(
        read(&mut m, "file-link").unwrap_err(),
        "ARTIFACT_SYMLINK_REFUSED"
    );
    assert_eq!(
        read(&mut m, "dir-link").unwrap_err(),
        "ARTIFACT_SYMLINK_REFUSED"
    );
    assert_eq!(
        read(&mut m, "hard").unwrap_err(),
        "ARTIFACT_HARD_LINK_REFUSED"
    );
    assert_eq!(
        read(&mut m, "dir").unwrap_err(),
        "ARTIFACT_NOT_REGULAR_FILE"
    );

    let fifo = std::ffi::CString::new(root.0.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: valid NUL-terminated path; creates a FIFO in the test root.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut m = ArtifactFileMaterializerV1::new(
        &root.0,
        PROJECT,
        vec![entry("pipe", "pipe", b"")],
        1 << 20,
        &clock,
    )
    .unwrap();
    assert_eq!(
        read(&mut m, "pipe").unwrap_err(),
        "ARTIFACT_NOT_REGULAR_FILE"
    );

    // Escaping or ambiguous catalog paths are refused before any read.
    for bad in [
        "../secret.txt",
        "/etc/passwd",
        "docs/../arch.md",
        "docs//arch.md",
        "./arch.md",
        "docs\\arch.md",
        "",
    ] {
        assert!(
            ArtifactFileMaterializerV1::new(
                &root.0,
                PROJECT,
                vec![entry("x", bad, b"")],
                64,
                &clock
            )
            .is_err(),
            "{bad:?} must be refused"
        );
    }
    assert!(
        ArtifactFileMaterializerV1::new(&root.0.join("link.txt"), PROJECT, Vec::new(), 64, &clock)
            .is_err(),
        "a root that is not a directory is refused"
    );
    assert!(
        ArtifactFileMaterializerV1::new(
            std::path::Path::new("relative"),
            PROJECT,
            Vec::new(),
            64,
            &clock
        )
        .is_err()
    );
}

struct FakeGitBlobs(Vec<u8>, usize);
impl RepositorySnapshotMaterializer for FakeGitBlobs {
    fn materialize(
        &mut self,
        _reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.1 += 1;
        accept_chunk(&self.0)?;
        Ok(ExternalSourceMaterialization {
            materializer_id: "test-only-fake-git-blob".into(),
            provenance: "fake".into(),
            materialized_at: T_AFTER.into(),
        })
    }
}

fn repo_ref() -> BoundContextSourceRefV1 {
    BoundContextSourceRefV1::RepositorySnapshot(RepositorySnapshotRefV1 {
        project_id: PROJECT.into(),
        repository_id: "repo-1".into(),
        commit_sha: "a".repeat(40),
        logical_relative_path: "docs/spec.md".into(),
    })
}

#[test]
fn changed_blob_digest_advances_epoch_and_invalidates_role() {
    let old = context_source_digest_v1(&repo_ref(), PROJECT, b"spec v1").unwrap();
    let (_tmp, mut repo) = seeded("st-w2-blob", LogicalRoleType::RuntimeA2, &old);
    // Replace the seeded artifact source with a repository blob source.
    let mut successor = repo
        .find_context_manifest("manifest-role-1")
        .unwrap()
        .unwrap();
    successor.manifest_id = "manifest-blob".into();
    successor.sources[0].r#ref.ref_type = ContextSourceRefType::RepoPath;
    successor.sources[0].r#ref.target = "docs/spec.md".into();
    successor.sources[0].source_class = SourceClass::Mandatory;
    repo.supersede_context_manifest(
        "role-1",
        &state_epoch(0),
        "manifest-role-1",
        successor,
        &crate::RoleChangeEvidenceV1 {
            changed_at: crate::CanonicalTimestampV1::parse(T_BEFORE).unwrap(),
            reason: None,
        },
    )
    .unwrap();
    let mut request = rehydration_request("attempt-unplugged", "manifest-blob", 0, "b1");
    request.source_bindings = vec![ContextSourceBindingV1 {
        source_ordinal: 0,
        source_id: "source-spec".into(),
        source_ref: repo_ref(),
    }];

    // Without a plugged repository port the source fails closed.
    let mut empty = MaterializerRegistryV1::new(None, None);
    let (r, a) = empty.ports();
    let failed = repo
        .rehydrate_context(
            request.clone(),
            r,
            a,
            &mut Supplier("01ARZ3NDEKTSV4RRFFQ69G5FA7"),
        )
        .unwrap();
    assert_eq!(failed.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        failed.attempt.failure_code.as_deref(),
        Some("MATERIALIZER_NOT_REGISTERED")
    );
    assert!(failed.sources.is_empty());

    // The registry accepts the stable repository-blob port (test fake only).
    let mut blobs = FakeGitBlobs(b"spec v2".to_vec(), 0);
    let mut registry = MaterializerRegistryV1::new(Some(&mut blobs), None);
    let (r, a) = registry.ports();
    request.rehydration_attempt_id = "attempt-blob".into();
    let outcome = repo
        .rehydrate_context(request, r, a, &mut Supplier("01ARZ3NDEKTSV4RRFFQ69G5FA8"))
        .unwrap();
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
    assert_eq!(
        outcome.attempt.source_evidence[0].comparison,
        SourceDigestComparison::Changed
    );
    assert_eq!(outcome.sources[0].bytes, b"spec v2");
    drop(registry);
    assert!(blobs.1 >= 1);

    let succession = repo
        .succeed_changed_context_sources(
            &trusted_clock_at(T_AFTER),
            &ChangedSourceSuccessionRequestV1 {
                role_id: "role-1".into(),
                rehydration_attempt_id: "attempt-blob".into(),
                successor_manifest_id: "manifest-blob-e1".into(),
                trigger: ContextEpochTrigger::ContractChange,
                authority: authority(),
            },
        )
        .unwrap();
    assert_eq!(succession.epoch.epoch, state_epoch(1));
    assert_eq!(succession.changed_sources[0].target, "docs/spec.md");
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids(PROJECT, 1)
            .unwrap(),
        Some(vec!["role-1".to_string()])
    );
    assert_eq!(
        repo.find_logical_role("role-1")
            .unwrap()
            .unwrap()
            .current_context_epoch,
        state_epoch(1)
    );
}

#[test]
fn real_artifact_file_rehydration_clears_continuation() {
    let digest = digest_of("artifact-arch", ARTIFACT_BYTES);
    let (_tmp, mut repo) = seeded("st-w2-file", LogicalRoleType::RuntimeA2, &digest);
    let root = Root::new("rehydrate");
    root.write("docs/arch.md", ARTIFACT_BYTES);
    let clock = trusted_clock_at(T_BEFORE);
    let mut files = ArtifactFileMaterializerV1::new(
        &root.0,
        PROJECT,
        vec![entry("artifact-arch", "docs/arch.md", ARTIFACT_BYTES)],
        1 << 20,
        &clock,
    )
    .unwrap();
    let mut registry = MaterializerRegistryV1::new(None, Some(&mut files));
    let (r, a) = registry.ports();
    let outcome = repo
        .rehydrate_context(
            rehydration_request("attempt-file", "manifest-role-1", 0, "b1"),
            r,
            a,
            &mut Supplier("01ARZ3NDEKTSV4RRFFQ69G5FA9"),
        )
        .unwrap();
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
    assert_eq!(outcome.sources[0].bytes, ARTIFACT_BYTES);
    assert_eq!(
        repo.continuation_clearance(&clock, "role-1")
            .unwrap()
            .rehydration_attempt_id,
        "attempt-file"
    );

    // Tampering after cataloguing fails the attempt loudly with no body.
    root.write("docs/arch.md", b"tampered");
    let (r, a) = registry.ports();
    let tampered = repo
        .rehydrate_context(
            rehydration_request("attempt-tampered", "manifest-role-1", 0, "b1"),
            r,
            a,
            &mut Supplier("01ARZ3NDEKTSV4RRFFQ69G5FAA"),
        )
        .unwrap();
    assert_eq!(tampered.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        tampered.attempt.failure_code.as_deref(),
        Some("ARTIFACT_DIGEST_MISMATCH")
    );
    assert!(tampered.sources.is_empty());
}
