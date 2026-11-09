//! Orchestration's WriteScopePort bound to the real Workspace engine (S10).
use receipts_e2e::WorkspaceWriteScopeMatcher;
use receipts_orchestration::scheduler::{
    WriteScopeCollisionV1, WriteScopeOverlapV1, WriteScopePort, classify_write_collision,
};
use receipts_workspace_execution::WritePathPattern;

fn s(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

#[test]
fn disjoint_scopes_run_concurrently() {
    let m = WorkspaceWriteScopeMatcher;
    assert_eq!(
        m.overlap(&s(&["src/state/**"]), &s(&["src/core/**"])),
        WriteScopeOverlapV1::Disjoint
    );
    assert_eq!(
        classify_write_collision(&m, &s(&["docs/*.md"]), &s(&["src/**"])),
        WriteScopeCollisionV1::Concurrent
    );
}

#[test]
fn overlapping_scopes_conflict_with_a_witness_both_sets_accept() {
    let m = WorkspaceWriteScopeMatcher;
    let (left, right) = (s(&["src/**/*.rs"]), s(&["src/core/**"]));
    let WriteScopeCollisionV1::Conflict { witness } = classify_write_collision(&m, &left, &right)
    else {
        panic!("expected a conflict");
    };
    for pattern in left.iter().chain(&right) {
        assert!(
            WritePathPattern::parse(pattern).unwrap().matches(&witness),
            "{witness}"
        );
    }
}

#[test]
fn invalid_patterns_and_resource_limits_defer_never_admit() {
    let m = WorkspaceWriteScopeMatcher;
    assert_eq!(
        classify_write_collision(&m, &s(&["../escape/**"]), &s(&["src/**"])),
        WriteScopeCollisionV1::Defer
    );
    // 400 x 400 pairwise-disjoint patterns exceed the engine's work budget.
    let left: Vec<String> = (0..400).map(|i| format!("a/x{i}.rs")).collect();
    let right: Vec<String> = (0..400).map(|i| format!("b/y{i}.rs")).collect();
    assert_eq!(m.overlap(&left, &right), WriteScopeOverlapV1::Undetermined);
    assert_eq!(
        classify_write_collision(&m, &left, &right),
        WriteScopeCollisionV1::Defer
    );
}
