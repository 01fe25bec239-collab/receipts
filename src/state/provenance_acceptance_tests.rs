//! A link called ACCEPTANCE_REF must name a recorded ACCEPT decision with
//! exactly one provenance entry for this task, bound to the recorded SHAs.
//! Nonaccept and entry-less decisions remain valid stored documents.

use serde_json::{Value, json};

use crate::decision_records::{
    DecisionRecordKind as K, DecisionRecordV1, ProvenanceContradictionV1 as C,
    ProvenanceLinkKind as L,
};
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const S: &str = "cccccccccccccccccccccccccccccccccccccccc";
const D: &str = "dddddddddddddddddddddddddddddddddddddddd";

fn entry(task: &str, start: &str, implementation: &str, review: &str) -> Value {
    json!({"task_id":task,"start_sha":start,"implementation_sha":implementation,"review_sha":review})
}

fn decision(outcome: &str, chain: Vec<Value>) -> String {
    json!({"decision_id":"dec","request_id":"ir-1","outcome":outcome,
        "checks_evaluated":[{"check":"ci","result":"PASS"}],"provenance_chain":chain,
        "integration_sha":B,"decided_at":"2026-10-29T00:00:00Z"})
    .to_string()
}

/// A repo whose task `t-1` has all seven links and a stored A4 review; the
/// recorded integration decision is the one under test.
fn repo_with(tag: &str, decision_json: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let review = json!({"review_id":"rev","task_id":"t-1","reviewed_sha":A,"verdict":"PASS",
        "independence_attested":true,"blocking_findings":[],"nonblocking_findings":[],
        "dimensions":[{"dimension":"CORRECTNESS","assessment":"SATISFIED"}]})
    .to_string();
    for (kind, id, json) in [
        (K::A4Review, "rev", review.as_str()),
        (K::IntegrationDecision, "dec", decision_json),
    ] {
        repo.append_decision_record(&DecisionRecordV1::parse("p", kind, id, json).unwrap())
            .unwrap();
    }
    for (kind, value) in [
        (L::StartSha, S),
        (L::ImplementationSha, A),
        (L::ReviewSha, A),
        (L::VerdictRef, "rev"),
        (L::AcceptanceRef, "dec"),
        (L::WorkstreamSha, D),
        (L::MainSha, B),
    ] {
        repo.append_provenance_link("p", "t-1", kind, value)
            .unwrap();
    }
    (tmp, repo)
}

fn contradictions(tag: &str, decision_json: &str) -> Vec<C> {
    let (_tmp, repo) = repo_with(tag, decision_json);
    let chain = repo.provenance_chain("p", "t-1").unwrap();
    assert!(chain.missing.is_empty(), "all links were recorded");
    assert_eq!(
        chain.is_complete_and_consistent(),
        chain.contradictions.is_empty()
    );
    chain.contradictions
}

#[test]
fn accept_decision_with_one_matching_same_task_entry_is_complete_and_consistent() {
    let (_tmp, repo) = repo_with(
        "accept-ok",
        &decision("ACCEPT", vec![entry("t-1", S, A, A)]),
    );
    let chain = repo.provenance_chain("p", "t-1").unwrap();
    assert!(chain.is_complete_and_consistent(), "{chain:?}");
    // A foreign task's entry beside the same-task entry changes nothing.
    let (_tmp, repo) = repo_with(
        "accept-ok-foreign",
        &decision("ACCEPT", vec![entry("t-2", D, D, D), entry("t-1", S, A, A)]),
    );
    assert!(
        repo.provenance_chain("p", "t-1")
            .unwrap()
            .is_complete_and_consistent()
    );
}

#[test]
fn absent_or_foreign_task_entries_are_contradictory_not_consistent() {
    assert_eq!(
        contradictions("accept-none", &decision("ACCEPT", vec![])),
        vec![C::AcceptanceTaskEntryAbsent]
    );
    assert_eq!(
        contradictions(
            "accept-foreign",
            &decision("ACCEPT", vec![entry("t-2", S, A, A)])
        ),
        vec![C::AcceptanceTaskEntryAbsent]
    );
    // An omitted provenance_chain is the same absence.
    let omitted = json!({"decision_id":"dec","request_id":"ir-1","outcome":"ACCEPT",
        "checks_evaluated":[{"check":"ci","result":"PASS"}],"decided_at":"x"})
    .to_string();
    assert_eq!(
        contradictions("accept-omitted", &omitted),
        vec![C::AcceptanceTaskEntryAbsent]
    );
}

#[test]
fn duplicate_same_task_entries_are_ambiguous_even_when_identical() {
    for (tag, chain) in [
        (
            "dup-identical",
            vec![entry("t-1", S, A, A), entry("t-1", S, A, A)],
        ),
        (
            "dup-conflict",
            vec![entry("t-1", S, A, A), entry("t-1", D, A, A)],
        ),
    ] {
        assert_eq!(
            contradictions(tag, &decision("ACCEPT", chain)),
            vec![C::AcceptanceTaskEntryAmbiguous],
            "{tag}"
        );
    }
}

#[test]
fn nonaccept_decisions_are_valid_records_but_not_acceptance_evidence() {
    for outcome in ["REPAIR", "BLOCKED", "HUMAN_REQUIRED"] {
        let json = decision(outcome, vec![entry("t-1", S, A, A)]);
        // The nonaccept record is a valid stored historical document.
        assert!(DecisionRecordV1::parse("p", K::IntegrationDecision, "dec", &json).is_ok());
        assert_eq!(
            contradictions(&format!("nonaccept-{outcome}"), &json),
            vec![C::AcceptanceNotAccepted],
            "{outcome}"
        );
    }
}

#[test]
fn each_mismatched_sha_is_a_contradiction() {
    for (tag, e) in [
        ("start", entry("t-1", D, A, A)),
        ("implementation", entry("t-1", S, D, A)),
        ("review", entry("t-1", S, A, D)),
    ] {
        assert_eq!(
            contradictions(&format!("mismatch-{tag}"), &decision("ACCEPT", vec![e])),
            vec![C::AcceptanceChainMismatch],
            "{tag}"
        );
    }
    // Several defects are all reported, never collapsed into one.
    assert_eq!(
        contradictions("multi", &decision("BLOCKED", vec![entry("t-1", D, A, A)])),
        vec![C::AcceptanceNotAccepted, C::AcceptanceChainMismatch]
    );
}
