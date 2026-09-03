use crate::{WorkspaceCheckpointRef, WorkspaceCheckpointRefError, WorkspaceCheckpointRefType};

#[test]
fn constructs_with_exact_required_shape() {
    let reference = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::RepoPath,
        "some/path".to_string(),
        None,
        None,
    )
    .expect("a non-empty target is valid");

    assert_eq!(reference.ref_type(), WorkspaceCheckpointRefType::RepoPath);
    assert_eq!(reference.target(), "some/path");
    assert_eq!(reference.digest(), None);
    assert_eq!(reference.section(), None);
}

#[test]
fn rejects_only_an_exactly_empty_target_for_every_ref_type() {
    for ref_type in WorkspaceCheckpointRefType::ALL {
        assert_eq!(
            WorkspaceCheckpointRef::new(ref_type, "", None, None),
            Err(WorkspaceCheckpointRefError::EmptyTarget)
        );
    }

    for target in [" ", "  value  ", "  refs/α path?q=1#frag  "] {
        let reference =
            WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::RepoPath, target, None, None)
                .expect("every non-empty target is valid");
        assert_eq!(reference.target(), target);
    }
}

#[test]
fn preserves_optional_digest_and_section_exactly() {
    for (digest, section) in [
        (None, None),
        (Some(""), Some("")),
        (Some("  SHA-256:ABC  "), Some("  stdout.tail  ")),
    ] {
        let reference = WorkspaceCheckpointRef::new(
            WorkspaceCheckpointRefType::ArtifactId,
            "target",
            digest.map(str::to_string),
            section.map(str::to_string),
        )
        .expect("the target is non-empty");
        assert_eq!(reference.digest(), digest);
        assert_eq!(reference.section(), section);
    }
}

#[test]
fn preserves_every_ref_type_and_clones_structurally() {
    for ref_type in WorkspaceCheckpointRefType::ALL {
        let reference = WorkspaceCheckpointRef::new(
            ref_type,
            "target",
            Some("digest".to_string()),
            Some("section".to_string()),
        )
        .expect("the target is non-empty");
        assert_eq!(reference.ref_type(), ref_type);
        assert_eq!(reference.clone(), reference);
    }
}

#[test]
fn empty_target_error_is_typed_and_standard() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<WorkspaceCheckpointRefError>();
    assert_eq!(
        WorkspaceCheckpointRefError::EmptyTarget.to_string(),
        "workspace checkpoint reference target is empty"
    );
}

#[test]
fn source_exposes_no_mutable_or_resolution_bypass() {
    let source = include_str!("checkpoint_reference.rs");
    for field in [
        "    ref_type: WorkspaceCheckpointRefType,",
        "    target: String,",
        "    digest: Option<String>,",
        "    section: Option<String>,",
    ] {
        assert!(source.contains(field));
    }
    for forbidden in [
        "pub target:",
        "pub digest:",
        "pub section:",
        "impl Default",
        "fn set_target",
        "fn target_mut",
        "fn resolve(",
        "fn read(",
        "fn fetch(",
        "fn load(",
        "fn query(",
        "fn validate_target(",
    ] {
        assert!(!source.contains(forbidden), "forbidden source: {forbidden}");
    }
}
