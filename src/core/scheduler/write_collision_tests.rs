//! Collision resolutions go through apply-v1 authority; uncertainty refuses.

use super::*;
use crate::GraphNodeState::Planned;
use crate::dag::TaskDag;
use crate::test_support::*;
use crate::{
    GraphMutationActor, GraphMutationApplyError, GraphMutationScopeV1, GraphResultLedgerV1,
    GraphVersionV1, ScopedActorV1,
};

fn two_workstreams() -> crate::ExecutionGraphEnvelope {
    envelope(
        &[
            ("a", "TASK", Planned, Some("ws-a")),
            ("b", "TASK", Planned, Some("ws-b")),
            ("a2", "TASK", Planned, Some("ws-a")),
        ],
        &[],
    )
}

fn scope(actor: ScopedActorV1) -> GraphMutationScopeV1 {
    GraphMutationScopeV1::new("p", GRAPH, GraphVersionV1::try_new("1").unwrap(), actor)
}

fn a1() -> (GraphMutationScopeV1, GraphMutationActor) {
    (
        scope(ScopedActorV1::RuntimeA1 { role_id: None }),
        GraphMutationActor::try_new("RUNTIME_A1", None).unwrap(),
    )
}

fn resolve(
    env: &crate::ExecutionGraphEnvelope,
    scope: &GraphMutationScopeV1,
    actor: GraphMutationActor,
    left: (&str, &[String]),
    right: (&str, &[String]),
    resolution: WriteCollisionResolutionV1<'_>,
) -> Result<WriteCollisionResolvedV1, WriteCollisionResolutionErrorV1> {
    let results = GraphResultLedgerV1::new();
    resolve_write_collision_v1(
        &PORT,
        CollidingWriterV1 {
            node_id: left.0,
            write_paths: left.1,
        },
        CollidingWriterV1 {
            node_id: right.0,
            write_paths: right.1,
        },
        resolution,
        CollisionMutationV1 {
            envelope: env,
            results: &results,
            scope,
            actor,
            mutation_id: "m-serialize",
            edge_id: "e-serialize",
            created_at: time("2026-11-08T00:00:01Z"),
        },
    )
}

#[test]
fn serialize_adds_one_precedence_edge_and_holds_the_second_writer() {
    let env = two_workstreams();
    let (scope, actor) = a1();
    let core = paths(&["src/core/**"]);
    let file = paths(&["src/core/lib.rs"]);
    let resolved = resolve(
        &env,
        &scope,
        actor,
        ("a", &core),
        ("b", &file),
        WriteCollisionResolutionV1::Serialize {
            first: "a",
            second: "b",
        },
    )
    .unwrap();
    let WriteCollisionResolvedV1::Ordered {
        writer,
        consumer,
        mutation,
        outcome,
    } = resolved
    else {
        panic!("expected an ordering");
    };
    assert_eq!((writer.as_str(), consumer.as_str()), ("a", "b"));
    assert_eq!(mutation.resulting_version().as_str(), "2");
    let next = &outcome.envelope;
    assert_eq!(next.graph_version().as_str(), "2");
    let edge = next.graph().edge("e-serialize").unwrap();
    assert_eq!((edge.from_node(), edge.to_node()), ("a", "b"));
    assert!(!next.graph().has_precedence_cycle());
    let ready = TaskDag::new(next, None).ready_nodes().ready;
    assert!(ready.contains(&"a".to_string()) && !ready.contains(&"b".to_string()));
    // Inputs are unchanged.
    assert!(env.graph().edge("e-serialize").is_none());
}

#[test]
fn canonical_writer_is_the_provable_owner_of_the_overlap() {
    let env = two_workstreams();
    let (scope, actor) = a1();
    let ownership = [
        WorkstreamOwnershipV1 {
            workstream_id: "ws-a".into(),
            owned_paths: paths(&["src/core/**"]),
        },
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: paths(&["docs/**"]),
        },
    ];
    let (core, file) = (paths(&["src/core/**"]), paths(&["src/core/lib.rs"]));
    // b is listed first; the owner (a) still writes first.
    let resolved = resolve(
        &env,
        &scope,
        actor.clone(),
        ("b", &file),
        ("a", &core),
        WriteCollisionResolutionV1::CanonicalWriter {
            ownership: &ownership,
        },
    )
    .unwrap();
    assert!(matches!(
        resolved,
        WriteCollisionResolvedV1::Ordered { ref writer, ref consumer, .. }
            if writer == "a" && consumer == "b"
    ));

    // Neither or both owning the witness, or undecidable ownership, refuses.
    let both = [
        ownership[0].clone(),
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: paths(&["src/**"]),
        },
    ];
    let unknown = [
        ownership[0].clone(),
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: paths(&["sr?/**"]),
        },
    ];
    for (owners, expected) in [
        (&both[..], WriteCollisionResolutionErrorV1::NoCanonicalOwner),
        (
            &unknown[..],
            WriteCollisionResolutionErrorV1::OwnershipUndetermined,
        ),
        (
            &ownership[..1],
            WriteCollisionResolutionErrorV1::OwnershipMissing,
        ),
    ] {
        assert_eq!(
            resolve(
                &env,
                &scope,
                actor.clone(),
                ("a", &core),
                ("b", &file),
                WriteCollisionResolutionV1::CanonicalWriter { ownership: owners },
            ),
            Err(expected)
        );
    }
    // An undetermined collision has no witness to assign an owner for.
    let fuzzy = paths(&["src/c?re/**"]);
    assert_eq!(
        resolve(
            &env,
            &scope,
            actor,
            ("a", &core),
            ("b", &fuzzy),
            WriteCollisionResolutionV1::CanonicalWriter {
                ownership: &ownership
            },
        ),
        Err(WriteCollisionResolutionErrorV1::NoWitness)
    );
}

#[test]
fn defer_changes_nothing_and_disjoint_scopes_have_nothing_to_resolve() {
    let env = two_workstreams();
    let (scope, actor) = a1();
    let (core, docs, fuzzy) = (
        paths(&["src/core/**"]),
        paths(&["docs/**"]),
        paths(&["src/c?re/**"]),
    );
    assert_eq!(
        resolve(
            &env,
            &scope,
            actor.clone(),
            ("a", &core),
            ("b", &fuzzy),
            WriteCollisionResolutionV1::Defer
        ),
        Ok(WriteCollisionResolvedV1::Deferred)
    );
    for resolution in [
        WriteCollisionResolutionV1::Defer,
        WriteCollisionResolutionV1::Serialize {
            first: "a",
            second: "b",
        },
    ] {
        assert_eq!(
            resolve(
                &env,
                &scope,
                actor.clone(),
                ("a", &core),
                ("b", &docs),
                resolution
            ),
            Err(WriteCollisionResolutionErrorV1::NoCollision)
        );
    }
    assert_eq!(
        resolve(
            &env,
            &scope,
            actor,
            ("a", &core),
            ("b", &core),
            WriteCollisionResolutionV1::Serialize {
                first: "a",
                second: "a2"
            }
        ),
        Err(WriteCollisionResolutionErrorV1::NotTheCollidingPair)
    );
}

#[test]
fn serialize_requires_scope_authority_and_refuses_cycles() {
    let core = paths(&["src/core/**"]);
    // A RUNTIME_A2 may not add a cross-workstream edge.
    let env = two_workstreams();
    let a2 = scope(ScopedActorV1::RuntimeA2 {
        role_id: None,
        workstream_id: "ws-a".into(),
    });
    let a2_actor = GraphMutationActor::try_new("RUNTIME_A2", None).unwrap();
    assert!(matches!(
        resolve(
            &env,
            &a2,
            a2_actor.clone(),
            ("a", &core),
            ("b", &core),
            WriteCollisionResolutionV1::Serialize {
                first: "a",
                second: "b"
            }
        ),
        Err(WriteCollisionResolutionErrorV1::Apply(
            GraphMutationApplyError::Operation { .. }
        ))
    ));
    // Inside its own workstream it may.
    assert!(
        resolve(
            &env,
            &a2,
            a2_actor,
            ("a", &core),
            ("a2", &core),
            WriteCollisionResolutionV1::Serialize {
                first: "a",
                second: "a2"
            }
        )
        .is_ok()
    );
    // An actor that disagrees with the scope is refused by apply-v1.
    let (a1_scope, _) = a1();
    assert!(matches!(
        resolve(
            &env,
            &a1_scope,
            GraphMutationActor::try_new("USER", None).unwrap(),
            ("a", &core),
            ("b", &core),
            WriteCollisionResolutionV1::Serialize {
                first: "a",
                second: "b"
            }
        ),
        Err(WriteCollisionResolutionErrorV1::Apply(
            GraphMutationApplyError::Scope(_)
        ))
    ));
    // b already precedes a: ordering a before b would close a cycle.
    let chained = envelope(
        &[
            ("a", "TASK", Planned, Some("ws-a")),
            ("b", "TASK", Planned, Some("ws-b")),
        ],
        &[("e0", "b", "a")],
    );
    let (scope, actor) = a1();
    assert!(matches!(
        resolve(
            &chained,
            &scope,
            actor,
            ("a", &core),
            ("b", &core),
            WriteCollisionResolutionV1::Serialize {
                first: "a",
                second: "b"
            }
        ),
        Err(WriteCollisionResolutionErrorV1::Apply(_))
    ));
}
