use std::any::TypeId;

#[test]
fn root_and_graph_paths_expose_the_same_types() {
    assert_eq!(
        TypeId::of::<crate::GraphNodeState>(),
        TypeId::of::<crate::graph::GraphNodeState>()
    );
    assert_eq!(
        TypeId::of::<crate::GraphNode>(),
        TypeId::of::<crate::graph::GraphNode>()
    );
    assert_eq!(
        TypeId::of::<crate::GraphEdge>(),
        TypeId::of::<crate::graph::GraphEdge>()
    );
    assert_eq!(
        TypeId::of::<crate::ExecutionGraph>(),
        TypeId::of::<crate::graph::ExecutionGraph>()
    );
    assert_eq!(
        TypeId::of::<crate::GraphNodeResultOutcome>(),
        TypeId::of::<crate::graph::GraphNodeResultOutcome>(),
    );
    assert_eq!(
        TypeId::of::<crate::GraphNodeCheckResult>(),
        TypeId::of::<crate::graph::GraphNodeCheckResult>(),
    );
    assert_eq!(
        TypeId::of::<crate::GraphMutationOperationKind>(),
        TypeId::of::<crate::graph::GraphMutationOperationKind>(),
    );
}

#[test]
fn root_exports_preserve_the_frozen_graph_vocabulary() {
    assert_eq!(crate::GraphNodeState::ALL.len(), 15);
    assert_eq!(crate::GraphNodeResultOutcome::ALL.len(), 7);
    assert_eq!(crate::GraphNodeCheckResult::ALL.len(), 5);
    assert_eq!(crate::GraphMutationOperationKind::ALL.len(), 6);

    let lifecycle_edge_count = crate::AUTHORIZED_PREFIX_TRANSITIONS.len()
        + crate::AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len()
        + crate::AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len()
        + crate::AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len()
        + crate::AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS.len()
        + crate::AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS.len();
    assert_eq!(lifecycle_edge_count, 11);
    assert_eq!(
        crate::validate_prefix_transition(
            crate::GraphNodeState::Planned,
            crate::GraphNodeState::Ready,
        ),
        Ok(()),
    );
}
