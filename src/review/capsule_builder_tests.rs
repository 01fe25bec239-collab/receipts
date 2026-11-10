use crate::capsule_builder::*;
use crate::test_repo::*;
use crate::wire::{decode_review_request, encode_review_capsule, encode_review_request};
use crate::{AssuranceProfile, ReviewCapsuleCheckResult};
use receipts_workspace_execution::WorkspaceCheckpointRefType;

#[test]
fn capsule_binds_exact_sha() {
    let mut repo = Repo::new();
    let implementation = repo.commit(&[("src/status.txt", "ok\n")], "impl");
    let checks = [worker_check(
        &["true"],
        &implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    let request = request(
        &repo.base,
        &implementation,
        AssuranceProfile::Standard,
        None,
    );
    let capsule = build_review_capsule(ReviewCapsuleBuildInput {
        request: &request,
        repository_root: &repo.root(),
        worker_checks: &checks,
    })
    .unwrap();
    assert_eq!(capsule.implementation_sha().as_str(), implementation);
    assert_eq!(capsule.baseline_sha().as_str(), repo.base);
    assert_eq!(capsule.task_id(), "task-1");
    assert_eq!(capsule.attempt_id(), "attempt-1");
    assert_eq!(capsule.context_epoch().decimal_digits(), "3");
    assert_eq!(
        capsule.diff().ref_type(),
        WorkspaceCheckpointRefType::ArtifactId
    );
    assert_eq!(
        capsule.diff().target(),
        format!("git-diff:{}..{implementation}", repo.base)
    );
    assert!(capsule.diff().digest().unwrap().starts_with("sha256:"));
    assert!(
        capsule.reproduction_required(),
        "STANDARD requires reproduction"
    );
    assert_eq!(
        capsule.acceptance_criteria(),
        request.core().acceptance_criteria()
    );
    assert_eq!(capsule.checks().unwrap().len(), 1);
    // Same request and subject → same review identity; another SHA → another.
    assert_eq!(capsule.review_id(), review_id(&request));
    let other = repo.commit(&[("src/status.txt", "ok again\n")], "more");
    assert_ne!(
        review_id(&crate::test_repo::request(
            &repo.base,
            &other,
            AssuranceProfile::Standard,
            None
        )),
        capsule.review_id()
    );
}

#[test]
fn diff_digest_changes_with_content() {
    let mut repo = Repo::new();
    let a = repo.commit(&[("src/status.txt", "ok\n")], "a");
    let req_a = request(&repo.base, &a, AssuranceProfile::Light, None);
    let b = repo.commit(&[("src/status.txt", "ok!\n")], "b");
    let req_b = request(&repo.base, &b, AssuranceProfile::Light, None);
    let build = |r| {
        build_review_capsule(ReviewCapsuleBuildInput {
            request: r,
            repository_root: &repo.root(),
            worker_checks: &[],
        })
        .unwrap()
    };
    assert_ne!(build(&req_a).diff().digest(), build(&req_b).diff().digest());
    assert!(
        !build(&req_a).reproduction_required(),
        "LIGHT has no A4 reproduction"
    );
}

#[test]
fn missing_acceptance_criteria_refused() {
    let mut repo = Repo::new();
    let implementation = repo.commit(&[("src/status.txt", "ok\n")], "impl");
    // The request contract itself refuses an empty criteria list.
    let json = encode_review_request(&request(
        &repo.base,
        &implementation,
        AssuranceProfile::Standard,
        None,
    ));
    let start = json.find("\"acceptance_criteria\":[").unwrap() + "\"acceptance_criteria\":".len();
    let end = json.find("],\"allowed_write_paths\"").unwrap() + 1;
    let empty = format!("{}[]{}", &json[..start], &json[end..]);
    assert!(decode_review_request(&empty).is_err());
    // A deterministic criterion without argv cannot be reproduced: refused.
    let no_cmd = json.replacen(
        r#","check_command":["grep","-q","ok","src/status.txt"]"#,
        "",
        1,
    );
    let request = decode_review_request(&no_cmd).unwrap();
    assert_eq!(
        build_review_capsule(ReviewCapsuleBuildInput {
            request: &request,
            repository_root: &repo.root(),
            worker_checks: &[],
        }),
        Err(CapsuleBuildRefusal::DeterministicCriterionWithoutCommand {
            criterion_id: "c1".into()
        })
    );
}

#[test]
fn subject_and_check_binding_refusals() {
    let mut repo = Repo::new();
    let implementation = repo.commit(&[("src/status.txt", "ok\n")], "impl");
    let root = repo.root();
    let build = |r: &crate::ReviewRequest, checks: &[crate::ReviewCapsuleCheck]| {
        build_review_capsule(ReviewCapsuleBuildInput {
            request: r,
            repository_root: &root,
            worker_checks: checks,
        })
    };
    let missing = "c".repeat(40);
    assert!(matches!(
        build(
            &request(&repo.base, &missing, AssuranceProfile::Standard, None),
            &[]
        ),
        Err(CapsuleBuildRefusal::SubjectEvidence(_))
    ));
    // Baseline must be an ancestor of the implementation.
    assert!(matches!(
        build(
            &request(
                &implementation,
                &repo.base,
                AssuranceProfile::Standard,
                None
            ),
            &[]
        ),
        Err(CapsuleBuildRefusal::SubjectEvidence(_))
    ));
    let good = request(
        &repo.base,
        &implementation,
        AssuranceProfile::Standard,
        None,
    );
    let stale = [worker_check(
        &["true"],
        &repo.base,
        ReviewCapsuleCheckResult::Pass,
    )];
    assert_eq!(
        build(&good, &stale),
        Err(CapsuleBuildRefusal::CheckNotAtImplementationSha { index: 0 })
    );
    let mut broker = worker_check(&["true"], &implementation, ReviewCapsuleCheckResult::Pass);
    broker = crate::ReviewCapsuleCheck::new(
        receipts_workspace_execution::WorkspaceCheckpointExecutedCheckCore::new(
            receipts_workspace_execution::WorkspaceCheckpointCheckSource::ReviewExecution,
            broker.command().to_vec(),
            0,
            sha(&implementation),
            None,
            None,
        )
        .unwrap(),
        broker.result(),
    );
    assert_eq!(
        build(&good, &[broker]),
        Err(CapsuleBuildRefusal::CheckNotWorkerExecution { index: 0 })
    );
}

#[test]
fn capsule_has_no_field_for_a3_conversation() {
    // Compile-time: see the compile_fail doctests on `capsule_builder`.
    // Behavioural: the only inputs are request, repository and mechanical
    // checks, so implementer narrative cannot appear in the capsule.
    let mut repo = Repo::new();
    let implementation = repo.commit(&[("src/status.txt", "ok\n")], "SECRET-SELF-ASSESSMENT");
    let request = request(
        &repo.base,
        &implementation,
        AssuranceProfile::Standard,
        None,
    );
    let capsule = build_review_capsule(ReviewCapsuleBuildInput {
        request: &request,
        repository_root: &repo.root(),
        worker_checks: &[],
    })
    .unwrap();
    let json = encode_review_capsule(&capsule);
    assert!(!json.contains("SECRET-SELF-ASSESSMENT"));
    for forbidden in [
        "conversation",
        "implementation_summary",
        "open_questions",
        "security_notes",
    ] {
        assert!(!json.contains(forbidden), "{forbidden}");
    }
}
