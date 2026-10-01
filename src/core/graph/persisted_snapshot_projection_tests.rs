//! Real State boundary coverage for the persisted snapshot projection.
//!
//! Every fixture is written through the public State genesis API, the
//! repository is dropped, and the projection is reread through a fresh
//! read-only reader before the pure helper runs. Expected values are built
//! independently: each fixture lists explicit field literals from which both
//! the State request and a canonical reference [`ExecutionGraph`] are
//! constructed, and reference digests are anchored to the A1-accepted v1
//! vectors where one exists. The projector never supplies an oracle.
//!
//! Synthetic handles and the synthetic clock establish physical
//! compatibility only, not trusted live admission. Failure messages are fixed
//! labels; no content-bearing value is formatted.
//!
//! Not runnable through public State APIs (State has no public constructor
//! for `PersistedCurrentGraphV1` and already enforces these invariants):
//! `VersionMismatch`, `InvalidCapturedAt`, `InvalidProjection` from malformed
//! stored fields, and `DigestFailure` (State refuses empty node sets; the
//! encoded-length ceiling is impractical to persist). Those guards are
//! defensive only; their categories are checked for closed formatting here.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_state::{
    GraphCompilerRefV1, GraphGenesisReasonV1, GraphSourceEvidenceRefV1, PersistedCurrentGraphV1,
    ReadOnlyGraphReader, SqliteStateRepository, StateError, StateGraphEdgeRelationV1,
    StateGraphEdgeV1, StateGraphGenesisProvenanceV1, StateGraphGenesisV1, StateGraphNodeV1,
    TrustedClockV1, TrustedTimeSampleV1, TrustedTimeWatermarkV1,
};

use super::*;
use crate::orchestration::OrchestrationDateTimeV1;

use GraphSnapshotProjectionErrorV1 as E;

const CAPTURED_AT: &str = "2026-10-01T15:30:00.123456789Z";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

const MINIMAL: &str = "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f";
const LOCKED_NULL: &str = "2844778885981452c8db6a135f5cf005baed618100ccd3d9689fe19c7a14725c";

/// Literal frozen spelling paired with its canonical enum.
type St = (&'static str, GraphNodeState);
const PLANNED: St = ("PLANNED", GraphNodeState::Planned);
const STATES: [St; 15] = [
    PLANNED,
    ("READY", GraphNodeState::Ready),
    ("ADMITTED", GraphNodeState::Admitted),
    ("DISPATCHED", GraphNodeState::Dispatched),
    ("RUNNING", GraphNodeState::Running),
    ("AWAITING_REVIEW", GraphNodeState::AwaitingReview),
    ("PASSED", GraphNodeState::Passed),
    ("REJECTED", GraphNodeState::Rejected),
    ("REPAIRING", GraphNodeState::Repairing),
    ("ACCEPTED", GraphNodeState::Accepted),
    ("INTEGRATED", GraphNodeState::Integrated),
    ("BLOCKED", GraphNodeState::Blocked),
    ("LOCKED_REQUIRES_PRO", GraphNodeState::LockedRequiresPro),
    ("CANCELLED", GraphNodeState::Cancelled),
    ("HUMAN_REQUIRED", GraphNodeState::HumanRequired),
];

#[derive(Clone, Copy)]
enum Rel {
    P(&'static str, PrecedenceKind),
    C(&'static str, ControlKind),
}
const REQUIRES_ACCEPTED: Rel = Rel::P("REQUIRES_ACCEPTED", PrecedenceKind::RequiresAccepted);
const REQUIRES_INTEGRATED: Rel = Rel::P("REQUIRES_INTEGRATED", PrecedenceKind::RequiresIntegrated);
const REQUIRES_INTERFACE: Rel = Rel::P("REQUIRES_INTERFACE", PrecedenceKind::RequiresInterface);
const ON_PASS: Rel = Rel::C("ON_PASS", ControlKind::OnPass);
const ON_REJECT: Rel = Rel::C("ON_REJECT", ControlKind::OnReject);
const ON_FAILURE: Rel = Rel::C("ON_FAILURE", ControlKind::OnFailure);
const ON_BLOCKED: Rel = Rel::C("ON_BLOCKED", ControlKind::OnBlocked);
const ESCALATE: Rel = Rel::C("ESCALATE", ControlKind::Escalate);
const EXPANDS_INTO: Rel = Rel::C("EXPANDS_INTO", ControlKind::ExpandsInto);

/// Unwraps without formatting the error value.
fn ok<T, X>(result: Result<T, X>, label: &str) -> T {
    match result {
        Ok(value) => value,
        Err(_) => panic!("{label}"),
    }
}

#[derive(Clone)]
struct NodeSpec {
    id: String,
    kind: String,
    state: St,
    title: Option<String>,
    parent: Option<String>,
    attempt: Option<String>,
    caps: Option<Vec<&'static str>>,
    capsule: Option<String>,
    workstream: Option<String>,
    code_sha: Option<&'static str>,
    workspace: Option<String>,
    result: Option<String>,
    locked: Option<Option<String>>,
    created: Option<String>,
}

fn n(id: &str, kind: &str, state: St) -> NodeSpec {
    NodeSpec {
        id: id.into(),
        kind: kind.into(),
        state,
        title: None,
        parent: None,
        attempt: None,
        caps: None,
        capsule: None,
        workstream: None,
        code_sha: None,
        workspace: None,
        result: None,
        locked: None,
        created: None,
    }
}

impl NodeSpec {
    fn stored(&self, graph: &str) -> StateGraphNodeV1 {
        ok(
            StateGraphNodeV1::new(
                self.id.clone(),
                graph.into(),
                self.kind.clone(),
                self.state.0.into(),
                self.title.clone(),
                self.parent.clone(),
                self.attempt.clone(),
                self.caps
                    .as_ref()
                    .map(|caps| caps.iter().map(|c| (*c).to_owned()).collect()),
                self.capsule.clone(),
                self.workstream.clone(),
                self.code_sha.map(str::to_owned),
                self.workspace.clone(),
                self.result.clone(),
                self.locked.clone(),
                self.created.clone(),
            ),
            "fixture State node",
        )
    }

    fn reference(&self, graph: &str) -> GraphNode {
        let kind = ok(GraphNodeKind::new(self.kind.clone()), "fixture kind");
        let mut node = ok(
            GraphNode::new(self.id.as_str(), graph, kind, self.state.1),
            "fixture node",
        );
        if let Some(title) = &self.title {
            node = node.with_title(title.as_str());
        }
        if let Some(parent) = &self.parent {
            node = ok(node.with_parent_node_id(parent.as_str()), "fixture parent");
        }
        if let Some(attempt) = &self.attempt {
            let attempt = ok(
                GraphNodeAttemptNumber::try_new(attempt.as_str()),
                "fixture attempt",
            );
            node = node.with_attempt_number(attempt);
        }
        if let Some(caps) = &self.caps {
            let caps = caps
                .iter()
                .map(|c| ok(CapabilityName::new(*c), "fixture capability"))
                .collect();
            node = node.with_required_capabilities(caps);
        }
        if let Some(capsule) = &self.capsule {
            node = node.with_task_capsule_ref(capsule.as_str());
        }
        if let Some(workstream) = &self.workstream {
            node = ok(
                node.with_workstream_id(workstream.as_str()),
                "fixture workstream",
            );
        }
        if let Some(sha) = self.code_sha {
            node = ok(node.with_code_sha(sha), "fixture code_sha");
        }
        if let Some(workspace) = &self.workspace {
            node = ok(
                node.with_workspace_id(workspace.as_str()),
                "fixture workspace",
            );
        }
        if let Some(result) = &self.result {
            node = node.with_result_ref(result.as_str());
        }
        match &self.locked {
            None => {}
            Some(None) => node = node.with_null_locked_reason(),
            Some(Some(reason)) => node = node.with_locked_reason(reason.as_str()),
        }
        if let Some(created) = &self.created {
            node = node.with_created_in_version(ok(
                GraphVersionV1::try_new(created.as_str()),
                "fixture version",
            ));
        }
        node
    }
}

#[derive(Clone)]
struct EdgeSpec {
    id: String,
    from: String,
    to: String,
    rel: Rel,
    note: Option<String>,
}

fn e(id: &str, from: &str, to: &str, rel: Rel) -> EdgeSpec {
    EdgeSpec {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        rel,
        note: None,
    }
}

impl EdgeSpec {
    fn noted(mut self, note: &str) -> Self {
        self.note = Some(note.into());
        self
    }

    fn stored(&self, graph: &str) -> StateGraphEdgeV1 {
        let relation = match self.rel {
            Rel::P(kind, _) => StateGraphEdgeRelationV1::Precedence(kind.into()),
            Rel::C(kind, _) => StateGraphEdgeRelationV1::Control(kind.into()),
        };
        ok(
            StateGraphEdgeV1::new(
                self.id.clone(),
                graph.into(),
                self.from.clone(),
                self.to.clone(),
                relation,
                self.note.clone(),
            ),
            "fixture State edge",
        )
    }

    fn reference(&self, graph: &str) -> GraphEdge {
        let (id, from, to) = (self.id.as_str(), self.from.as_str(), self.to.as_str());
        let edge = match self.rel {
            Rel::P(_, kind) => GraphEdge::precedence(id, graph, from, to, kind),
            Rel::C(_, kind) => GraphEdge::control(id, graph, from, to, kind),
        };
        let edge = ok(edge, "fixture edge");
        match &self.note {
            Some(note) => edge.with_note(note.as_str()),
            None => edge,
        }
    }
}

#[derive(Clone)]
struct Fixture {
    project: String,
    graph: String,
    nodes: Vec<NodeSpec>,
    edges: Vec<EdgeSpec>,
}

fn fixture(project: &str, graph: &str, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec>) -> Fixture {
    Fixture {
        project: project.into(),
        graph: graph.into(),
        nodes,
        edges,
    }
}

impl Fixture {
    fn in_project(mut self, project: &str) -> Self {
        self.project = project.into();
        self
    }

    /// Independent canonical reference; panics for invalid topology.
    fn reference_digest(&self) -> String {
        let graph = ok(
            ExecutionGraph::from_parts(
                self.graph.as_str(),
                self.nodes
                    .iter()
                    .map(|n| n.reference(&self.graph))
                    .collect(),
                self.edges
                    .iter()
                    .map(|e| e.reference(&self.graph))
                    .collect(),
            ),
            "fixture reference graph",
        );
        ok(graph_content_digest_v1(&graph), "fixture reference digest")
    }

    /// Children are written in descending ID byte order, the reverse of the
    /// expected presentation order.
    fn genesis(&self, digest: &str) -> StateGraphGenesisV1 {
        let mut nodes = self.nodes.clone();
        nodes.sort_by(|a, b| b.id.as_bytes().cmp(a.id.as_bytes()));
        let mut edges = self.edges.clone();
        edges.sort_by(|a, b| b.id.as_bytes().cmp(a.id.as_bytes()));
        let provenance = StateGraphGenesisProvenanceV1::new(
            ok(
                GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                "synthetic compiler",
            ),
            ok(
                GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW"),
                "synthetic evidence",
            ),
            GraphGenesisReasonV1::InitialCompilation,
        );
        ok(
            StateGraphGenesisV1::new(
                self.project.clone(),
                self.graph.clone(),
                "synthetic-goal".into(),
                "synthetic-policy".into(),
                Some(None),
                Some(vec!["SPEC.md".into(), "SPEC.md".into()]),
                None,
                digest.into(),
                nodes.iter().map(|n| n.stored(&self.graph)).collect(),
                edges.iter().map(|e| e.stored(&self.graph)).collect(),
                provenance,
            ),
            "fixture genesis",
        )
    }

    /// Expected output built from the literals, in ascending ID byte order.
    fn expected(&self, digest: &str) -> GraphSnapshot {
        let mut nodes = self.nodes.clone();
        nodes.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        let states = nodes
            .iter()
            .map(|n| {
                ok(
                    GraphSnapshotNodeState::try_new(
                        n.id.as_str(),
                        n.state.1,
                        Some(ok(GraphNodeKind::new(n.kind.clone()), "expected kind")),
                        None,
                        n.code_sha.map(str::to_owned),
                    ),
                    "expected node state",
                )
            })
            .collect();
        ok(
            GraphSnapshot::try_new(
                self.graph.as_str(),
                ok(GraphVersionV1::try_new("1"), "expected version"),
                ok(
                    OrchestrationDateTimeV1::try_new(CAPTURED_AT),
                    "expected time",
                ),
                states,
                Some(digest.into()),
                None,
            ),
            "expected snapshot",
        )
    }
}

/// Clearly synthetic trusted clock; establishes no production time source.
struct SyntheticClock;
impl TrustedClockV1 for SyntheticClock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: CAPTURED_AT.into(),
            clock_source_id: "synthetic-a3-038-test-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

static NEXT_STORE: AtomicU64 = AtomicU64::new(0);

/// Temporary State store, removed with its WAL/SHM files on drop.
struct Store(PathBuf);

impl Store {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        Self(std::env::temp_dir().join(format!(
            "receipts-a3-038-{}-{nonce}-{}.db",
            std::process::id(),
            NEXT_STORE.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn create(&self, fixture: &Fixture, digest: &str) {
        let mut repository = ok(SqliteStateRepository::open(&self.0), "open State");
        ok(
            repository.create_initial_graph_v1(&SyntheticClock, fixture.genesis(digest)),
            "create genesis",
        );
    }

    fn read(&self, project: &str, graph: &str) -> PersistedCurrentGraphV1 {
        let reader = ok(ReadOnlyGraphReader::open_existing(&self.0), "open reader");
        let found = ok(reader.read_current_v1(project, graph), "read projection");
        found.unwrap_or_else(|| panic!("persisted graph absent"))
    }

    fn watermark(&self, project: &str) -> Option<TrustedTimeWatermarkV1> {
        let repository = ok(SqliteStateRepository::open(&self.0), "open State");
        ok(
            repository.find_trusted_time_watermark(project),
            "read watermark",
        )
    }

    /// Writes, closes, reopens read-only and returns the physical projection.
    fn persist(&self, fixture: &Fixture, digest: &str) -> PersistedCurrentGraphV1 {
        self.create(fixture, digest);
        self.read(&fixture.project, &fixture.graph)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

fn project(
    persisted: &PersistedCurrentGraphV1,
    fixture: &Fixture,
) -> Result<GraphSnapshot, GraphSnapshotProjectionErrorV1> {
    project_persisted_current_graph_v1(persisted, &fixture.project, &fixture.graph)
}

fn one(spec: NodeSpec) -> Fixture {
    fixture("p", "g", vec![spec], Vec::new())
}

fn node_g(id: &str) -> NodeSpec {
    n(id, "TASK", PLANNED)
}

/// Accepted v1 vectors (`digest_profile_2026-09-27/vectors.json`), copied
/// verbatim, with fixtures mirroring `inputs.json` field for field.
fn accepted_vectors() -> Vec<(&'static str, Fixture, &'static str)> {
    let reviewer = ["graph.core", "review.independent_a4", "graph.core"];
    let reordered = ["graph.core", "graph.core", "review.independent_a4"];
    let mut frozen = Vec::new();
    for (index, state) in STATES.iter().enumerate() {
        frozen.push(n(&format!("s{index:02}"), "TASK", *state));
    }
    let unicode_full = fixture(
        "p",
        "g",
        vec![
            NodeSpec {
                title: Some("\u{e9}\u{0}:😀".into()),
                parent: Some("a".into()),
                attempt: Some("18446744073709551616".into()),
                caps: Some(reviewer.to_vec()),
                capsule: Some(String::new()),
                workstream: Some("工".into()),
                code_sha: Some(SHA),
                workspace: Some("区".into()),
                result: Some(" résultat ".into()),
                locked: Some(Some(String::new())),
                created: Some("18446744073709551617".into()),
                ..n("😀", "custom:种", STATES[1])
            },
            node_g("\u{e000}"),
            n("a", "GOAL", STATES[9]),
        ],
        vec![
            e("😀", "😀", "a", ON_REJECT).noted("\u{0}résumé"),
            e("\u{e000}", "a", "😀", REQUIRES_INTERFACE).noted(""),
        ],
    );
    vec![
        ("minimal", one(node_g("n")), MINIMAL),
        (
            "locked_null",
            one(NodeSpec {
                locked: Some(None),
                ..node_g("n")
            }),
            LOCKED_NULL,
        ),
        (
            "locked_empty",
            one(NodeSpec {
                locked: Some(Some(String::new())),
                ..node_g("n")
            }),
            "52bec8623576f5cdce3424500cebf2bb0138ea05acfaa2d4daf3eeff224d4aa7",
        ),
        (
            "title_empty",
            one(NodeSpec {
                title: Some(String::new()),
                ..node_g("n")
            }),
            "96eefab41ac68bb7362c342d7e82b3884d3a4e7c565be04a88167fe30e5935bd",
        ),
        (
            "capabilities_empty",
            one(NodeSpec {
                caps: Some(Vec::new()),
                ..node_g("n")
            }),
            "1790cf2374c214e06fb8964d72016164b7737cb358784c502091ec4e528739f5",
        ),
        (
            "capabilities_duplicates",
            one(NodeSpec {
                caps: Some(reviewer.to_vec()),
                ..node_g("n")
            }),
            "56a77bf8a7987bdba857255be96ce1c8b7f06510c5d513f01aa83a629b035478",
        ),
        (
            "capabilities_reordered",
            one(NodeSpec {
                caps: Some(reordered.to_vec()),
                ..node_g("n")
            }),
            "371e5dfb2229d7ee8059398970799ddf7b3eaf4cf43d7a032df15e7e1f3f9e44",
        ),
        (
            "unicode_composed",
            one(NodeSpec {
                title: Some("\u{e9}".into()),
                ..node_g("n")
            }),
            "77c6161d253bb7d8e4f117c53ec23f46d028f36f98cce9ce033f84e583a19b11",
        ),
        (
            "unicode_decomposed",
            one(NodeSpec {
                title: Some("e\u{301}".into()),
                ..node_g("n")
            }),
            "ef1fe141eb0737df5d7aa5d8ee24764c3b929349c75c020df13efbaeba2a0d0b",
        ),
        (
            "unicode_full",
            unicode_full,
            "068696ce17b2b0843fe3ffdc623874038c2419237430836db19357beca6ab3df",
        ),
        (
            "long_decimals",
            one(NodeSpec {
                attempt: Some(format!("1{}", "0".repeat(10_000))),
                created: Some("9".repeat(10_001)),
                ..node_g("n")
            }),
            "f696b293400e0061af6bbc325647bebf5657865c08ff0062a46ba4196ae7693c",
        ),
        (
            "control_self_loop_same_namespace_id",
            fixture("p", "g", vec![node_g("n")], vec![e("n", "n", "n", ON_PASS)]),
            "825438f3af72290860d1a056400681e42f4b13989af7ffd3092194d5ea666563",
        ),
        (
            "all_frozen_vocabularies",
            fixture(
                "p",
                "g",
                frozen,
                vec![
                    e("p0", "s00", "s01", REQUIRES_ACCEPTED),
                    e("p1", "s01", "s02", REQUIRES_INTEGRATED),
                    e("p2", "s02", "s03", REQUIRES_INTERFACE),
                    e("c0", "s00", "s00", ON_PASS),
                    e("c1", "s00", "s00", ON_REJECT),
                    e("c2", "s00", "s00", ON_FAILURE),
                    e("c3", "s00", "s00", ON_BLOCKED),
                    e("c4", "s00", "s00", ESCALATE),
                    e("c5", "s00", "s00", EXPANDS_INTO),
                ],
            ),
            "e6bd7c9c086e0635e5f9c89bcc80c5ddf31054f89ad834818f3747f77a18ea30",
        ),
    ]
}

/// One rich node carrying all 15 fields plus two plain nodes and one edge.
fn field_base(project: &str) -> Fixture {
    fixture(
        project,
        "g",
        vec![
            NodeSpec {
                title: Some("e\u{301}\u{0}😀".into()),
                parent: Some("parent-é".into()),
                attempt: Some("18446744073709551616".into()),
                caps: Some(vec!["graph.core", "review.independent_a4", "graph.core"]),
                capsule: Some(String::new()),
                workstream: Some("工".into()),
                code_sha: Some(SHA),
                workspace: Some("区".into()),
                result: Some(" résultat\u{0} ".into()),
                locked: Some(Some(String::new())),
                created: Some("9".repeat(10_001)),
                ..n("x-ñ\u{0}", "custom:种\u{0}", STATES[5])
            },
            node_g("w"),
            node_g("y"),
        ],
        vec![e("edge-é", "w", "y", ON_PASS).noted("")],
    )
}

#[test]
fn accepted_vectors_round_trip_through_state_and_project_exactly() {
    assert_eq!(MINIMAL.len(), 64);
    let store = Store::new();
    for (id, vector, digest) in accepted_vectors() {
        // Project identity is not hashed: each vector gets its own project.
        let vector = vector.in_project(&format!("p-{id}"));
        assert!(
            vector.reference_digest() == digest,
            "reference digest differs from accepted vector"
        );
        let persisted = store.persist(&vector, digest);
        let snapshot = project(&persisted, &vector);
        assert!(
            snapshot == Ok(vector.expected(digest)),
            "accepted vector projection differs"
        );
    }
}

#[test]
fn minimal_snapshot_has_exact_outer_fields_and_no_summary() {
    let store = Store::new();
    let minimal = one(node_g("n"));
    let persisted = store.persist(&minimal, MINIMAL);
    let snapshot = ok(project(&persisted, &minimal), "minimal projection");
    assert_eq!(snapshot.graph_id(), "g");
    assert_eq!(snapshot.graph_version().as_str(), "1");
    assert_eq!(snapshot.captured_at().as_str(), CAPTURED_AT);
    assert_eq!(
        snapshot.captured_at().as_str(),
        persisted.created_at().as_str()
    );
    assert_eq!(snapshot.resulting_digest(), Some(MINIMAL));
    assert!(snapshot.summary().is_none(), "summary must be absent");
    let [state] = snapshot.node_states() else {
        panic!("expected exactly one node state");
    };
    assert_eq!(state.node_id(), "n");
    assert_eq!(state.state(), GraphNodeState::Planned);
    assert_eq!(state.kind().map(GraphNodeKind::as_str), Some("TASK"));
    assert_eq!(state.locked(), None);
    assert_eq!(state.code_sha(), None);
}

#[test]
fn every_state_and_lock_reason_form_projects_locked_none() {
    let forms: [(&str, Option<Option<String>>); 4] = [
        ("g-absent", None),
        ("g-null", Some(None)),
        ("g-empty", Some(Some(String::new()))),
        ("g-text", Some(Some("requires pro\u{0}é".into()))),
    ];
    let store = Store::new();
    let mut digests = Vec::new();
    for (graph, locked) in &forms {
        let nodes = STATES
            .iter()
            .enumerate()
            .map(|(index, state)| NodeSpec {
                locked: locked.clone(),
                code_sha: (index % 2 == 0).then_some(SHA),
                ..n(&format!("s{index:02}"), "TASK", *state)
            })
            .collect();
        let graph = fixture("p", graph, nodes, Vec::new());
        let digest = graph.reference_digest();
        let persisted = store.persist(&graph, &digest);
        let snapshot = ok(project(&persisted, &graph), "lock-form projection");
        assert!(
            snapshot == graph.expected(&digest),
            "lock-form projection differs"
        );
        assert_eq!(snapshot.node_states().len(), 15);
        for (state, (_, expected)) in snapshot.node_states().iter().zip(STATES) {
            assert_eq!(state.state(), expected);
            assert_eq!(state.locked(), None, "locked must stay absent");
        }
        digests.push((graph, digest));
    }
    let distinct: HashSet<_> = digests.iter().map(|(_, digest)| digest.clone()).collect();
    assert_eq!(
        distinct.len(),
        4,
        "lock-reason forms must change the child digest"
    );

    // Content of one lock form stored with another form's digest is refused.
    for (index, (graph, _)) in digests.iter().enumerate() {
        let (_, other) = &digests[(index + 1) % digests.len()];
        let swapped = graph.clone().in_project(&format!("swap-{index}"));
        let persisted = store.persist(&swapped, other);
        assert!(
            project(&persisted, &swapped) == Err(E::DigestMismatch),
            "lock swap accepted"
        );
    }
}

#[test]
fn full_fidelity_graph_preserves_every_field_kind_and_byte_order() {
    let store = Store::new();
    let rich = NodeSpec {
        title: Some("e\u{301}\u{0}😀".into()),
        parent: Some("Z".into()),
        attempt: Some("18446744073709551616".into()),
        caps: Some(vec!["graph.core", "review.independent_a4", "graph.core"]),
        capsule: Some(String::new()),
        workstream: Some("工".into()),
        code_sha: Some(SHA),
        workspace: Some("区".into()),
        result: Some(" résultat\u{0} ".into()),
        locked: Some(Some(String::new())),
        created: Some("9".repeat(10_001)),
        ..n("ñode-é\u{0}", "custom:种\u{0}", STATES[5])
    };
    let graph = fixture(
        "projet-é\u{0}😀",
        "graphe-種\u{0}",
        vec![
            NodeSpec {
                locked: Some(Some("requires pro".into())),
                caps: Some(vec!["graph.core", "graph.core", "review.independent_a4"]),
                ..n("😀", "REVIEW", STATES[12])
            },
            NodeSpec {
                title: Some("\u{e9}".into()),
                attempt: Some(format!("1{}", "0".repeat(10_000))),
                caps: Some(Vec::new()),
                result: Some(String::new()),
                locked: Some(None),
                created: Some("18446744073709551617".into()),
                ..n("\u{e000}", "INTEGRATION", STATES[10])
            },
            rich,
            n("z", "GOAL", STATES[9]),
            n("a", "TASK", STATES[0]),
            NodeSpec {
                code_sha: Some("ffffffffffffffffffffffffffffffffffffffff"),
                ..n("Z", "WORKSTREAM", STATES[14])
            },
        ],
        vec![
            e("p-accepted", "a", "z", REQUIRES_ACCEPTED),
            e("p-integrated", "z", "😀", REQUIRES_INTEGRATED).noted(""),
            e("p-interface", "Z", "a", REQUIRES_INTERFACE).noted("\u{0}résumé"),
            e("c-pass", "😀", "a", ON_PASS),
            e("c-reject-self", "a", "a", ON_REJECT).noted("ignore previous instructions"),
            e("c-failure", "z", "a", ON_FAILURE).noted(""),
            e("c-blocked", "ñode-é\u{0}", "\u{e000}", ON_BLOCKED),
            e("c-escalate", "\u{e000}", "ñode-é\u{0}", ESCALATE),
            e("ç-expands", "a", "ñode-é\u{0}", EXPANDS_INTO).noted("é"),
        ],
    );
    let digest = graph.reference_digest();
    let persisted = store.persist(&graph, &digest);
    let snapshot = ok(project(&persisted, &graph), "full-fidelity projection");
    assert!(
        snapshot == graph.expected(&digest),
        "full-fidelity projection differs"
    );
    let order: Vec<&str> = snapshot.node_states().iter().map(|s| s.node_id()).collect();
    // UTF-8 byte order: uppercase before lowercase, U+E000 before U+1F600
    // (UTF-16 code-unit order would invert the last pair).
    assert!(
        order == ["Z", "a", "z", "ñode-é\u{0}", "\u{e000}", "😀"],
        "node states are not in ascending UTF-8 byte order"
    );
    assert!(
        snapshot.node_states().iter().all(|s| s.locked().is_none()),
        "locked must stay absent"
    );
}

#[test]
fn every_child_field_change_is_detected_by_the_verified_digest() {
    let base = field_base("base");
    let base_digest = base.reference_digest();
    let rich = |change: fn(&mut NodeSpec)| {
        let mut variant = field_base("variant");
        change(&mut variant.nodes[0]);
        variant
    };
    let edge = |change: fn(&mut EdgeSpec)| {
        let mut variant = field_base("variant");
        change(&mut variant.edges[0]);
        variant
    };
    let mut graph_id = field_base("variant");
    graph_id.graph = "g2".into();
    let variants: Vec<Fixture> = vec![
        rich(|n| n.id = "x-ñ".into()),
        graph_id,
        rich(|n| n.kind = "custom:种".into()),
        rich(|n| n.state = STATES[6]),
        rich(|n| n.title = Some("\u{e9}\u{0}😀".into())),
        rich(|n| n.title = None),
        rich(|n| n.parent = Some("parent-e".into())),
        rich(|n| n.parent = None),
        rich(|n| n.attempt = Some("18446744073709551617".into())),
        rich(|n| n.caps = Some(vec!["graph.core", "graph.core", "review.independent_a4"])),
        rich(|n| n.caps = Some(vec!["graph.core", "review.independent_a4"])),
        rich(|n| n.caps = Some(Vec::new())),
        rich(|n| n.caps = None),
        rich(|n| n.capsule = None),
        rich(|n| n.workstream = Some("工 ".into())),
        rich(|n| n.code_sha = None),
        rich(|n| n.code_sha = Some("1123456789abcdef0123456789abcdef01234567")),
        rich(|n| n.workspace = None),
        rich(|n| n.result = Some(" résultat ".into())),
        rich(|n| n.locked = Some(None)),
        rich(|n| n.locked = None),
        rich(|n| n.locked = Some(Some("x".into()))),
        rich(|n| n.created = Some("9".repeat(10_000))),
        rich(|n| n.created = None),
        edge(|e| e.id = "edge-e".into()),
        edge(|e| e.from = "y".into()),
        edge(|e| e.to = "w".into()),
        edge(|e| e.rel = ON_REJECT),
        edge(|e| e.rel = REQUIRES_ACCEPTED),
        edge(|e| e.note = None),
        edge(|e| e.note = Some("\u{0}".into())),
    ];

    let store = Store::new();
    let persisted = store.persist(&base, &base_digest);
    assert!(
        project(&persisted, &base) == Ok(base.expected(&base_digest)),
        "base refused"
    );

    let mut seen = HashSet::from([base_digest.clone()]);
    for (index, variant) in variants.into_iter().enumerate() {
        let variant = variant.in_project(&format!("variant-{index}"));
        assert!(
            seen.insert(variant.reference_digest()),
            "field change left digest unchanged"
        );
        // Changed content persisted with the base digest must be refused.
        let persisted = store.persist(&variant, &base_digest);
        assert!(
            project(&persisted, &variant) == Err(E::DigestMismatch),
            "changed field accepted"
        );
    }
}

#[test]
fn wrong_expected_project_or_graph_is_identity_mismatch() {
    let store = Store::new();
    let minimal = one(node_g("n"));
    let persisted = store.persist(&minimal, MINIMAL);
    let other = minimal.clone().in_project("q");
    let other_persisted = store.persist(&other, MINIMAL);
    for (expected_project, expected_graph) in [
        ("q", "g"),
        ("p", "h"),
        ("q", "h"),
        ("P", "g"),
        ("p", "G"),
        ("p\u{0}", "g"),
        ("p", "g "),
        ("", ""),
    ] {
        let refused =
            project_persisted_current_graph_v1(&persisted, expected_project, expected_graph);
        assert!(
            refused == Err(E::IdentityMismatch),
            "selector mismatch accepted"
        );
    }
    // Same content in another project: its digest matches, its identity does not.
    let crossed = project_persisted_current_graph_v1(&other_persisted, "p", "g");
    assert!(
        crossed == Err(E::IdentityMismatch),
        "cross-project projection accepted"
    );
    assert!(
        project(&other_persisted, &other) == Ok(other.expected(MINIMAL)),
        "other refused"
    );
}

#[test]
fn precedence_cycles_are_invalid_topology_before_digest_comparison() {
    let wrong = "0".repeat(64);
    let cycles = [
        fixture(
            "p",
            "reciprocal",
            vec![node_g("a"), node_g("b")],
            vec![
                e("ab", "a", "b", REQUIRES_ACCEPTED),
                e("ba", "b", "a", REQUIRES_INTERFACE),
            ],
        ),
        fixture(
            "p",
            "self",
            vec![node_g("a")],
            vec![e("aa", "a", "a", REQUIRES_INTEGRATED)],
        ),
        fixture(
            "p",
            "mixed",
            vec![node_g("a"), node_g("b"), node_g("c")],
            vec![
                e("ab", "a", "b", REQUIRES_ACCEPTED),
                e("bc", "b", "c", REQUIRES_ACCEPTED),
                e("ca", "c", "a", REQUIRES_ACCEPTED),
                e("cc", "c", "c", ON_PASS),
            ],
        ),
    ];
    let store = Store::new();
    for cycle in &cycles {
        // State persists physically valid cycles with a well-formed digest.
        let persisted = store.persist(cycle, &wrong);
        assert!(
            project(&persisted, cycle) == Err(E::InvalidTopology),
            "cycle accepted"
        );
        // Selector checks precede topology.
        let selector = project_persisted_current_graph_v1(&persisted, "q", &cycle.graph);
        assert!(
            selector == Err(E::IdentityMismatch),
            "topology checked before selector"
        );
    }
}

#[test]
fn control_loops_with_independent_digest_project_successfully() {
    let store = Store::new();
    let loops = fixture(
        "p",
        "g",
        vec![node_g("a"), node_g("b")],
        vec![
            e("ab", "a", "b", ON_REJECT),
            e("ba", "b", "a", ESCALATE),
            e("aa", "a", "a", EXPANDS_INTO).noted("loop"),
            e("p", "a", "b", REQUIRES_ACCEPTED),
        ],
    );
    let digest = loops.reference_digest();
    let persisted = store.persist(&loops, &digest);
    assert!(
        project(&persisted, &loops) == Ok(loops.expected(&digest)),
        "control loop refused"
    );
}

#[test]
fn wrong_stored_digest_on_valid_graph_is_digest_mismatch() {
    let minimal = one(node_g("n"));
    assert!(
        minimal.reference_digest() == MINIMAL,
        "minimal vector differs"
    );
    let mut flipped = MINIMAL.as_bytes().to_vec();
    flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
    let flipped = ok(String::from_utf8(flipped), "flipped digest");
    let store = Store::new();
    for (index, wrong) in [LOCKED_NULL, flipped.as_str()].into_iter().enumerate() {
        assert!(wrong != MINIMAL, "wrong digest must differ");
        let stored = minimal.clone().in_project(&format!("p-{index}"));
        let persisted = store.persist(&stored, wrong);
        assert!(
            project(&persisted, &stored) == Err(E::DigestMismatch),
            "wrong digest accepted"
        );
        let selector = project_persisted_current_graph_v1(&persisted, "p", "g");
        assert!(
            selector == Err(E::IdentityMismatch),
            "digest checked before selector"
        );
    }
}

#[test]
fn error_categories_are_closed_payload_free_and_sourceless() {
    use std::error::Error as _;
    let variants = [
        (E::IdentityMismatch, "IdentityMismatch"),
        (E::VersionMismatch, "VersionMismatch"),
        (E::InvalidProjection, "InvalidProjection"),
        (E::InvalidTopology, "InvalidTopology"),
        (E::DigestMismatch, "DigestMismatch"),
        (E::DigestFailure, "DigestFailure"),
        (E::InvalidCapturedAt, "InvalidCapturedAt"),
    ];
    let mut displays = HashSet::new();
    for (error, name) in variants {
        // Formatting a constructed variant checks diagnostics only; it is
        // not evidence that the projector reached that branch.
        assert_eq!(format!("{error:?}"), name);
        let display = error.to_string();
        assert!(
            !display.is_empty() && display.is_ascii(),
            "display must be fixed text"
        );
        assert!(displays.insert(display), "display text must be distinct");
        assert!(error.source().is_none(), "no nested source");
    }

    // Runtime refusals over sentinel content never echo it.
    let sentinel_digest = "deadbeef".repeat(8);
    let sentinel = fixture(
        "SENTINEL-PROJECT-91c4",
        "SENTINEL-GRAPH-5be2",
        vec![NodeSpec {
            title: Some("SENTINEL-TITLE-77e1".into()),
            ..node_g("SENTINEL-NODE-0d7a")
        }],
        vec![e(
            "SENTINEL-EDGE-3f19",
            "SENTINEL-NODE-0d7a",
            "SENTINEL-NODE-0d7a",
            ON_PASS,
        )],
    );
    let computed = sentinel.reference_digest();
    let mut cyclic = sentinel.clone().in_project("SENTINEL-PROJECT-cycle");
    cyclic.edges[0].rel = REQUIRES_ACCEPTED;
    let store = Store::new();
    let persisted = store.persist(&sentinel, &sentinel_digest);
    let cyclic_persisted = store.persist(&cyclic, &sentinel_digest);
    let refusals = [
        (
            project_persisted_current_graph_v1(
                &persisted,
                "SENTINEL-EXPECTED-PROJECT",
                "SENTINEL-GRAPH-5be2",
            ),
            E::IdentityMismatch,
        ),
        (project(&cyclic_persisted, &cyclic), E::InvalidTopology),
        (project(&persisted, &sentinel), E::DigestMismatch),
    ];
    for (refused, category) in refusals {
        let Err(error) = refused else {
            panic!("sentinel fixture unexpectedly projected");
        };
        assert_eq!(error, category);
        let text = format!("{error} {error:?}");
        for secret in [
            "SENTINEL",
            "91c4",
            "5be2",
            "0d7a",
            "77e1",
            "3f19",
            &sentinel_digest[..16],
            &computed[..16],
        ] {
            assert!(
                !text.contains(secret),
                "error diagnostics echo supplied content"
            );
        }
        assert!(error.source().is_none(), "no nested source");
    }
}

#[test]
fn projection_never_mutates_input_projection_schema_or_watermark() {
    let store = Store::new();
    let graph = field_base("p");
    let digest = graph.reference_digest();
    store.create(&graph, &digest);
    let watermark = store.watermark("p");
    assert!(watermark.is_some(), "genesis must record a watermark");
    let persisted = store.read("p", "g");
    let before = persisted.clone();

    let first = project(&persisted, &graph);
    assert!(first == Ok(graph.expected(&digest)), "projection refused");
    assert!(
        project(&persisted, &graph) == first,
        "projection is not deterministic"
    );
    let refused = project_persisted_current_graph_v1(&persisted, "q", "g");
    assert!(
        refused == Err(E::IdentityMismatch),
        "selector mismatch accepted"
    );

    let mut cyclic = graph.clone().in_project("cyclic");
    cyclic.edges[0].rel = REQUIRES_ACCEPTED;
    cyclic.edges.push(e("back", "y", "w", REQUIRES_ACCEPTED));
    let cyclic_persisted = store.persist(&cyclic, &digest);
    let cyclic_before = cyclic_persisted.clone();
    let cyclic_watermark = store.watermark("cyclic");
    assert!(
        project(&cyclic_persisted, &cyclic) == Err(E::InvalidTopology),
        "cycle accepted"
    );

    assert!(persisted == before, "input projection changed");
    assert!(
        cyclic_persisted == cyclic_before,
        "refused input projection changed"
    );
    // A fresh read-only reader still verifies the supported schema and
    // returns the identical projections; the public watermarks are unchanged.
    assert!(store.read("p", "g") == before, "stored projection changed");
    assert!(
        store.read("cyclic", "g") == cyclic_before,
        "refused stored projection changed"
    );
    assert!(store.watermark("p") == watermark, "watermark changed");
    assert!(
        store.watermark("cyclic") == cyclic_watermark,
        "refused watermark changed"
    );
}
