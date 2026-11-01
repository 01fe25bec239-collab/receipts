//! Code versus non-code admission. An omitted `code_sha` classifies nothing:
//! until trusted core admits the attempt as code (known or admitted SHA) or as
//! non-code, its result, verdict and acceptance are unavailable. This is the
//! A1 reproduction (an IMPLEMENTATION accepted with no admitted SHA) and its
//! controls, through the public interpreter.

use serde_json::{Value, json};

use super::mutation_interpreter_tests::{
    SHA, a1, a2, attach, bare, dispatch, envelope, node, ok, ok_ev, op_err, op_err_ev, rev, run_as,
    set, start, verdict_rv, with_attempt,
};
use super::review_evidence_tests::{Rec, ReviewDir};
use crate::{
    AttemptAdmissionError, ExecutionGraphEnvelope, GraphNodeKind, GraphNodeState as S,
    GraphOperationError as O, GraphResultLedgerV1, ReviewEvidenceRefusal as R, ReviewOutcome,
};

const SHA2: &str = "89abcdef0123456789abcdef0123456789abcdef";
const UNCLASSIFIED: O = O::EvidenceUnavailable("attempt code or non-code classification");

/// An IMPLEMENTATION whose graph data names no commit.
fn impl_env() -> ExecutionGraphEnvelope {
    envelope(
        vec![node("code", GraphNodeKind::IMPLEMENTATION, S::Planned)],
        vec![],
        "1",
    )
}

fn claim(sha: Option<&str>) -> Value {
    let r = with_attempt(
        super::mutation_interpreter_tests::result("code", "PASS", "claimed-by-the-worker"),
        "ac1",
    );
    match sha {
        Some(sha) => {
            let mut r = r;
            r["code_sha"] = json!(sha);
            r
        }
        None => bare(r),
    }
}

fn judge() -> Value {
    verdict_rv("code", "PASSED", "code", "independent")
}

fn accept() -> Value {
    set(
        "code",
        "PASSED",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"decision"}),
    )
}

/// PLANNED → RUNNING as attempt `ac1`; the SHA is still unknown to core.
fn running() -> (ExecutionGraphEnvelope, GraphResultLedgerV1) {
    let mut ops = run_as("code", "ac1");
    ops.pop();
    ok(
        &impl_env(),
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    )
}

#[test]
fn the_omission_path_refuses_unless_core_admitted_a_classification() {
    // A1's reproduction: dispatch, a PASS with no SHA, an admitted independent
    // review record with no SHA, explicit acceptance, all in one batch. It
    // used to end ACCEPTED with no SHA anywhere.
    let dir = ReviewDir::new("omission");
    dir.put(&Rec::new("independent", "code", "ac1", "", ""));
    let mut ops = run_as("code", "ac1");
    ops.push(attach("code", "res", claim(None)));
    ops.push(judge());
    ops.push(accept());
    let none = GraphResultLedgerV1::new();
    assert_eq!(
        op_err_ev(&impl_env(), &none, a1(), Value::Array(ops), rev(&dir)),
        (5, UNCLASSIFIED)
    );

    // Across calls: submitted but unclassified, a SHA-less result, a code
    // claim, and the verdict itself are all unavailable; none changes state.
    let (env, ledger) = running();
    let (env, ledger) = ok(
        &env,
        &ledger,
        a2("w"),
        json!([super::mutation_interpreter_tests::submit("code", "ac1")]),
    );
    for sha in [None, Some(SHA)] {
        assert_eq!(
            op_err_ev(
                &env,
                &ledger,
                a1(),
                json!([attach("code", "res", claim(sha))]),
                rev(&dir)
            ),
            (0, UNCLASSIFIED),
            "{sha:?}"
        );
    }
    assert_eq!(
        op_err_ev(&env, &ledger, a1(), json!([judge()]), rev(&dir)),
        (0, UNCLASSIFIED),
        "the verdict does not rest on an unclassified attempt"
    );
    assert!(!ledger.attempt_is_non_code("P", "g", "code"));
    assert_eq!(ledger.attempt_code_sha("P", "g", "code"), None);
}

#[test]
fn worker_claims_cannot_classify_the_attempt() {
    // Neither the result's SHA, its review id nor its outcome admits it; a
    // known node of a work kind with no SHA stays unclassified too.
    let (env, ledger) = running();
    let (env, ledger) = ok(
        &env,
        &ledger,
        a2("w"),
        json!([super::mutation_interpreter_tests::submit("code", "ac1")]),
    );
    assert_eq!(ledger.attempt("P", "g", "code"), Some("ac1"));
    for r in [claim(None), claim(Some(SHA)), claim(Some(SHA2))] {
        assert_eq!(
            op_err(&env, &ledger, a2("w"), json!([attach("code", "res", r)])).1,
            UNCLASSIFIED
        );
    }
}

#[test]
fn correct_code_admission_reaches_acceptance_with_the_bound_sha() {
    let dir = ReviewDir::new("code-admitted");
    let (env, ledger) = running();
    let admitted = ledger
        .admit_attempt_code_sha("P", "g", "code", "ac1", SHA)
        .unwrap();
    let (env, ledger) = ok(
        &env,
        &admitted,
        a2("w"),
        json!([
            super::mutation_interpreter_tests::submit("code", "ac1"),
            attach("code", "res", claim(Some(SHA)))
        ]),
    );
    // A record that omits the SHA, or names another, is not this attempt's.
    for (sha, expect) in [("", R::CodeMismatch), (SHA2, R::CodeMismatch)] {
        dir.put(&Rec::new("independent", "code", "ac1", sha, ""));
        assert_eq!(
            op_err_ev(&env, &ledger, a2("w"), json!([judge()]), rev(&dir)).1,
            O::Review(expect),
            "record sha {sha:?}"
        );
    }
    dir.put(&Rec::new("independent", "code", "ac1", SHA, ""));
    let (env, ledger) = ok_ev(&env, &ledger, a2("w"), json!([judge()]), rev(&dir));
    let (env, ledger) = ok(&env, &ledger, a1(), json!([accept()]));
    assert_eq!(env.graph().node_state("code"), Some(S::Accepted));
    assert_eq!(ledger.attempt_code_sha("P", "g", "code"), Some(SHA));
    assert!(!ledger.attempt_is_non_code("P", "g", "code"));
}

#[test]
fn explicit_non_code_admission_succeeds_without_a_fabricated_sha() {
    let dir = ReviewDir::new("non-code");
    let (env, ledger) = running();
    let non_code = ledger
        .admit_attempt_non_code("P", "g", "code", "ac1")
        .unwrap();
    let (env, ledger) = ok(
        &env,
        &non_code,
        a2("w"),
        json!([
            super::mutation_interpreter_tests::submit("code", "ac1"),
            attach("code", "res", claim(None))
        ]),
    );
    // A record that claims a commit for a non-code attempt is contradicted.
    dir.put(&Rec::new("independent", "code", "ac1", SHA, ""));
    assert_eq!(
        op_err_ev(&env, &ledger, a2("w"), json!([judge()]), rev(&dir)).1,
        O::Review(R::CodeMismatch)
    );
    dir.put(&Rec::new("independent", "code", "ac1", "", ""));
    let (env, ledger) = ok_ev(&env, &ledger, a2("w"), json!([judge()]), rev(&dir));
    // A conflicting admission after the judgement is refused and leaves the
    // judged result, review and the later acceptance intact.
    assert_eq!(
        ledger.admit_attempt_code_sha("P", "g", "code", "ac1", SHA),
        Err(AttemptAdmissionError::DomainConflict)
    );
    assert_eq!(
        ledger.review("P", "g", "code"),
        Some(("independent", ReviewOutcome::Pass))
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([accept()]));
    assert_eq!(env.graph().node_state("code"), Some(S::Accepted));
    assert_eq!(ledger.attempt_code_sha("P", "g", "code"), None);
    assert!(ledger.attempt_is_non_code("P", "g", "code"));
}

#[test]
fn classification_is_exact_never_downgraded_and_survives_failed_batches() {
    let (env, ledger) = running();
    // Wrong node, attempt or project/graph key.
    for (p, g, n, a) in [
        ("P", "g", "code", "other"),
        ("P", "g", "ghost", "ac1"),
        ("Q", "g", "code", "ac1"),
        ("P", "h", "code", "ac1"),
    ] {
        assert_eq!(
            ledger.admit_attempt_non_code(p, g, n, a),
            Err(AttemptAdmissionError::NoSuchAttempt),
            "{p}/{g}/{n}/{a}"
        );
    }
    let code = ledger
        .admit_attempt_code_sha("P", "g", "code", "ac1", SHA)
        .unwrap();
    // Admitted code is never downgraded; a different SHA still conflicts.
    assert_eq!(
        code.admit_attempt_non_code("P", "g", "code", "ac1"),
        Err(AttemptAdmissionError::DomainConflict)
    );
    assert_eq!(
        code.admit_attempt_code_sha("P", "g", "code", "ac1", SHA2),
        Err(AttemptAdmissionError::Conflict)
    );
    // A failed batch neither consumes nor changes a classification.
    let bad = json!([
        super::mutation_interpreter_tests::submit("code", "ac1"),
        attach("code", "res", with_attempt(claim(Some(SHA)), "elsewhere"))
    ]);
    assert_eq!(
        op_err(&env, &code, a2("w"), bad).1,
        O::ResultAssociation("attempt_id")
    );
    let wrong_sha = json!([
        super::mutation_interpreter_tests::submit("code", "ac1"),
        attach("code", "res", claim(Some(SHA2)))
    ]);
    assert_eq!(
        op_err(&env, &code, a2("w"), wrong_sha).1,
        O::ResultAssociation("code_sha")
    );
    let missing_sha = json!([
        super::mutation_interpreter_tests::submit("code", "ac1"),
        attach("code", "res", claim(None))
    ]);
    assert_eq!(
        op_err(&env, &code, a2("w"), missing_sha).1,
        O::ResultAssociation("code_sha")
    );
    assert_eq!(code.attempt_code_sha("P", "g", "code"), Some(SHA));
    ok(
        &env,
        &code,
        a2("w"),
        json!([
            super::mutation_interpreter_tests::submit("code", "ac1"),
            attach("code", "res", claim(Some(SHA)))
        ]),
    );
}

#[test]
fn a_new_attempt_starts_unclassified() {
    // Non-code admission belongs to one attempt; a resumed node's next
    // attempt is a new one and is unclassified again.
    let (env, ledger) = running();
    let ledger = ledger
        .admit_attempt_non_code("P", "g", "code", "ac1")
        .unwrap();
    let (env, ledger) = ok(
        &env,
        &ledger,
        a1(),
        json!([
            set(
                "code",
                "RUNNING",
                "BLOCKED",
                json!({"kind":"BLOCKED","reason":"r"})
            ),
            set(
                "code",
                "BLOCKED",
                "READY",
                json!({"kind":"RESOLVED","resolution_ref":"x"})
            ),
            set(
                "code",
                "READY",
                "ADMITTED",
                json!({"kind":"ADMISSION","admission_decision_id":"ad"})
            ),
            dispatch("code", "ac2"),
            start("code", "ac2"),
            super::mutation_interpreter_tests::submit("code", "ac2"),
        ]),
    );
    assert!(!ledger.attempt_is_non_code("P", "g", "code"));
    assert_eq!(
        op_err(
            &env,
            &ledger,
            a1(),
            json!([attach("code", "res", with_attempt(claim(None), "ac2"))])
        )
        .1,
        UNCLASSIFIED
    );
    assert_eq!(
        ledger.admit_attempt_non_code("P", "g", "code", "ac1"),
        Err(AttemptAdmissionError::NoSuchAttempt),
        "the old attempt can no longer be reclassified"
    );
}

#[test]
fn no_kind_is_exempt_from_classification_at_the_verdict() {
    // A REVIEW node is judged by its own result, but its label does not say
    // the attempt is non-code: the verdict waits for core's classification.
    let env = envelope(
        vec![node("rv", GraphNodeKind::REVIEW, S::Planned)],
        vec![],
        "1",
    );
    let mut ops = run_as("rv", "arv");
    let submit = ops.pop().unwrap();
    let (env, ledger) = ok(
        &env,
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    );
    let (env, ledger) = ok(&env, &ledger, a2("w"), json!([submit]));
    let verdict = super::mutation_interpreter_tests::verdict("rv", "PASSED", "rv");
    assert_eq!(
        op_err(&env, &ledger, a2("w"), json!([verdict.clone()])).1,
        UNCLASSIFIED
    );
    // Once classified, the refusal is the missing result, not the domain.
    let ledger = ledger
        .admit_attempt_non_code("P", "g", "rv", "arv")
        .unwrap();
    assert_eq!(
        op_err(&env, &ledger, a2("w"), json!([verdict])).1,
        O::EvidenceUnavailable("attached review result")
    );
}
