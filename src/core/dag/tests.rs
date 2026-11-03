//! Readiness over the frozen fixtures and the real interface verifier with a
//! file-backed source.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::*;
use crate::wire::ExecutionGraphWireV1;
use crate::{ExecutionGraphEnvelope, GraphEdge, GraphNode, GraphNodeKind, GraphNodeState as S};

fn fixture(name: &str) -> ExecutionGraphEnvelope {
    let path = format!(
        "{}/../../fixtures/graphs/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    ExecutionGraphWireV1::from_json(&std::fs::read_to_string(path).unwrap())
        .unwrap()
        .to_envelope()
        .unwrap()
}

/// Steps node states in a successor envelope (test-only state stepping).
fn with_states(env: &ExecutionGraphEnvelope, states: &[(&str, S)]) -> ExecutionGraphEnvelope {
    let nodes = env
        .graph()
        .nodes()
        .map(|n| match states.iter().find(|(id, _)| *id == n.node_id()) {
            Some((_, s)) => n.clone().with_state(*s),
            None => n.clone(),
        })
        .collect();
    let edges = env.graph().edges().cloned().collect();
    let graph = crate::ExecutionGraph::from_parts(env.graph_id(), nodes, edges).unwrap();
    env.successor(graph, env.created_at().clone(), None)
        .unwrap()
}

fn ready(env: &ExecutionGraphEnvelope) -> Vec<String> {
    TaskDag::new(env, None).ready_nodes().ready
}

#[test]
fn fanout_fanin_fixture_ready_sets_per_step() {
    let env = fixture("02_parallel_fanout_fanin");
    assert_eq!(ready(&env), ["g"]);
    let env = with_states(&env, &[("g", S::Accepted)]);
    assert_eq!(ready(&env), ["a", "b", "c"]);
    let env = with_states(
        &env,
        &[("a", S::Accepted), ("b", S::Integrated), ("c", S::Running)],
    );
    assert!(ready(&env).is_empty());
    let env = with_states(&env, &[("c", S::Accepted)]);
    assert_eq!(ready(&env), ["j"]);
    assert_eq!(TaskDag::new(&env, None).task_state("j"), Some(S::Planned));
    let deps = TaskDag::new(&env, None).dependency_status("j").unwrap();
    assert_eq!(deps.len(), 3);
    assert!(deps.iter().all(|d| d.status.is_satisfied()));
}

#[test]
fn control_edge_does_not_block_readiness() {
    // Fixture 03: i2 has only incoming CONTROL edges (ON_REJECT, EXPANDS_INTO).
    let env = fixture("03_repair_expansion");
    assert_eq!(ready(&env), ["i2", "t"]);
    assert!(
        TaskDag::new(&env, None)
            .dependency_status("i2")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn human_gate_fixture_blocks_until_human() {
    let env = fixture("04_human_gate");
    assert_eq!(ready(&env), ["t"]);
    let env = with_states(&env, &[("t", S::Accepted), ("sec", S::Accepted)]);
    assert!(ready(&env).is_empty(), "HUMAN_REQUIRED gate is never ready");
    // Even PLANNED, a HUMAN_GATE kind node is not dispatchable.
    let env2 = with_states(&env, &[("h", S::Planned)]);
    assert!(ready(&env2).is_empty());
    let env = with_states(&env, &[("h", S::Accepted)]);
    assert_eq!(ready(&env), ["int"]);
}

#[test]
fn locked_node_never_ready() {
    let env = fixture("07_pro_graph_with_locked_node");
    let env = with_states(
        &env,
        &[
            ("g", S::Accepted),
            ("t", S::Accepted),
            ("route", S::Accepted),
            ("a3", S::Accepted),
        ],
    );
    assert!(ready(&env).is_empty());
    assert_eq!(
        TaskDag::new(&env, None).task_state("a4"),
        Some(S::LockedRequiresPro)
    );
}

#[test]
fn requires_integrated_needs_integrated() {
    let edge =
        GraphEdge::precedence("e", "g", "a", "b", PrecedenceKind::RequiresIntegrated).unwrap();
    for (state, expect) in [(S::Accepted, false), (S::Integrated, true)] {
        let env = graph(state, edge.clone());
        assert_eq!(ready(&env).contains(&"b".to_owned()), expect, "{state:?}");
    }
}

// --- REQUIRES_INTERFACE with a file-backed trusted source ----------------

const SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn graph(a_state: S, edge: GraphEdge) -> ExecutionGraphEnvelope {
    let a = GraphNode::new("a", "g", GraphNodeKind::TASK, a_state)
        .unwrap()
        .with_code_sha(SHA)
        .unwrap();
    let b = GraphNode::new("b", "g", GraphNodeKind::TASK, S::Planned).unwrap();
    let c = GraphNode::new("c", "g", GraphNodeKind::TASK, S::Planned).unwrap();
    let g = crate::ExecutionGraph::from_parts("g", vec![a, b, c], vec![edge]).unwrap();
    ExecutionGraphEnvelope::bind(
        &g,
        crate::ExecutionGraphEnvelopeFields::new(
            "P",
            "goal",
            crate::GraphVersionV1::try_new("4").unwrap(),
            crate::orchestration::OrchestrationDateTimeV1::try_new("2026-10-29T00:00:00Z").unwrap(),
            "policy",
        ),
    )
    .unwrap()
}

/// Records are `key=value` lines in `<dir>/records/<graph>/<edge>`; artifact
/// bytes live in `<dir>/artifacts/<artifact_id>`.
struct DirectorySource(PathBuf);

impl InterfaceArtifactSource for DirectorySource {
    fn interface_record(
        &self,
        graph_id: &str,
        edge_id: &str,
    ) -> Result<Option<InterfaceArtifactRecord>, InterfaceSourceUnavailable> {
        let path = self.0.join("records").join(graph_id).join(edge_id);
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(None);
        };
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .unwrap_or("")
                .to_owned()
        };
        Ok(Some(InterfaceArtifactRecord {
            graph_id: get("graph_id"),
            graph_version: get("graph_version"),
            edge_id: get("edge_id"),
            source_node_id: get("source"),
            consumer_node_id: get("consumer"),
            artifact_id: get("artifact_id"),
            code_sha: get("code_sha"),
            content_sha256: get("content_sha256"),
            provenance: match get("accepted").as_str() {
                "" => InterfaceProvenance::NotAccepted,
                id => InterfaceProvenance::Accepted {
                    acceptance_id: id.to_owned(),
                },
            },
        }))
    }
    fn artifact_content(
        &self,
        artifact_id: &str,
    ) -> Result<Option<Vec<u8>>, InterfaceSourceUnavailable> {
        Ok(std::fs::read(self.0.join("artifacts").join(artifact_id)).ok())
    }
}

struct Offline;
impl InterfaceArtifactSource for Offline {
    fn interface_record(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<InterfaceArtifactRecord>, InterfaceSourceUnavailable> {
        Err(InterfaceSourceUnavailable)
    }
    fn artifact_content(&self, _: &str) -> Result<Option<Vec<u8>>, InterfaceSourceUnavailable> {
        Err(InterfaceSourceUnavailable)
    }
}

fn store(dir: &Path, record: &[(&str, &str)], content: &[u8]) {
    std::fs::create_dir_all(dir.join("records/g")).unwrap();
    std::fs::create_dir_all(dir.join("artifacts")).unwrap();
    let text: String = record.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    std::fs::write(dir.join("records/g/ei"), text).unwrap();
    std::fs::write(dir.join("artifacts/iface-1"), content).unwrap();
}

fn hex256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn interface_evidence_real_verifier_positive_tampered_stale_and_cross_consumer() {
    let dir = std::env::temp_dir().join(format!("or-iface-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let content = b"pub trait AuthApi { fn login(&self); }\n";
    let digest = hex256(content);
    let good: Vec<(&str, &str)> = vec![
        ("graph_id", "g"),
        ("graph_version", "4"),
        ("edge_id", "ei"),
        ("source", "a"),
        ("consumer", "b"),
        ("artifact_id", "iface-1"),
        ("code_sha", SHA),
        ("content_sha256", &digest),
        ("accepted", "acc-7"),
    ];
    let edge =
        GraphEdge::precedence("ei", "g", "a", "b", PrecedenceKind::RequiresInterface).unwrap();
    // The predecessor is still RUNNING: interface parallelism.
    let env = graph(S::Running, edge);
    let source = DirectorySource(dir.clone());
    let dag = |s: &dyn InterfaceArtifactSource| -> (ReadyNodes, Vec<DependencyCheck>) {
        let d = TaskDag::new(&env, Some(InterfaceEvidenceVerifier::new(s)));
        (d.ready_nodes(), d.dependency_status("b").unwrap())
    };

    // No source / unreachable / missing record => unavailable, never ready.
    let none = TaskDag::new(&env, None);
    assert_eq!(none.ready_nodes().awaiting_evidence, ["b"]);
    assert!(!none.ready_nodes().ready.contains(&"b".to_owned()));
    assert_eq!(dag(&Offline).0.awaiting_evidence, ["b"]);
    assert_eq!(
        dag(&source).1[0].status,
        DependencyStatus::InterfaceRefused(InterfaceEvidenceRefusal::MissingRecord)
    );

    store(&dir, &good, content);
    let (ready, deps) = dag(&source);
    assert_eq!(ready.ready, ["b", "c"]);
    let DependencyStatus::InterfaceAdmitted(evidence) = &deps[0].status else {
        panic!("{deps:?}")
    };
    assert_eq!(
        (
            evidence.artifact_id(),
            evidence.code_sha(),
            evidence.acceptance_id()
        ),
        ("iface-1", SHA, "acc-7")
    );
    assert_eq!(evidence.content_sha256(), digest);

    let refused = |edit: (&str, &str), content: &[u8], expect: InterfaceEvidenceRefusal| {
        let record: Vec<_> = good
            .iter()
            .map(|(k, v)| if *k == edit.0 { (*k, edit.1) } else { (*k, *v) })
            .collect();
        store(&dir, &record, content);
        let (ready, deps) = dag(&source);
        assert_eq!(
            deps[0].status,
            DependencyStatus::InterfaceRefused(expect),
            "{edit:?}"
        );
        assert!(
            !ready.ready.contains(&"b".to_owned())
                && !ready.awaiting_evidence.contains(&"b".to_owned())
        );
    };
    use InterfaceEvidenceRefusal as R;
    refused(("graph_version", "3"), content, R::StaleVersion);
    refused(("consumer", "c"), content, R::WrongConsumer);
    refused(("source", "c"), content, R::WrongSource);
    refused(("graph_id", "h"), content, R::WrongGraph);
    refused(("edge_id", "other"), content, R::WrongEdge);
    refused(("accepted", ""), content, R::NotAccepted);
    refused(("code_sha", &"0".repeat(40)), content, R::CodeMismatch);
    refused(
        ("graph_id", "g"),
        b"pub trait AuthApi { fn logout(&self); }\n",
        R::ContentMismatch,
    );
    let _ = std::fs::remove_dir_all(&dir);
}
