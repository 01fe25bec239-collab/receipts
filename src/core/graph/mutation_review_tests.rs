//! Apply-v1 review evidence: aggregate verdicts and acceptance rest on
//! independent review that trusted core admits, code SHAs are bound to the
//! admitted attempt, and repair provenance holds across calls. The repair
//! round here runs end to end without hand-seeded terminal states except
//! where a test says its seeds are structural-only graph data.

use serde_json::{Value, json};

use super::mutation_interpreter_tests::{
    SHA, a1, a2, attach, bare, control_edge, dispatch, edited, envelope, expand, node, ok, ok_ev,
    op_err, op_err_ev, prec_edge, rev, run_as, run_to_review, set, start, submit, task_env,
    verdict, verdict_rv, with_attempt,
};
use super::review_evidence_tests::{Rec, ReviewDir};
use crate::{
    AttemptAdmissionError, ExecutionGraphEnvelope, GraphEdge, GraphNodeAttemptNumber,
    GraphNodeKind, GraphNodeResultOutcome, GraphNodeState as S, GraphOperationError as O,
    GraphResultLedgerV1, PrecedenceKind, ReviewEvidenceRefusal as R, ReviewOutcome, ScopedActorV1,
};

const SHA2: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn attempt_no(node: crate::GraphNode, number: &str) -> crate::GraphNode {
    node.with_attempt_number(GraphNodeAttemptNumber::try_new(number).unwrap())
}

fn user() -> ScopedActorV1 {
    ScopedActorV1::User { role_id: None }
}

// --- repair provenance and the supported noncircular review round -----------

fn repair_node() -> Value {
    json!({"node_id":"i2","graph_id":"g","kind":"REPAIR","state":"PLANNED","workstream_id":"w","parent_node_id":"i1","attempt_number":2,"created_in_version":4})
}
fn review_node() -> Value {
    json!({"node_id":"r2","graph_id":"g","kind":"REVIEW","state":"PLANNED","workstream_id":"w","attempt_number":2,"created_in_version":4})
}

/// `i1` is an existing, already ACCEPTED source artifact; its post-acceptance
/// independent review `r1` can therefore actually run (`i1 → r1` is
/// REQUIRES_ACCEPTED and satisfied). `next` waits for the repair round.
fn source_env() -> ExecutionGraphEnvelope {
    envelope(
        vec![
            attempt_no(
                node("i1", GraphNodeKind::IMPLEMENTATION, S::Accepted)
                    .with_code_sha(SHA)
                    .unwrap(),
                "1",
            ),
            attempt_no(node("r1", GraphNodeKind::REVIEW, S::Planned), "1"),
            node("next", GraphNodeKind::TASK, S::Planned),
        ],
        vec![
            GraphEdge::precedence("e2", "g", "i1", "r1", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    )
}

/// Runs `id` as `attempt` to `RUNNING`, lets trusted core admit that attempt
/// as non-code (a SHA-less result alone never would), then applies `rest`.
fn run_non_code(
    env: &ExecutionGraphEnvelope,
    ledger: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    id: &str,
    attempt: &str,
    rest: Vec<Value>,
) -> (ExecutionGraphEnvelope, GraphResultLedgerV1) {
    let mut ops = run_as(id, attempt);
    let submit = ops.pop().unwrap();
    let (env, ledger) = ok(env, ledger, actor.clone(), Value::Array(ops));
    let ledger = ledger
        .admit_attempt_non_code("P", "g", id, attempt)
        .unwrap();
    let mut ops = vec![submit];
    ops.extend(rest);
    ok(&env, &ledger, actor, Value::Array(ops))
}

/// `r1` runs and rejects through apply: a retained REJECTED REVIEW with its
/// own result and attempt, no seeded terminal state. Its attempt is admitted
/// as non-code by trusted core.
fn rejected_review() -> (ExecutionGraphEnvelope, GraphResultLedgerV1) {
    run_non_code(
        &source_env(),
        &GraphResultLedgerV1::new(),
        a2("w"),
        "r1",
        "ar1",
        vec![
            attach(
                "r1",
                "res-r1",
                with_attempt(
                    bare(super::mutation_interpreter_tests::result(
                        "r1", "REJECTED", "rv-r1",
                    )),
                    "ar1",
                ),
            ),
            verdict("r1", "REJECTED", "r1"),
        ],
    )
}

fn round_expansion() -> Value {
    expand(
        "r1",
        vec![repair_node(), review_node()],
        vec![
            control_edge("c1", "r1", "i2"),
            prec_edge("p1", "i2", "r2"),
            prec_edge("p2", "r2", "next"),
        ],
    )
}

#[test]
fn repair_expansion_refuses_incomplete_or_wrong_provenance() {
    let (env, results) = rejected_review();
    let (repair, review) = (repair_node(), review_node());
    let (c1, p1, p2) = (
        control_edge("c1", "r1", "i2"),
        prec_edge("p1", "i2", "r2"),
        prec_edge("p2", "r2", "next"),
    );
    let no_version = json!({"created_in_version": null});
    let other_repair = edited(repair.clone(), json!({"node_id":"i3"}));
    let other_review = edited(review.clone(), json!({"node_id":"r3"}));
    let cases: Vec<(&str, &str, Value)> = vec![
        // The cb2 reproduction: no parent and no REVIEW at all.
        (
            "no parent, no review",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![edited(repair.clone(), json!({"parent_node_id": null}))],
                vec![c1.clone()],
            ),
        ),
        (
            "missing parent",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id": null})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "parent is the old review, not the reviewed implementation",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id":"r1"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "parent is an unrelated node",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id":"next"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "no fresh review",
            "exactly one REPAIR and one REVIEW",
            expand("r1", vec![repair.clone()], vec![c1.clone()]),
        ),
        (
            "orphan review (no repair → review precedence)",
            "one distinct fresh REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p2.clone()],
            ),
        ),
        (
            "review precedes the repair instead of following it",
            "one distinct fresh REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), prec_edge("p1", "r2", "i2")],
            ),
        ),
        (
            "repair points back at the old review",
            "may not touch the rejected source",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p1.clone(), prec_edge("p3", "i2", "r1")],
            ),
        ),
        (
            "old review reused as the new review",
            "DuplicateNodeId",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"node_id":"r1"})),
                ],
                vec![c1.clone()],
            ),
        ),
        (
            "two repairs",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![repair.clone(), other_repair, review.clone()],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "two reviews",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone(), other_review],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "the accepted implementation as the source",
            "source must be a REJECTED REVIEW",
            expand(
                "i1",
                vec![repair.clone(), review.clone()],
                vec![control_edge("c1", "i1", "i2"), p1.clone()],
            ),
        ),
        (
            "no ON_REJECT edge",
            "missing ON_REJECT",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![p1.clone(), p2.clone()],
            ),
        ),
        (
            "ON_REJECT from the implementation, not the review",
            "missing ON_REJECT",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![control_edge("c1", "i1", "i2"), p1.clone()],
            ),
        ),
        (
            "unversioned review",
            "created_in_version",
            expand(
                "r1",
                vec![repair.clone(), edited(review.clone(), no_version)],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "repair created in the wrong version",
            "created_in_version",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"created_in_version":5})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "child names another graph",
            "ChildGraphIdMismatch",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"graph_id":"h"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "precedence cycle through the new round",
            "PrecedenceCycleRejected",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![
                    c1.clone(),
                    p1.clone(),
                    p2.clone(),
                    prec_edge("p4", "next", "i2"),
                ],
            ),
        ),
        (
            "repair in another workstream than the subject",
            "workstream binding",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"workstream_id":"x"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "review in another workstream than the repair",
            "workstream binding",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"workstream_id":"x"})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "repair round does not follow the subject's",
            "round ordinals",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"attempt_number":1})),
                    edited(review.clone(), json!({"attempt_number":1})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "review round differs from the repair's",
            "round ordinals",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"attempt_number":3})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "second execution subject also precedes the new review",
            "subject is ambiguous",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p1.clone(), prec_edge("p5", "i1", "r2")],
            ),
        ),
    ];
    for (label, reason, op) in cases {
        let (index, error) = op_err(&env, &results, a2("w"), json!([op]));
        assert_eq!(index, 0, "{label}");
        assert!(format!("{error:?}").contains(reason), "{label}: {error:?}");
    }
    // Ambiguous or missing subject linkage on the retained review.
    for (label, reason, edges) in [
        ("no subject", "no reviewed subject", vec![]),
        (
            "two subjects",
            "subject is ambiguous",
            vec![
                GraphEdge::precedence("a", "g", "x", "rv", PrecedenceKind::RequiresAccepted)
                    .unwrap(),
                GraphEdge::precedence("b", "g", "y", "rv", PrecedenceKind::RequiresAccepted)
                    .unwrap(),
            ],
        ),
    ] {
        let env = envelope(
            vec![
                node("x", GraphNodeKind::TASK, S::Accepted),
                node("y", GraphNodeKind::IMPLEMENTATION, S::Accepted),
                node("rv", GraphNodeKind::REVIEW, S::Rejected),
            ],
            edges,
            "1",
        );
        let repair = edited(
            repair_node(),
            json!({"parent_node_id":"x","created_in_version":2}),
        );
        let review = edited(review_node(), json!({"created_in_version":2}));
        let op = expand(
            "rv",
            vec![repair, review],
            vec![control_edge("c1", "rv", "i2"), prec_edge("p1", "i2", "r2")],
        );
        let (_, error) = op_err(&env, &GraphResultLedgerV1::new(), a1(), json!([op]));
        assert!(format!("{error:?}").contains(reason), "{label}: {error:?}");
    }
    // The retained rejected review is untouched by every refusal above.
    assert_eq!(env.graph().node_state("r1"), Some(S::Rejected));
}

/// A repair smuggled in through ADD_NODE/ADD_EDGE cannot be entered without
/// the same provenance. The seeded REJECTED states are structural-only graph
/// data: no valid history reaches REPAIR_ENTRY for an aggregate whose review
/// waits on its acceptance.
#[test]
fn a_smuggled_repair_cannot_be_entered() {
    let env = envelope(
        vec![
            attempt_no(node("s", GraphNodeKind::TASK, S::Rejected), "1"),
            node("rj", GraphNodeKind::REVIEW, S::Rejected),
        ],
        vec![
            GraphEdge::precedence("pr", "g", "s", "rj", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    );
    let repair = edited(
        repair_node(),
        json!({"node_id":"x","parent_node_id":"s","created_in_version":2}),
    );
    let smuggle = json!([
        {"op":"ADD_NODE","node_id":"x","detail":{"node":repair}},
        {"op":"ADD_EDGE","edge_id":"cx","detail":{"edge":control_edge("cx", "rj", "x")}},
        set("s", "REJECTED", "REPAIRING", json!({"kind":"REPAIR_ENTRY","repair_node_id":"x"})),
    ]);
    let (index, error) = op_err(&env, &GraphResultLedgerV1::new(), a1(), smuggle);
    assert_eq!(index, 2);
    assert!(
        format!("{error:?}").contains("one distinct fresh REVIEW"),
        "{error:?}"
    );
}

/// The supported noncircular path. `r1` (reviewing the accepted source `i1`)
/// rejects; the retained REJECTED review expands into a fresh repair `i2` and
/// a distinct fresh review `r2`. The repair's own pre-acceptance review is
/// independent evidence admitted from outside the graph: `r2` waits for
/// `i2`'s acceptance (REQUIRES_ACCEPTED, unchanged) and so cannot be the
/// prerequisite of that same acceptance. `r2` then completes its own,
/// post-acceptance round.
#[test]
fn repair_review_round_completes_without_circular_acceptance() {
    let (env, results) = rejected_review();
    let (env, results) = ok(&env, &results, a2("w"), json!([round_expansion()]));
    assert!(!env.graph().has_precedence_cycle());
    assert_eq!(env.graph().node("i2").unwrap().parent_node_id(), Some("i1"));
    // The same rejected review cannot be expanded a second time.
    let again = expand(
        "r1",
        vec![
            edited(
                repair_node(),
                json!({"node_id":"i3","created_in_version":5}),
            ),
            edited(
                review_node(),
                json!({"node_id":"r3","created_in_version":5}),
            ),
        ],
        vec![control_edge("c9", "r1", "i3"), prec_edge("p9", "i3", "r3")],
    );
    assert!(matches!(
        op_err(&env, &results, a2("w"), json!([again])).1,
        O::Repair(_)
    ));
    // The retained verdict cannot be reopened or revised.
    assert_eq!(
        op_err(
            &env,
            &results,
            a1(),
            json!([set(
                "r1",
                "REJECTED",
                "PASSED",
                json!({"kind":"REVIEW_VERDICT","result_node_id":"r1"})
            )])
        )
        .1,
        O::RetainedVerdict
    );

    // The repair runs as its own attempt and, with no commit known in the
    // graph, only trusted core can bind the commit it produced.
    let (env, results) = ok(&env, &results, a2("w"), Value::Array(run_as("i2", "ai2")));
    let claim = |sha: Option<&str>, attempt: &str| {
        let mut r = with_attempt(
            super::mutation_interpreter_tests::result("i2", "PASS", "claimed-rv"),
            attempt,
        );
        match sha {
            Some(sha) => r["code_sha"] = json!(sha),
            None => r = bare(r),
        }
        r
    };
    // Neither a code claim nor its omission classifies the attempt.
    for sha in [Some(SHA2), None] {
        assert_eq!(
            op_err(
                &env,
                &results,
                a2("w"),
                json!([attach("i2", "res-i2", claim(sha, "ai2"))])
            )
            .1,
            O::EvidenceUnavailable("attempt code or non-code classification"),
            "{sha:?}"
        );
    }
    let admitted = results
        .admit_attempt_code_sha("P", "g", "i2", "ai2", SHA2)
        .unwrap();
    assert_eq!(admitted.attempt_code_sha("P", "g", "i2"), Some(SHA2));
    assert_eq!(
        op_err(
            &env,
            &admitted,
            a2("w"),
            json!([attach("i2", "res-i2", claim(Some(SHA), "ai2"))])
        )
        .1,
        O::ResultAssociation("code_sha")
    );
    let (env, results) = ok(
        &env,
        &admitted,
        a2("w"),
        json!([attach("i2", "res-i2", claim(Some(SHA2), "ai2"))]),
    );

    // The worker's PASS and the review id it names are not review evidence.
    let judge = verdict_rv("i2", "PASSED", "i2", "claimed-rv");
    assert_eq!(
        op_err(&env, &results, a2("w"), json!([judge.clone()])).1,
        O::Review(R::NoSource)
    );
    let dir = ReviewDir::new("round");
    assert_eq!(
        op_err_ev(&env, &results, a2("w"), json!([judge]), rev(&dir)).1,
        O::Review(R::MissingRecord)
    );
    let good = Rec::new("rv-2", "i2", "ai2", SHA2, "2");
    let judge = verdict_rv("i2", "PASSED", "i2", "rv-2");
    for (label, rec, expect) in [
        (
            "an earlier attempt's review (old round)",
            Rec {
                attempt: "ar1".into(),
                ..good.clone()
            },
            O::Review(R::StaleAttempt),
        ),
        (
            "another commit",
            Rec {
                code_sha: SHA.into(),
                ..good.clone()
            },
            O::Review(R::CodeMismatch),
        ),
        (
            "old round number",
            Rec {
                round: "1".into(),
                ..good.clone()
            },
            O::Review(R::RoundMismatch),
        ),
        (
            "a rejection for a PASSED verdict",
            good.clone().rejected(),
            O::Review(R::OutcomeMismatch),
        ),
        (
            "another subject",
            Rec {
                subject: "i1".into(),
                ..good.clone()
            },
            O::Review(R::WrongSubject),
        ),
        (
            "not admitted by the source",
            Rec {
                admitted: String::new(),
                ..good.clone()
            },
            O::Review(R::NotAdmitted),
        ),
    ] {
        dir.put(&rec);
        assert_eq!(
            op_err_ev(&env, &results, a2("w"), json!([judge.clone()]), rev(&dir)).1,
            expect,
            "{label}"
        );
    }
    dir.put(&good);
    dir.tamper("rv-2", b"approved by the author");
    assert_eq!(
        op_err_ev(&env, &results, a2("w"), json!([judge.clone()]), rev(&dir)).1,
        O::Review(R::ContentMismatch)
    );
    // The review identity cannot be the work's own attempt.
    let selfie = Rec::new("ai2", "i2", "ai2", SHA2, "2");
    dir.put(&selfie);
    assert_eq!(
        op_err_ev(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("i2", "PASSED", "i2", "ai2")]),
            rev(&dir)
        )
        .1,
        O::Review(R::SelfReview)
    );
    // The old rejected review cannot promote the repair, and neither can the
    // fresh review that waits for this very acceptance.
    for (id, why) in [
        ("r1", "graph review has no matching verdict"),
        ("r2", "review requires the acceptance it would gate"),
    ] {
        dir.put(&Rec::new(id, "i2", "ai2", SHA2, "2"));
        assert_eq!(
            op_err_ev(
                &env,
                &results,
                a2("w"),
                json!([verdict_rv("i2", "PASSED", "i2", id)]),
                rev(&dir)
            )
            .1,
            O::EvidenceRefused(why),
            "{id}"
        );
    }
    // Acceptance of unjudged work and the dependent review both wait.
    let accept = set(
        "i2",
        "PASSED",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"d-i2"}),
    );
    let ready_r2 = set(
        "r2",
        "PLANNED",
        "READY",
        json!({"kind":"DEPENDENCIES_SATISFIED"}),
    );
    assert_eq!(
        op_err(&env, &results, a1(), json!([ready_r2.clone()])).1,
        O::EvidenceRefused("dependencies")
    );

    dir.put(&good);
    let (env, results) = ok_ev(&env, &results, a2("w"), json!([judge]), rev(&dir));
    assert_eq!(env.graph().node_state("i2"), Some(S::Passed));
    assert_eq!(
        results.review("P", "g", "i2"),
        Some(("rv-2", ReviewOutcome::Pass))
    );
    // The explicit acceptance is A1's, on the retained, exactly bound review.
    assert!(matches!(
        op_err(&env, &results, a2("w"), json!([accept.clone()])).1,
        O::NotPermitted(_)
    ));
    let (env, results) = ok(&env, &results, a1(), json!([accept]));
    assert_eq!(env.graph().node_state("i2"), Some(S::Accepted));

    // r2 is a different review, now actually runnable, with its own attempt
    // (admitted as non-code by trusted core).
    let (env, results) = run_non_code(
        &env,
        &results,
        a1(),
        "r2",
        "ar2",
        vec![
            attach(
                "r2",
                "res-r2",
                with_attempt(
                    bare(super::mutation_interpreter_tests::result(
                        "r2", "PASS", "rv-r2",
                    )),
                    "ar2",
                ),
            ),
            verdict("r2", "PASSED", "r2"),
            set(
                "r2",
                "PASSED",
                "ACCEPTED",
                json!({"kind":"ACCEPTANCE","decision_id":"d-r2"}),
            ),
            set(
                "next",
                "PLANNED",
                "READY",
                json!({"kind":"DEPENDENCIES_SATISFIED"}),
            ),
        ],
    );
    assert_eq!(env.graph().node_state("r2"), Some(S::Accepted));
    assert_eq!(env.graph().node_state("next"), Some(S::Ready));

    // History retained: the rejected review, its result, and every attempt.
    assert_eq!(env.graph().node_state("r1"), Some(S::Rejected));
    assert_eq!(env.graph().node("r1").unwrap().result_ref(), Some("res-r1"));
    assert_eq!(
        results.get("res-r1").unwrap().outcome(),
        GraphNodeResultOutcome::Rejected
    );
    assert_eq!(env.graph().node_state("i1"), Some(S::Accepted));
    for (node, attempt) in [("r1", "ar1"), ("i2", "ai2"), ("r2", "ar2")] {
        assert_eq!(results.attempt("P", "g", node), Some(attempt));
    }
    assert_eq!(results.len(), 3);
}

// --- evidence-backed verdicts and acceptance ---------------------------------

#[test]
fn self_asserted_pass_and_review_id_do_not_promote_work() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let mut ops = run_to_review("t");
    ops.push(attach(
        "t",
        "res",
        super::mutation_interpreter_tests::result("t", "PASS", "claimed-by-the-worker"),
    ));
    let (env, results) = ok(&env, &none, a2("w"), Value::Array(ops));
    // The pre-evidence call shape and a verdict that names no review.
    assert_eq!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("t", "PASSED", "t", "claimed-by-the-worker")])
        )
        .1,
        O::Review(R::NoSource)
    );
    assert_eq!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict("t", "PASSED", "t")])
        )
        .1,
        O::Detail("review_id")
    );
    // A source that knows no such review.
    let dir = ReviewDir::new("selfassert");
    assert_eq!(
        op_err_ev(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("t", "PASSED", "t", "claimed-by-the-worker")]),
            rev(&dir)
        )
        .1,
        O::Review(R::MissingRecord)
    );
    assert_eq!(env.graph().node_state("t"), Some(S::AwaitingReview));
}

#[test]
fn review_evidence_is_consumed_once_and_rejection_is_evidence_too() {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap(),
            node("u", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap(),
        ],
        vec![],
        "1",
    );
    let mut ops = run_as("t", "at1");
    ops.extend(run_as("u", "au1"));
    for (id, attempt) in [("t", "at1"), ("u", "au1")] {
        ops.push(attach(
            id,
            &format!("res-{id}"),
            with_attempt(
                super::mutation_interpreter_tests::result(id, "PASS", "claimed"),
                attempt,
            ),
        ));
    }
    let (env, results) = ok(
        &env,
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    );
    let dir = ReviewDir::new("consumed");
    dir.put(&Rec::new("rv-t", "t", "at1", SHA, "").rejected());
    let (env, results) = ok_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("t", "REJECTED", "t", "rv-t")]),
        rev(&dir),
    );
    assert_eq!(env.graph().node_state("t"), Some(S::Rejected));
    assert_eq!(
        results.review("P", "g", "t"),
        Some(("rv-t", ReviewOutcome::Rejected))
    );
    // A rejected node is never accepted on that review.
    assert!(matches!(
        op_err(
            &env,
            &results,
            a1(),
            json!([set(
                "t",
                "REJECTED",
                "ACCEPTED",
                json!({"kind":"ACCEPTANCE","decision_id":"d"})
            )])
        )
        .1,
        O::Transition(_)
    ));
    // Another node cannot reuse the consumed identity, whatever it records.
    dir.put(&Rec::new("rv-t", "u", "au1", SHA, ""));
    assert_eq!(
        op_err_ev(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("u", "PASSED", "u", "rv-t")]),
            rev(&dir)
        )
        .1,
        O::EvidenceRefused("review identity already used")
    );
}

#[test]
fn a_failed_batch_consumes_no_review_evidence() {
    let env = task_env();
    let mut ops = run_to_review("t");
    ops.push(attach(
        "t",
        "res",
        super::mutation_interpreter_tests::result("t", "PASS", "claimed"),
    ));
    let (env, results) = ok(
        &env,
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    );
    let dir = ReviewDir::new("failed-batch");
    dir.put(&Rec::new("rv1", "t", "at1", SHA, ""));
    let accept = set(
        "t",
        "PASSED",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"d"}),
    );
    // Good verdict, then a refused acceptance by the same actor.
    let (index, error) = op_err_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("t", "PASSED", "t", "rv1"), accept]),
        rev(&dir),
    );
    assert_eq!(index, 1);
    assert!(matches!(error, O::NotPermitted(_)));
    // The identity was not consumed: the same verdict still succeeds.
    let (env, results) = ok_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("t", "PASSED", "t", "rv1")]),
        rev(&dir),
    );
    assert_eq!(env.graph().node_state("t"), Some(S::Passed));
    assert_eq!(
        results.review("P", "g", "t"),
        Some(("rv1", ReviewOutcome::Pass))
    );
}

#[test]
fn acceptance_needs_the_admitted_pass_review() {
    // Graph data that merely claims PASSED has no review history.
    let env = envelope(
        vec![
            node("p", GraphNodeKind::TASK, S::Passed),
            node("rv", GraphNodeKind::REVIEW, S::Passed),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    let accept = |id: &str| {
        set(
            id,
            "PASSED",
            "ACCEPTED",
            json!({"kind":"ACCEPTANCE","decision_id":"d"}),
        )
    };
    assert_eq!(
        op_err(&env, &none, a1(), json!([accept("p")])).1,
        O::Review(R::MissingRecord)
    );
    // A review node's own verdict needs no outside review to be accepted.
    ok(&env, &none, user(), json!([accept("rv")]));
}

// --- code SHA binding ------------------------------------------------------

#[test]
fn code_sha_is_bound_to_the_admitted_attempt() {
    let env = envelope(
        vec![
            node("w", GraphNodeKind::TASK, S::Admitted),
            node("t", GraphNodeKind::TASK, S::Admitted)
                .with_code_sha(SHA)
                .unwrap(),
        ],
        vec![],
        "1",
    );
    let started = |id: &str, attempt: &str| vec![dispatch(id, attempt), start(id, attempt)];
    let mut ops = started("w", "aw1");
    ops.extend(started("t", "at1"));
    let (env, ledger) = ok(&env, &GraphResultLedgerV1::new(), a1(), Value::Array(ops));
    let result_for = |id: &str, attempt: &str, sha: Option<&str>| {
        let mut r = with_attempt(
            super::mutation_interpreter_tests::result(id, "PASS", "rv"),
            attempt,
        );
        match sha {
            Some(sha) => r["code_sha"] = json!(sha),
            None => r = bare(r),
        }
        attach(id, "r", r)
    };
    // A node with no known SHA is unclassified: neither a code claim nor its
    // omission makes the attempt code or non-code, so both are unavailable
    // until trusted core admits one.
    for claim in [Some(SHA), None] {
        assert_eq!(
            op_err(&env, &ledger, a1(), json!([result_for("w", "aw1", claim)])).1,
            O::EvidenceUnavailable("attempt code or non-code classification"),
            "{claim:?}"
        );
    }
    // Explicit non-code admission: a SHA-less result is fine with no invented
    // SHA, a code claim is not, and the SHA cannot be admitted afterwards.
    let non_code = ledger.admit_attempt_non_code("P", "g", "w", "aw1").unwrap();
    assert!(non_code.attempt_is_non_code("P", "g", "w"));
    assert!(
        !ledger.attempt_is_non_code("P", "g", "w"),
        "input unchanged"
    );
    assert_eq!(non_code.attempt_code_sha("P", "g", "w"), None);
    ok(&env, &non_code, a1(), json!([result_for("w", "aw1", None)]));
    assert_eq!(
        op_err(
            &env,
            &non_code,
            a1(),
            json!([result_for("w", "aw1", Some(SHA))])
        )
        .1,
        O::ResultAssociation("code_sha")
    );
    assert_eq!(
        non_code.admit_attempt_code_sha("P", "g", "w", "aw1", SHA),
        Err(AttemptAdmissionError::DomainConflict)
    );
    assert_eq!(
        non_code.admit_attempt_non_code("P", "g", "w", "aw1"),
        Ok(non_code.clone())
    );
    // Non-code admission is for the exact current attempt, and a code attempt
    // (known to the graph or admitted) is never downgraded.
    for (id, attempt) in [("w", "other"), ("ghost", "aw1"), ("t", "other")] {
        assert_eq!(
            ledger.admit_attempt_non_code("P", "g", id, attempt),
            Err(AttemptAdmissionError::NoSuchAttempt),
            "{id}/{attempt}"
        );
    }
    assert_eq!(
        ledger.admit_attempt_non_code("P", "g", "t", "at1"),
        Err(AttemptAdmissionError::DomainConflict)
    );
    // Admission is trusted core's, for the current attempt, a valid SHA, once.
    for (id, attempt, sha, expect) in [
        ("w", "other", SHA, AttemptAdmissionError::NoSuchAttempt),
        ("ghost", "aw1", SHA, AttemptAdmissionError::NoSuchAttempt),
        (
            "w",
            "aw1",
            "NOT-A-SHA",
            AttemptAdmissionError::InvalidCodeSha,
        ),
        ("t", "at1", SHA2, AttemptAdmissionError::Conflict),
    ] {
        assert_eq!(
            ledger.admit_attempt_code_sha("P", "g", id, attempt, sha),
            Err(expect),
            "{id}/{attempt}"
        );
    }
    let admitted = ledger
        .admit_attempt_code_sha("P", "g", "w", "aw1", SHA)
        .unwrap();
    assert_eq!(admitted.attempt_code_sha("P", "g", "w"), Some(SHA));
    assert_eq!(
        ledger.attempt_code_sha("P", "g", "w"),
        None,
        "input unchanged"
    );
    assert_eq!(
        admitted.admit_attempt_non_code("P", "g", "w", "aw1"),
        Err(AttemptAdmissionError::DomainConflict),
        "admitted code is never downgraded"
    );
    assert_eq!(
        admitted.admit_attempt_code_sha("P", "g", "w", "aw1", SHA2),
        Err(AttemptAdmissionError::Conflict)
    );
    assert_eq!(
        admitted.admit_attempt_code_sha("P", "g", "w", "aw1", SHA),
        Ok(admitted.clone())
    );
    ok(
        &env,
        &admitted,
        a1(),
        json!([result_for("w", "aw1", Some(SHA))]),
    );
    for claim in [Some(SHA2), None] {
        assert_eq!(
            op_err(
                &env,
                &admitted,
                a1(),
                json!([result_for("w", "aw1", claim)])
            )
            .1,
            O::ResultAssociation("code_sha"),
            "{claim:?}"
        );
    }
    // A node with a known SHA is bound to it at dispatch.
    assert_eq!(ledger.attempt_code_sha("P", "g", "t"), Some(SHA));
    for claim in [Some(SHA2), None] {
        assert_eq!(
            op_err(&env, &ledger, a1(), json!([result_for("t", "at1", claim)])).1,
            O::ResultAssociation("code_sha"),
            "{claim:?}"
        );
    }
    ok(
        &env,
        &ledger,
        a1(),
        json!([result_for("t", "at1", Some(SHA))]),
    );
}

#[test]
fn conflicting_sha_evidence_refuses_across_calls() {
    let on_node = |sha: Option<&str>, state: S| {
        let n = node("t", GraphNodeKind::TASK, state);
        envelope(
            vec![match sha {
                Some(sha) => n.with_code_sha(sha).unwrap(),
                None => n,
            }],
            vec![],
            "1",
        )
    };
    let conflict = O::Attempt("code sha differs from the admitted attempt");
    // Dispatched against SHA, then judged against an envelope naming another.
    let (_, bound) = ok(
        &on_node(Some(SHA), S::Admitted),
        &GraphResultLedgerV1::new(),
        a1(),
        json!([dispatch("t", "at1")]),
    );
    assert_eq!(
        op_err(
            &on_node(Some(SHA2), S::Dispatched),
            &bound,
            a1(),
            json!([start("t", "at1")])
        ),
        (0, conflict.clone())
    );
    assert_eq!(
        op_err(
            &on_node(Some(SHA2), S::Running),
            &bound,
            a1(),
            json!([submit("t", "at1")])
        ),
        (0, conflict.clone())
    );
    assert_eq!(
        op_err(
            &on_node(Some(SHA2), S::Running),
            &bound,
            a1(),
            json!([attach(
                "t",
                "r",
                with_attempt(
                    super::mutation_interpreter_tests::result("t", "PASS", "rv"),
                    "at1"
                )
            )])
        ),
        (0, conflict.clone())
    );
    // Dispatched with no known SHA, then one appears: unknown, refused.
    let (_, unbound) = ok(
        &on_node(None, S::Admitted),
        &GraphResultLedgerV1::new(),
        a1(),
        json!([dispatch("t", "at1")]),
    );
    assert_eq!(
        op_err(
            &on_node(Some(SHA), S::Dispatched),
            &unbound,
            a1(),
            json!([start("t", "at1")])
        ),
        (0, conflict)
    );
    // Unchanged data still proceeds.
    ok(
        &on_node(Some(SHA), S::Dispatched),
        &bound,
        a1(),
        json!([start("t", "at1")]),
    );
}

// --- a graph-resident review: circular versus usable --------------------------

#[test]
fn graph_review_is_evidence_only_when_it_does_not_wait_on_the_acceptance_it_gates() {
    let env = envelope(
        vec![
            node("src", GraphNodeKind::IMPLEMENTATION, S::Accepted)
                .with_code_sha(SHA2)
                .unwrap(),
            node("s", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap(),
            node("rg", GraphNodeKind::REVIEW, S::Planned),
            node("rx", GraphNodeKind::REVIEW, S::Planned),
        ],
        vec![
            GraphEdge::precedence("e1", "g", "src", "rg", PrecedenceKind::RequiresAccepted)
                .unwrap(),
            GraphEdge::precedence("e2", "g", "s", "rx", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    );
    // rx requires s accepted: it can never run before the acceptance it
    // would gate.
    assert_eq!(
        op_err(
            &env,
            &GraphResultLedgerV1::new(),
            a1(),
            json!([set(
                "rx",
                "PLANNED",
                "READY",
                json!({"kind":"DEPENDENCIES_SATISFIED"})
            )])
        )
        .1,
        O::EvidenceRefused("dependencies")
    );
    let mut ops = run_as("s", "as1");
    ops.push(attach(
        "s",
        "res-s",
        with_attempt(
            super::mutation_interpreter_tests::result("s", "PASS", "claimed"),
            "as1",
        ),
    ));
    let (env, results) = ok(
        &env,
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    );
    let dir = ReviewDir::new("circular");
    dir.put(&Rec::new("rx", "s", "as1", SHA, ""));
    dir.put(&Rec::new("rg", "s", "as1", SHA, ""));
    let judge = |id: &str| json!([verdict_rv("s", "PASSED", "s", id)]);
    assert_eq!(
        op_err_ev(&env, &results, a2("w"), judge("rx"), rev(&dir)).1,
        O::EvidenceRefused("review requires the acceptance it would gate")
    );
    // A review identity naming a work node is no review at all.
    dir.put(&Rec::new("src", "s", "as1", SHA, ""));
    assert_eq!(
        op_err_ev(&env, &results, a2("w"), judge("src"), rev(&dir)).1,
        O::EvidenceRefused("review identity names a non-review node")
    );
    // rg is satisfiable, but is evidence only once it holds the verdict.
    assert_eq!(
        op_err_ev(&env, &results, a2("w"), judge("rg"), rev(&dir)).1,
        O::EvidenceRefused("graph review has no matching verdict")
    );
    let (env, results) = run_non_code(
        &env,
        &results,
        a2("w"),
        "rg",
        "arg",
        vec![
            attach(
                "rg",
                "res-rg",
                with_attempt(
                    bare(super::mutation_interpreter_tests::result(
                        "rg", "PASS", "rv-rg",
                    )),
                    "arg",
                ),
            ),
            verdict("rg", "PASSED", "rg"),
        ],
    );
    let (env, results) = ok_ev(&env, &results, a2("w"), judge("rg"), rev(&dir));
    assert_eq!(env.graph().node_state("s"), Some(S::Passed));
    assert_eq!(
        results.review("P", "g", "s"),
        Some(("rg", ReviewOutcome::Pass))
    );
}

// --- a repaired aggregate (structural graph data, real round) -----------------

/// A fixture-03-shaped graph: `s` is reviewed by `rj`, whose `ON_REJECT` edge
/// leads to repair `c`, which a fresh review `rn` follows. `rj`'s REJECTED
/// state and the edges are structural-only seeds: an aggregate whose review
/// waits on its acceptance cannot reach them through apply. Everything that
/// matters for evidence — attempts, SHAs, both reviews, both verdicts and the
/// acceptance — runs through apply.
fn aggregate_env(repair_state: S) -> ExecutionGraphEnvelope {
    envelope(
        vec![
            attempt_no(
                node("s", GraphNodeKind::TASK, S::Planned)
                    .with_code_sha(SHA)
                    .unwrap(),
                "1",
            ),
            node("rj", GraphNodeKind::REVIEW, S::Rejected),
            attempt_no(
                node("c", GraphNodeKind::REPAIR, repair_state)
                    .with_parent_node_id("s")
                    .unwrap()
                    .with_created_in_version(crate::GraphVersionV1::try_new("1").unwrap()),
                "2",
            ),
            attempt_no(node("rn", GraphNodeKind::REVIEW, S::Planned), "2"),
        ],
        vec![
            GraphEdge::precedence("pr", "g", "s", "rj", PrecedenceKind::RequiresAccepted).unwrap(),
            GraphEdge::control("oj", "g", "rj", "c", crate::ControlKind::OnReject).unwrap(),
            GraphEdge::precedence("pn", "g", "c", "rn", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    )
}

#[test]
fn repaired_aggregate_is_judged_by_its_repairs_admitted_review() {
    let none = GraphResultLedgerV1::new();
    let dir = ReviewDir::new("aggregate");
    let result = |id: &str, attempt: &str, sha: &str| {
        let mut r = with_attempt(
            super::mutation_interpreter_tests::result(id, "PASS", "claimed"),
            attempt,
        );
        r["code_sha"] = json!(sha);
        r
    };
    // Round 1: s runs and independent review rejects it.
    let mut ops = run_as("s", "as1");
    ops.push(attach("s", "res-s", result("s", "as1", SHA)));
    let (env, results) = ok(
        &aggregate_env(S::Planned),
        &none,
        a2("w"),
        Value::Array(ops),
    );
    dir.put(&Rec::new("rv-1", "s", "as1", SHA, "1").rejected());
    let (env, results) = ok_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("s", "REJECTED", "s", "rv-1")]),
        rev(&dir),
    );
    assert_eq!(
        results.review("P", "g", "s"),
        Some(("rv-1", ReviewOutcome::Rejected))
    );
    // The repair is entered on the full provenance.
    let enter = set(
        "s",
        "REJECTED",
        "REPAIRING",
        json!({"kind":"REPAIR_ENTRY","repair_node_id":"c"}),
    );
    let (env, results) = ok(&env, &results, a2("w"), json!([enter]));
    // Round 2: the repair runs with its own attempt; trusted core binds the
    // commit it produced.
    let (env, results) = ok(&env, &results, a2("w"), Value::Array(run_as("c", "ac1")));
    let results = results
        .admit_attempt_code_sha("P", "g", "c", "ac1", SHA2)
        .unwrap();
    let (env, results) = ok(
        &env,
        &results,
        a2("w"),
        json!([attach("c", "res-c", result("c", "ac1", SHA2))]),
    );
    // The rejected round's review cannot promote the repair, in any guise.
    dir.put(&Rec::new("rv-1", "c", "ac1", SHA2, "2"));
    assert_eq!(
        op_err_ev(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("c", "PASSED", "c", "rv-1")]),
            rev(&dir)
        )
        .1,
        O::EvidenceRefused("review identity already used")
    );
    dir.put(&Rec::new("rv-old", "s", "as1", SHA, "1"));
    assert_eq!(
        op_err_ev(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("c", "PASSED", "c", "rv-old")]),
            rev(&dir)
        )
        .1,
        O::Review(R::WrongSubject)
    );
    dir.put(&Rec::new("rv-2", "c", "ac1", SHA2, "2"));
    let (env, results) = ok_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("c", "PASSED", "c", "rv-2")]),
        rev(&dir),
    );
    let submitted = set(
        "s",
        "REPAIRING",
        "AWAITING_REVIEW",
        json!({"kind":"REPAIR_SUBMITTED","repair_node_id":"c"}),
    );
    let (env, results) = ok(&env, &results, a2("w"), json!([submitted]));
    // s cannot reuse its own rejected round, and needs no new outside review:
    // its verdict cites the repair, whose admitted review is the evidence.
    assert!(matches!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict("s", "PASSED", "s")])
        )
        .1,
        O::EvidenceRefused(_)
    ));
    assert_eq!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict("s", "REJECTED", "c")])
        )
        .1,
        O::EvidenceRefused("cited result is not the current round")
    );
    let (env, results) = ok(
        &env,
        &results,
        a2("w"),
        json!([verdict("s", "PASSED", "c")]),
    );
    assert_eq!(env.graph().node_state("s"), Some(S::Passed));
    let accept = set(
        "s",
        "PASSED",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"d"}),
    );
    let (env, results) = ok(&env, &results, a1(), json!([accept]));
    assert_eq!(env.graph().node_state("s"), Some(S::Accepted));
    // Retained history: both reviews, both attempts, the rejected review node.
    assert_eq!(
        results.review("P", "g", "c"),
        Some(("rv-2", ReviewOutcome::Pass))
    );
    assert_eq!(results.attempt("P", "g", "s"), Some("as1"));
    assert_eq!(results.attempt("P", "g", "c"), Some("ac1"));
    assert_eq!(env.graph().node_state("rj"), Some(S::Rejected));
}

#[test]
fn a_repair_without_admitted_review_cannot_carry_its_aggregate() {
    let none = GraphResultLedgerV1::new();
    // A repair that merely claims PASSED as graph data has no review history.
    let mut ops = run_as("s", "as1");
    ops.push(attach(
        "s",
        "res-s",
        with_attempt(
            super::mutation_interpreter_tests::result("s", "PASS", "claimed"),
            "as1",
        ),
    ));
    let (env, results) = ok(&aggregate_env(S::Passed), &none, a2("w"), Value::Array(ops));
    let dir = ReviewDir::new("seeded-repair");
    dir.put(&Rec::new("rv-1", "s", "as1", SHA, "1").rejected());
    let (env, results) = ok_ev(
        &env,
        &results,
        a2("w"),
        json!([verdict_rv("s", "REJECTED", "s", "rv-1")]),
        rev(&dir),
    );
    let (env, results) = ok(
        &env,
        &results,
        a2("w"),
        json!([
            set(
                "s",
                "REJECTED",
                "REPAIRING",
                json!({"kind":"REPAIR_ENTRY","repair_node_id":"c"})
            ),
            set(
                "s",
                "REPAIRING",
                "AWAITING_REVIEW",
                json!({"kind":"REPAIR_SUBMITTED","repair_node_id":"c"})
            )
        ]),
    );
    assert_eq!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict("s", "PASSED", "c")])
        )
        .1,
        O::Review(R::MissingRecord)
    );
    // An aggregate nobody has judged cannot borrow a repair's verdict either.
    let (env, results) = ok(
        &aggregate_env(S::Passed),
        &none,
        a2("w"),
        Value::Array({
            let mut ops = run_as("s", "as1");
            ops.push(attach(
                "s",
                "res-s",
                with_attempt(
                    super::mutation_interpreter_tests::result("s", "PASS", "claimed"),
                    "as1",
                ),
            ));
            ops
        }),
    );
    assert_eq!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict_rv("s", "PASSED", "c", "rv")])
        )
        .1,
        O::EvidenceRefused("cited result is not the current round")
    );
}
