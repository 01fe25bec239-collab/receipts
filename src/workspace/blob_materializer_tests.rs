use crate::{GitBlobMaterializer, test_support::TestRepo};
use receipts_state::context_rehydration::{
    RepositorySnapshotMaterializer, RepositorySnapshotRefV1,
};

#[test]
fn blob_reads_immutable_git_objects_and_refuses_wrong_identity() {
    let repo = TestRepo::new("blob");
    let sha = repo.commit_file("source", "original\n");
    repo.commit_file("source", "changed\n");
    let mut reader =
        GitBlobMaterializer::new(repo.path(), "p", "r", "2026-11-11T00:00:00Z").unwrap();
    let mut reference = RepositorySnapshotRefV1 {
        project_id: "p".into(),
        repository_id: "r".into(),
        commit_sha: sha,
        logical_relative_path: "source".into(),
    };
    assert_eq!(reader.known_length(&reference).unwrap(), Some(9));
    let mut bytes = Vec::new();
    let receipt = reader
        .materialize(&reference, &mut |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
    assert_eq!(bytes, b"original\n");
    assert!(receipt.provenance.contains(&reference.commit_sha));
    reference.project_id = "foreign".into();
    assert!(reader.known_length(&reference).is_err());
    reference.project_id = "p".into();
    reference.commit_sha = "f".repeat(40);
    assert!(reader.known_length(&reference).is_err());
    reference.commit_sha = repo.head_sha();
    reference.logical_relative_path = "../source".into();
    assert!(reader.known_length(&reference).is_err());
}

#[test]
fn blob_refuses_oversize_before_delivering_partial_bytes() {
    let repo = TestRepo::new("blob-large");
    let sha = repo.commit_file("large", &"x".repeat(1_048_577));
    let mut reader =
        GitBlobMaterializer::new(repo.path(), "p", "r", "2026-11-11T00:00:00Z").unwrap();
    let reference = RepositorySnapshotRefV1 {
        project_id: "p".into(),
        repository_id: "r".into(),
        commit_sha: sha,
        logical_relative_path: "large".into(),
    };
    let mut delivered = 0;
    assert!(
        reader
            .materialize(&reference, &mut |chunk| {
                delivered += chunk.len();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(delivered, 0);
}
