//! Compiler determinism, frozen-fixture shape, verbatim spec, provenance,
//! coherence and criteria-confirmation controls.

use std::collections::{BTreeMap, BTreeSet};

use receipts_state::entitlement::ProductEntitlementState;
use receipts_state::{GraphCompilerRefV1, GraphSourceEvidenceRefV1};
use serde_json::Value;

use super::*;
use crate::capsules::{Criterion, CriterionKind};
use crate::orchestration::{CoreContentResolverV1, FeatureAdmissionOutcome};
use crate::test_support::{COMPILER, PORT, SOURCE, paths, time};
use crate::wire::ExecutionGraphWireV1;
use crate::{CapabilityName, GraphNodeKind, PrecedenceKind};

const SPEC: &[u8] = b"# Goal\r\nShip the scheduler.\n\x00trailing bytes kept\n";

struct Specs;
impl CoreContentResolverV1 for Specs {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        (reference == "SPEC.md" && matches!(project_id, "P1" | "p")).then(|| SPEC.to_vec())
    }
}

fn read(path: &str) -> Value {
    let text =
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn criterion(id: &str, check: bool, origin: CriterionOriginV1) -> ProposedCriterionV1 {
    ProposedCriterionV1 {
        criterion: Criterion::try_new(
            id.into(),
            format!("criterion {id}"),
            CriterionKind::Deterministic,
            check.then(|| paths(&["cargo", "test"])),
            None,
        )
        .unwrap(),
        origin,
    }
}

/// A proposal reproducing a frozen graph fixture, in reversed order.
fn proposal_from(fixture: &Value) -> DecompositionProposalV1 {
    let kind = |k: &str| match k {
        "REQUIRES_ACCEPTED" => PrecedenceKind::RequiresAccepted,
        "REQUIRES_INTEGRATED" => PrecedenceKind::RequiresIntegrated,
        "REQUIRES_INTERFACE" => PrecedenceKind::RequiresInterface,
        other => panic!("{other}"),
    };
    let mut nodes: Vec<ProposedNodeV1> = fixture["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| ProposedNodeV1 {
            node_id: n["node_id"].as_str().unwrap().into(),
            kind: GraphNodeKind::new(n["kind"].as_str().unwrap().to_owned()).unwrap(),
            title: n["title"].as_str().map(str::to_owned),
            workstream_id: n["workstream_id"].as_str().map(str::to_owned),
            required_capabilities: n["required_capabilities"].as_array().map(|caps| {
                caps.iter()
                    .map(|c| CapabilityName::new(c.as_str().unwrap()).unwrap())
                    .collect()
            }),
        })
        .collect();
    let mut dependencies: Vec<ProposedDependencyV1> = fixture["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| ProposedDependencyV1 {
            from: e["from_node"].as_str().unwrap().into(),
            to: e["to_node"].as_str().unwrap().into(),
            kind: kind(e["precedence_kind"].as_str().unwrap()),
        })
        .collect();
    nodes.reverse();
    dependencies.reverse();
    DecompositionProposalV1 {
        workstreams: Vec::new(),
        nodes,
        dependencies,
        criteria: vec![criterion("c1", true, CriterionOriginV1::Stated)],
    }
}

struct Inputs {
    admission: BTreeMap<String, FeatureAdmissionOutcome>,
    confirmed: BTreeSet<String>,
}

impl Inputs {
    fn new() -> Self {
        let admission = [
            "graph.core",
            "graph.deterministic_checks",
            "routing.model_intelligence",
            "orchestration.multi_runtime",
        ]
        .into_iter()
        .map(|c| (c.to_owned(), FeatureAdmissionOutcome::Allow))
        .chain([(
            "review.independent_a4".to_owned(),
            FeatureAdmissionOutcome::LockedRequiresPro,
        )])
        .collect();
        Self {
            admission,
            confirmed: BTreeSet::new(),
        }
    }

    fn context<'a>(&'a self, fixture: &'a Value) -> CompileContextV1<'a> {
        CompileContextV1 {
            project_id: fixture["project_id"].as_str().unwrap(),
            graph_id: fixture["graph_id"].as_str().unwrap(),
            goal_id: fixture["goal_id"].as_str().unwrap(),
            policy_id: fixture["policy_id"].as_str().unwrap(),
            created_at: time(fixture["created_at"].as_str().unwrap()),
            context_epoch: fixture["context_epoch"]
                .as_u64()
                .map(|e| GoalEvaluationContextEpoch::from_decimal(&e.to_string()).unwrap()),
            compiler_id: GraphCompilerRefV1::parse(COMPILER).unwrap(),
            source: GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
            spec_ref: "SPEC.md",
            content: &Specs,
            capability_admission: &self.admission,
            entitlement_state: ProductEntitlementState::Free,
            confirmed_criteria: &self.confirmed,
        }
    }
}

fn sorted(mut v: Value) -> Value {
    for (list, key) in [("nodes", "node_id"), ("edges", "edge_id")] {
        v[list]
            .as_array_mut()
            .unwrap()
            .sort_by(|a, b| a[key].as_str().cmp(&b[key].as_str()));
    }
    v
}

#[test]
fn frozen_graph_fixtures_compile_to_their_exact_shape() {
    let inputs = Inputs::new();
    for name in [
        "01_linear",
        "02_parallel_fanout_fanin",
        "04_human_gate",
        "06_free_graph",
        "07_pro_graph_with_locked_node",
    ] {
        let fixture = read(&format!("fixtures/graphs/{name}.json"));
        let compiled =
            compile_graph_v1(&inputs.context(&fixture), &proposal_from(&fixture), &PORT).unwrap();
        let mut encoded: Value = serde_json::from_str(
            &ExecutionGraphWireV1::from_envelope(&compiled.envelope).to_json(),
        )
        .unwrap();
        let object = encoded.as_object_mut().unwrap();
        let compiled_from = object.remove("compiled_from").unwrap();
        let digest = object.remove("resulting_digest").unwrap();
        assert_eq!(encoded, sorted(fixture.clone()), "{name}");
        assert_eq!(
            compiled_from,
            serde_json::json!(["SPEC.md", format!("sha256:{}", compiled.spec_sha256)])
        );
        assert_eq!(
            digest.as_str().unwrap(),
            graph_content_digest_v1(compiled.envelope.graph()).unwrap()
        );
    }
}

#[test]
fn compile_is_deterministic_and_order_independent() {
    let inputs = Inputs::new();
    let fixture = read("fixtures/graphs/02_parallel_fanout_fanin.json");
    let proposal = proposal_from(&fixture);
    let first = compile_graph_v1(&inputs.context(&fixture), &proposal, &PORT).unwrap();
    let mut reordered = proposal.clone();
    reordered.nodes.reverse();
    reordered.dependencies.rotate_left(2);
    for _ in 0..3 {
        assert_eq!(
            compile_graph_v1(&inputs.context(&fixture), &reordered, &PORT).unwrap(),
            first
        );
    }
}

#[test]
fn spec_is_kept_verbatim_with_compiler_genesis_provenance() {
    let inputs = Inputs::new();
    let fixture = read("fixtures/graphs/06_free_graph.json");
    let compiled =
        compile_graph_v1(&inputs.context(&fixture), &proposal_from(&fixture), &PORT).unwrap();
    assert_eq!(compiled.spec, SPEC);
    let provenance = compiled.genesis.genesis_provenance();
    assert_eq!(provenance.compiler_id().as_str(), COMPILER);
    assert_eq!(provenance.source_ref().as_str(), SOURCE);
    assert_eq!(provenance.creation_reason().as_str(), "INITIAL_COMPILATION");
    assert_eq!(
        Some(compiled.genesis.resulting_digest()),
        compiled.envelope.resulting_digest()
    );
    assert_eq!(compiled.genesis.parent_version(), None);
    assert_eq!(compiled.genesis.policy_id(), "free.single_runtime");
    // An unresolvable spec compiles nothing.
    let mut context = inputs.context(&fixture);
    context.spec_ref = "MISSING.md";
    assert_eq!(
        compile_graph_v1(&context, &proposal_from(&fixture), &PORT),
        Err(CompileErrorV1::SpecUnavailable)
    );
}

fn ws(id: &str, owned: &[&str]) -> WorkstreamOwnershipV1 {
    WorkstreamOwnershipV1 {
        workstream_id: id.into(),
        owned_paths: paths(owned),
    }
}

fn task(id: &str, workstream: &str) -> ProposedNodeV1 {
    ProposedNodeV1 {
        node_id: id.into(),
        kind: GraphNodeKind::TASK,
        title: None,
        workstream_id: Some(workstream.into()),
        required_capabilities: None,
    }
}

fn small(workstreams: Vec<WorkstreamOwnershipV1>) -> DecompositionProposalV1 {
    DecompositionProposalV1 {
        workstreams,
        nodes: vec![task("t1", "ws-a"), task("t2", "ws-b")],
        dependencies: vec![],
        criteria: vec![criterion("c1", true, CriterionOriginV1::Stated)],
    }
}

fn compile_small(
    proposal: &DecompositionProposalV1,
    inputs: &Inputs,
) -> Result<CompiledGraphV1, CompileErrorV1> {
    let fixture = read("fixtures/graphs/06_free_graph.json");
    compile_graph_v1(&inputs.context(&fixture), proposal, &PORT)
}

#[test]
fn too_granular_workstreams_merge_and_ownerless_or_overlapping_ones_refuse() {
    let inputs = Inputs::new();
    // Same scope (order and duplicates aside): one ownership unit.
    let merged = compile_small(
        &small(vec![
            ws("ws-b", &["src/core/**", "src/core/**"]),
            ws("ws-a", &["src/core/**"]),
        ]),
        &inputs,
    )
    .unwrap();
    assert_eq!(merged.merged_workstreams, [("ws-b".into(), "ws-a".into())]);
    assert_eq!(merged.workstreams, [ws("ws-a", &["src/core/**"])]);
    assert_eq!(
        merged.envelope.graph().node("t2").unwrap().workstream_id(),
        Some("ws-a")
    );

    let refusals = [
        (
            small(vec![ws("ws-a", &["src/core/**"]), ws("ws-b", &[])]),
            CompileErrorV1::WorkstreamWithoutOwnership("ws-b".into()),
        ),
        (
            small(vec![ws("ws-a", &["src/core/**"])]),
            CompileErrorV1::UnownedWorkstream {
                node_id: "t2".into(),
                workstream_id: "ws-b".into(),
            },
        ),
        (
            small(vec![ws("ws-a", &["src/**"]), ws("ws-b", &["src/core/**"])]),
            CompileErrorV1::OverlappingOwnership {
                left: "ws-a".into(),
                right: "ws-b".into(),
                witness: "src/core/x".into(),
            },
        ),
        (
            small(vec![ws("ws-a", &["src/**"]), ws("ws-b", &["d?cs/**"])]),
            CompileErrorV1::OwnershipUndetermined {
                left: "ws-a".into(),
                right: "ws-b".into(),
            },
        ),
        (
            small(vec![ws("ws-a", &["a/**"]), ws("ws-a", &["b/**"])]),
            CompileErrorV1::DuplicateWorkstream("ws-a".into()),
        ),
    ];
    for (proposal, expected) in refusals {
        assert_eq!(compile_small(&proposal, &inputs), Err(expected));
    }
}

#[test]
fn derived_or_unmeasurable_criteria_need_explicit_confirmation() {
    let mut inputs = Inputs::new();
    let mut proposal = small(vec![ws("ws-a", &["a/**"]), ws("ws-b", &["b/**"])]);
    proposal.criteria = vec![
        criterion("stated", true, CriterionOriginV1::Stated),
        criterion("derived", true, CriterionOriginV1::Derived),
        criterion("unmeasurable", false, CriterionOriginV1::Stated),
    ];
    assert_eq!(
        compile_small(&proposal, &inputs),
        Err(CompileErrorV1::CriteriaRequireConfirmation(vec![
            "derived".into(),
            "unmeasurable".into()
        ]))
    );
    inputs.confirmed.insert("derived".into());
    assert_eq!(
        compile_small(&proposal, &inputs),
        Err(CompileErrorV1::CriteriaRequireConfirmation(vec![
            "unmeasurable".into()
        ]))
    );
    inputs.confirmed.insert("unmeasurable".into());
    assert_eq!(compile_small(&proposal, &inputs).unwrap().criteria.len(), 3);

    proposal.criteria.clear();
    assert_eq!(
        compile_small(&proposal, &inputs),
        Err(CompileErrorV1::NoAcceptanceCriteria)
    );
    proposal.criteria = vec![
        criterion("x", true, CriterionOriginV1::Stated),
        criterion("x", true, CriterionOriginV1::Stated),
    ];
    assert_eq!(
        compile_small(&proposal, &inputs),
        Err(CompileErrorV1::DuplicateCriterion("x".into()))
    );
}

#[test]
fn unresolved_capabilities_and_malformed_topology_refuse() {
    let mut inputs = Inputs::new();
    let fixture = read("fixtures/graphs/07_pro_graph_with_locked_node.json");
    inputs.admission.remove("routing.model_intelligence");
    assert_eq!(
        compile_graph_v1(&inputs.context(&fixture), &proposal_from(&fixture), &PORT),
        Err(CompileErrorV1::CapabilityAdmissionUnresolved(
            "routing.model_intelligence".into()
        ))
    );
    inputs.admission.insert(
        "routing.model_intelligence".into(),
        FeatureAdmissionOutcome::EntitlementUnknown,
    );
    assert_eq!(
        compile_graph_v1(&inputs.context(&fixture), &proposal_from(&fixture), &PORT),
        Err(CompileErrorV1::CapabilityAdmissionUnresolved(
            "routing.model_intelligence".into()
        ))
    );

    let inputs = Inputs::new();
    let base = small(vec![ws("ws-a", &["a/**"]), ws("ws-b", &["b/**"])]);
    let dep = |from: &str, to: &str| ProposedDependencyV1 {
        from: from.into(),
        to: to.into(),
        kind: PrecedenceKind::RequiresAccepted,
    };
    let with = |deps: Vec<ProposedDependencyV1>| {
        let mut p = base.clone();
        p.dependencies = deps;
        compile_small(&p, &inputs)
    };
    assert_eq!(
        with(vec![dep("t1", "t2"), dep("t2", "t1")]),
        Err(CompileErrorV1::PrecedenceCycle)
    );
    assert_eq!(
        with(vec![dep("t1", "t2"), dep("t1", "t2")]),
        Err(CompileErrorV1::DuplicateDependency {
            from: "t1".into(),
            to: "t2".into()
        })
    );
    assert_eq!(
        with(vec![dep("t1", "ghost")]),
        Err(CompileErrorV1::UnknownDependencyEndpoint("ghost".into()))
    );
    assert_eq!(
        with(vec![dep("t1", "t1")]),
        Err(CompileErrorV1::SelfDependency("t1".into()))
    );
    let mut duplicate = base.clone();
    duplicate.nodes.push(task("t1", "ws-a"));
    assert_eq!(
        compile_small(&duplicate, &inputs),
        Err(CompileErrorV1::DuplicateNode("t1".into()))
    );
    let mut empty = base;
    empty.nodes.clear();
    assert_eq!(
        compile_small(&empty, &inputs),
        Err(CompileErrorV1::EmptyProposal)
    );
}
