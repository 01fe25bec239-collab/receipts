//! Fixed-vector and boundary coverage for the graph child-content digest v1.
//!
//! Expected bytes and hashes are the A1-accepted scoping vectors
//! (`digest_profile_2026-09-27/vectors.json`, SHA-256
//! `91ba1806e1e16cd7516b6cb3bec3ece70fed401c27e29acca3b7e46ff3271670`), copied
//! verbatim; they are not computed by the implementation under test. Fixture
//! graphs are built only through canonical constructors.
//!
//! The context-only positive cases `other_project_same_content` and
//! `excluded_metadata_changed` vary only project or outer metadata, which the
//! kernel does not receive: both deliberately map to the `minimal` graph and
//! must reproduce its digest.
//!
//! The 33 negative scoping fixtures map as follows:
//!
//! * Kernel [`GraphContentDigestError::EmptyGraph`]: `reject_empty_nodes`.
//! * Canonical constructor rejection before any graph exists (checked in
//!   `negative_fixtures_fail_before_reaching_the_kernel`, and by existing
//!   execution_graph/capability/node_state regression tests):
//!   `reject_duplicate_nodes`, `reject_wrong_child_graph`,
//!   `reject_wrong_outer_graph` (containment half), `reject_empty_id`,
//!   `reject_long_id`, `reject_invalid_capability`, `reject_uppercase_sha`,
//!   `reject_decimal_zero`, `reject_decimal_leading_zero`,
//!   `reject_decimal_non_ascii`, `reject_decimal_float_text`,
//!   `reject_state_alias`, `reject_duplicate_edges`,
//!   `reject_dangling_endpoint`, `reject_wrong_edge_graph`,
//!   `reject_precedence_self_loop`, `reject_precedence_cycle`,
//!   `reject_capability_bare`, `reject_capability_hyphen`,
//!   `reject_capability_empty_component`, `reject_empty_parent_node_id`,
//!   `reject_empty_workstream_id`, `reject_empty_workspace_id`.
//! * Unrepresentable in typed records; a future raw decoder or the trusted
//!   project/graph boundary must reject them, and this typed kernel makes no
//!   claim about them: `reject_wrong_project`, the expected-graph comparison
//!   of `reject_wrong_outer_graph`, `reject_null_title`,
//!   `reject_null_capabilities`, `reject_unknown_field`, `reject_surrogate`,
//!   `reject_both_edge_kinds`, `reject_unknown_edge_kind`, `reject_null_note`,
//!   `reject_missing_edge_kind`.

use crate::content_digest::{
    GraphContentDigestError, MAX_ENCODED_BYTES, checked_total, encode_v1, graph_content_digest_v1,
};
use crate::edge::{ControlKind, GraphEdge, PrecedenceKind};
use crate::error::GraphError;
use crate::execution_graph::ExecutionGraph;
use crate::node::{CapabilityName, GraphNode, GraphNodeAttemptNumber, GraphNodeKind};
use crate::node_state::GraphNodeState;
use crate::version::GraphVersionV1;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Vector {
    id: &'static str,
    byte_count: usize,
    sha256: &'static str,
    /// Complete accepted encoding; empty only for `long_decimals` (20,083
    /// bytes), whose structure is asserted separately.
    encoded_hex: &'static str,
}

const VECTORS: &[Vector] = &[
    Vector {
        id: "minimal",
        byte_count: 69,
        sha256: "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e45440000000000000000000000303a",
        ),
    },
    Vector {
        id: "locked_null",
        byte_count: 69,
        sha256: "2844778885981452c8db6a135f5cf005baed618100ccd3d9689fe19c7a14725c",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e45440000000000000000000100303a",
        ),
    },
    Vector {
        id: "locked_empty",
        byte_count: 71,
        sha256: "52bec8623576f5cdce3424500cebf2bb0138ea05acfaa2d4daf3eeff224d4aa7",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000000000000000002303a00303a",
        ),
    },
    Vector {
        id: "title_empty",
        byte_count: 71,
        sha256: "96eefab41ac68bb7362c342d7e82b3884d3a4e7c565be04a88167fe30e5935bd",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454401303a00000000000000000000303a",
        ),
    },
    Vector {
        id: "capabilities_empty",
        byte_count: 71,
        sha256: "1790cf2374c214e06fb8964d72016164b7737cb358784c502091ec4e528739f5",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000001303a00000000000000303a",
        ),
    },
    Vector {
        id: "capabilities_duplicates",
        byte_count: 121,
        sha256: "56a77bf8a7987bdba857255be96ce1c8b7f06510c5d513f01aa83a629b035478",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000001333a31303a67726170682e636f726532313a7265",
            "766965772e696e646570656e64656e745f613431303a67726170682e636f72650000000000000030",
            "3a",
        ),
    },
    Vector {
        id: "capabilities_reordered",
        byte_count: 121,
        sha256: "371e5dfb2229d7ee8059398970799ddf7b3eaf4cf43d7a032df15e7e1f3f9e44",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000001333a31303a67726170682e636f726531303a6772",
            "6170682e636f726532313a7265766965772e696e646570656e64656e745f61340000000000000030",
            "3a",
        ),
    },
    Vector {
        id: "capabilities_deduplicated",
        byte_count: 108,
        sha256: "63a59fde58c30d2447e57db41dac76e0043f6a0d74d8c96b53875a0c920fd27b",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000001323a31303a67726170682e636f726532313a7265",
            "766965772e696e646570656e64656e745f613400000000000000303a",
        ),
    },
    Vector {
        id: "unicode_composed",
        byte_count: 73,
        sha256: "77c6161d253bb7d8e4f117c53ec23f46d028f36f98cce9ce033f84e583a19b11",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454401323ac3a900000000000000000000303a",
        ),
    },
    Vector {
        id: "unicode_decomposed",
        byte_count: 74,
        sha256: "ef1fe141eb0737df5d7aa5d8ee24764c3b929349c75c020df13efbaeba2a0d0b",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454401333a65cc8100000000000000000000303a",
        ),
    },
    Vector {
        id: "unicode_full",
        byte_count: 410,
        sha256: "068696ce17b2b0843fe3ffdc623874038c2419237430836db19357beca6ab3df",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100333a313a61313a",
            "67343a474f414c383a41434345505445440000000000000000000000333aee8080313a67343a5441",
            "534b373a504c414e4e45440000000000000000000000343af09f9880313a6731303a637573746f6d",
            "3ae7a78d353a524541445901383ac3a9003af09f988001313a610132303a31383434363734343037",
            "3337303935353136313601333a31303a67726170682e636f726532313a7265766965772e696e6465",
            "70656e64656e745f613431303a67726170682e636f726501303a01333ae5b7a50134303a30313233",
            "34353637383961626364656630313233343536373839616263646566303132333435363701333ae5",
            "8cba0131313a2072c3a973756c7461742002303a0132303a31383434363734343037333730393535",
            "31363137323a333aee8080313a67313a61343af09f98800031383a52455155495245535f494e5445",
            "524641434501303a343af09f9880313a67343af09f9880313a6101393a4f4e5f52454a4543540139",
            "3a0072c3a973756dc3a9",
        ),
    },
    Vector {
        id: "unicode_full_reordered",
        byte_count: 410,
        sha256: "068696ce17b2b0843fe3ffdc623874038c2419237430836db19357beca6ab3df",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100333a313a61313a",
            "67343a474f414c383a41434345505445440000000000000000000000333aee8080313a67343a5441",
            "534b373a504c414e4e45440000000000000000000000343af09f9880313a6731303a637573746f6d",
            "3ae7a78d353a524541445901383ac3a9003af09f988001313a610132303a31383434363734343037",
            "3337303935353136313601333a31303a67726170682e636f726532313a7265766965772e696e6465",
            "70656e64656e745f613431303a67726170682e636f726501303a01333ae5b7a50134303a30313233",
            "34353637383961626364656630313233343536373839616263646566303132333435363701333ae5",
            "8cba0131313a2072c3a973756c7461742002303a0132303a31383434363734343037333730393535",
            "31363137323a333aee8080313a67313a61343af09f98800031383a52455155495245535f494e5445",
            "524641434501303a343af09f9880313a67343af09f9880313a6101393a4f4e5f52454a4543540139",
            "3a0072c3a973756dc3a9",
        ),
    },
    Vector {
        id: "long_decimals",
        byte_count: 20083,
        sha256: "f696b293400e0061af6bbc325647bebf5657865c08ff0062a46ba4196ae7693c",
        encoded_hex: "",
    },
    Vector {
        id: "other_project_same_content",
        byte_count: 69,
        sha256: "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e45440000000000000000000000303a",
        ),
    },
    Vector {
        id: "other_graph_identity",
        byte_count: 69,
        sha256: "78e1c7337c1a4f7989cbb93339663179fd9b8583ba95e40997c7507f53a6384d",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "68343a5441534b373a504c414e4e45440000000000000000000000303a",
        ),
    },
    Vector {
        id: "excluded_metadata_changed",
        byte_count: 69,
        sha256: "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e45440000000000000000000000303a",
        ),
    },
    Vector {
        id: "control_self_loop_same_namespace_id",
        byte_count: 92,
        sha256: "825438f3af72290860d1a056400681e42f4b13989af7ffd3092194d5ea666563",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e45440000000000000000000000313a313a6e313a67313a6e313a",
            "6e01373a4f4e5f5041535300",
        ),
    },
    Vector {
        id: "id_200_multibyte",
        byte_count: 470,
        sha256: "acb00cd08bea29c52e8261b3ddcaa50445d6faccc0c34962e238793062910486",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a3430303ac3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3",
            "a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a9c3a931",
            "3a67343a5441534b373a504c414e4e45440000000000000000000000303a",
        ),
    },
    Vector {
        id: "all_frozen_vocabularies",
        byte_count: 895,
        sha256: "e6bd7c9c086e0635e5f9c89bcc80c5ddf31054f89ad834818f3747f77a18ea30",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e76310031353a333a7330",
            "30313a67343a5441534b373a504c414e4e45440000000000000000000000333a733031313a67343a",
            "5441534b353a52454144590000000000000000000000333a733032313a67343a5441534b383a4144",
            "4d49545445440000000000000000000000333a733033313a67343a5441534b31303a444953504154",
            "434845440000000000000000000000333a733034313a67343a5441534b373a52554e4e494e470000",
            "000000000000000000333a733035313a67343a5441534b31353a4157414954494e475f5245564945",
            "570000000000000000000000333a733036313a67343a5441534b363a504153534544000000000000",
            "0000000000333a733037313a67343a5441534b383a52454a45435445440000000000000000000000",
            "333a733038313a67343a5441534b393a524550414952494e470000000000000000000000333a7330",
            "39313a67343a5441534b383a41434345505445440000000000000000000000333a733130313a6734",
            "3a5441534b31303a494e54454752415445440000000000000000000000333a733131313a67343a54",
            "41534b373a424c4f434b45440000000000000000000000333a733132313a67343a5441534b31393a",
            "4c4f434b45445f52455155495245535f50524f0000000000000000000000333a733133313a67343a",
            "5441534b393a43414e43454c4c45440000000000000000000000333a733134313a67343a5441534b",
            "31343a48554d414e5f52455155495245440000000000000000000000393a323a6330313a67333a73",
            "3030333a73303001373a4f4e5f5041535300323a6331313a67333a733030333a73303001393a4f4e",
            "5f52454a45435400323a6332313a67333a733030333a7330300131303a4f4e5f4641494c55524500",
            "323a6333313a67333a733030333a7330300131303a4f4e5f424c4f434b454400323a6334313a6733",
            "3a733030333a73303001383a455343414c41544500323a6335313a67333a733030333a7330300131",
            "323a455850414e44535f494e544f00323a7030313a67333a733030333a7330310031373a52455155",
            "495245535f414343455054454400323a7031313a67333a733031333a7330320031393a5245515549",
            "5245535f494e544547524154454400323a7032313a67333a733032333a7330330031383a52455155",
            "495245535f494e5445524641434500",
        ),
    },
    Vector {
        id: "capabilities_underscore_digits",
        byte_count: 97,
        sha256: "39a8ed54add3f65aa630d034169060cea6fafa96f091684bf4346f7029db2f44",
        encoded_hex: concat!(
            "72656365697074732e67726170682d636f6e74656e742e7368613235362e763100313a313a6e313a",
            "67343a5441534b373a504c414e4e454400000001333a363a615f2e625f32383a78312e79322e7a5f",
            "363a615f2e625f3200000000000000303a",
        ),
    },
];

fn node_in(graph_id: &str, node_id: &str, kind: GraphNodeKind, state: GraphNodeState) -> GraphNode {
    GraphNode::new(node_id, graph_id, kind, state).expect("valid fixture node")
}

fn node(node_id: &str) -> GraphNode {
    node_in("g", node_id, GraphNodeKind::TASK, GraphNodeState::Planned)
}

fn graph(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> ExecutionGraph {
    ExecutionGraph::from_parts("g", nodes, edges).expect("valid fixture graph")
}

fn one(node: GraphNode) -> ExecutionGraph {
    graph(vec![node], Vec::new())
}

fn caps(names: &[&str]) -> Vec<CapabilityName> {
    names
        .iter()
        .map(|name| CapabilityName::new(*name).expect("valid fixture capability"))
        .collect()
}

fn attempt(value: impl Into<String>) -> GraphNodeAttemptNumber {
    GraphNodeAttemptNumber::try_new(value).expect("valid fixture attempt number")
}

fn version(value: impl Into<String>) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).expect("valid fixture version")
}

/// The `unicode_full` records; `reversed` supplies them in another order.
fn unicode_full(reversed: bool) -> ExecutionGraph {
    let full = GraphNode::new(
        "😀",
        "g",
        GraphNodeKind::new("custom:种").expect("valid kind"),
        GraphNodeState::Ready,
    )
    .expect("valid node")
    .with_title("é\u{0}:😀")
    .with_parent_node_id("a")
    .expect("valid parent")
    .with_attempt_number(attempt("18446744073709551616"))
    .with_required_capabilities(caps(&["graph.core", "review.independent_a4", "graph.core"]))
    .with_task_capsule_ref("")
    .with_workstream_id("工")
    .expect("valid workstream")
    .with_code_sha("0123456789abcdef0123456789abcdef01234567")
    .expect("valid sha")
    .with_workspace_id("区")
    .expect("valid workspace")
    .with_result_ref(" résultat ")
    .with_locked_reason("")
    .with_created_in_version(version("18446744073709551617"));
    let mut nodes = vec![
        full,
        node("\u{E000}"),
        node_in("g", "a", GraphNodeKind::GOAL, GraphNodeState::Accepted),
    ];
    let mut edges = vec![
        GraphEdge::control("😀", "g", "😀", "a", ControlKind::OnReject)
            .expect("valid edge")
            .with_note("\u{0}résumé"),
        GraphEdge::precedence(
            "\u{E000}",
            "g",
            "a",
            "😀",
            PrecedenceKind::RequiresInterface,
        )
        .expect("valid edge")
        .with_note(""),
    ];
    if reversed {
        nodes.reverse();
        edges.reverse();
    }
    graph(nodes, edges)
}

fn all_frozen_vocabularies() -> ExecutionGraph {
    let nodes = GraphNodeState::ALL
        .iter()
        .enumerate()
        .map(|(index, state)| node_in("g", &format!("s{index:02}"), GraphNodeKind::TASK, *state))
        .collect();
    let precedence = [
        PrecedenceKind::RequiresAccepted,
        PrecedenceKind::RequiresIntegrated,
        PrecedenceKind::RequiresInterface,
    ];
    let control = [
        ControlKind::OnPass,
        ControlKind::OnReject,
        ControlKind::OnFailure,
        ControlKind::OnBlocked,
        ControlKind::Escalate,
        ControlKind::ExpandsInto,
    ];
    let mut edges: Vec<GraphEdge> = precedence
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            GraphEdge::precedence(
                format!("p{i}"),
                "g",
                format!("s{i:02}"),
                format!("s{:02}", i + 1),
                *kind,
            )
            .expect("valid edge")
        })
        .collect();
    edges.extend(control.iter().enumerate().map(|(i, kind)| {
        GraphEdge::control(format!("c{i}"), "g", "s00", "s00", *kind).expect("valid edge")
    }));
    graph(nodes, edges)
}

/// Builds the canonical graph for an accepted positive fixture.
fn fixture(id: &str) -> ExecutionGraph {
    match id {
        "minimal" | "other_project_same_content" | "excluded_metadata_changed" => one(node("n")),
        "locked_null" => one(node("n").with_null_locked_reason()),
        "locked_empty" => one(node("n").with_locked_reason("")),
        "title_empty" => one(node("n").with_title("")),
        "capabilities_empty" => one(node("n").with_required_capabilities(Vec::new())),
        "capabilities_duplicates" => one(node("n").with_required_capabilities(caps(&[
            "graph.core",
            "review.independent_a4",
            "graph.core",
        ]))),
        "capabilities_reordered" => one(node("n").with_required_capabilities(caps(&[
            "graph.core",
            "graph.core",
            "review.independent_a4",
        ]))),
        "capabilities_deduplicated" => {
            one(node("n")
                .with_required_capabilities(caps(&["graph.core", "review.independent_a4"])))
        }
        "capabilities_underscore_digits" => {
            one(node("n").with_required_capabilities(caps(&["a_.b_2", "x1.y2.z_", "a_.b_2"])))
        }
        "unicode_composed" => one(node("n").with_title("\u{E9}")),
        "unicode_decomposed" => one(node("n").with_title("e\u{301}")),
        "unicode_full" => unicode_full(false),
        "unicode_full_reordered" => unicode_full(true),
        "long_decimals" => one(node("n")
            .with_attempt_number(attempt(format!("1{}", "0".repeat(10_000))))
            .with_created_in_version(version("9".repeat(10_001)))),
        "other_graph_identity" => ExecutionGraph::from_parts(
            "h",
            vec![node_in(
                "h",
                "n",
                GraphNodeKind::TASK,
                GraphNodeState::Planned,
            )],
            Vec::new(),
        )
        .expect("valid graph"),
        "control_self_loop_same_namespace_id" => graph(
            vec![node("n")],
            vec![GraphEdge::control("n", "g", "n", "n", ControlKind::OnPass).expect("valid edge")],
        ),
        "id_200_multibyte" => one(node(&"é".repeat(200))),
        "all_frozen_vocabularies" => all_frozen_vocabularies(),
        other => panic!("no fixture graph for {other}"),
    }
}

/// The exact bytes the production encoder streams to SHA-256.
fn encoded(graph: &ExecutionGraph) -> Vec<u8> {
    let mut bytes = Vec::new();
    let written = encode_v1(graph, 0, |chunk| bytes.extend_from_slice(chunk)).expect("encodes");
    assert_eq!(written, bytes.len() as u64);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest(graph: &ExecutionGraph) -> String {
    graph_content_digest_v1(graph).expect("digest")
}

fn vector(id: &str) -> &'static Vector {
    VECTORS
        .iter()
        .find(|vector| vector.id == id)
        .expect("known vector")
}

/// `receipts.graph-content.sha256.v1` + NUL + `1:1:n1:g4:TASK7:PLANNED` +
/// eleven absent-tag bytes + `0:`, built by hand from the grammar.
fn minimal_literal() -> Vec<u8> {
    let mut bytes = b"receipts.graph-content.sha256.v1\0".to_vec();
    bytes.extend_from_slice(b"1:1:n1:g4:TASK7:PLANNED");
    bytes.extend_from_slice(&[0; 11]);
    bytes.extend_from_slice(b"0:");
    bytes
}

// ---------------------------------------------------------------------------
// Accepted vectors
// ---------------------------------------------------------------------------

#[test]
fn all_twenty_accepted_positive_vectors_are_reproduced() {
    assert_eq!(VECTORS.len(), 20);
    for vector in VECTORS {
        let graph = fixture(vector.id);
        let bytes = encoded(&graph);
        assert_eq!(bytes.len(), vector.byte_count, "{}", vector.id);
        if !vector.encoded_hex.is_empty() {
            assert_eq!(hex(&bytes), vector.encoded_hex, "{}", vector.id);
        }
        assert_eq!(digest(&graph), vector.sha256, "{}", vector.id);
    }
}

#[test]
fn minimal_graph_matches_the_hand_built_69_byte_literal() {
    let literal = minimal_literal();
    assert_eq!(literal.len(), 69);
    assert_eq!(&literal[..33], b"receipts.graph-content.sha256.v1\0");
    assert_eq!(encoded(&fixture("minimal")), literal);
    assert_eq!(
        digest(&fixture("minimal")),
        "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f"
    );
}

#[test]
fn long_decimal_carriers_are_hashed_as_exact_text() {
    let bytes = encoded(&fixture("long_decimals"));
    let mut expected = minimal_literal();
    let tail = expected.split_off(56);
    assert_eq!(tail, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'0', b':']);
    // node prefix, then OI(attempt), OC..result_ref absent, L absent, OI(version)
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(b"\x0110001:1");
    expected.extend_from_slice("0".repeat(10_000).as_bytes());
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
    expected.extend_from_slice(b"\x0110001:");
    expected.extend_from_slice("9".repeat(10_001).as_bytes());
    expected.extend_from_slice(b"0:");
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), vector("long_decimals").byte_count);
}

#[test]
fn digest_is_sixty_four_lowercase_hex_characters() {
    for vector in VECTORS {
        let value = digest(&fixture(vector.id));
        assert_eq!(value.len(), 64);
        assert!(
            value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        );
    }
}

// ---------------------------------------------------------------------------
// Presence, order, Unicode and identity distinctions
// ---------------------------------------------------------------------------

#[test]
fn locked_reason_absent_null_empty_and_nonempty_are_distinct() {
    let variants = [
        node("n"),
        node("n").with_null_locked_reason(),
        node("n").with_locked_reason(""),
        node("n").with_locked_reason("x"),
    ];
    let digests: Vec<String> = variants.into_iter().map(|n| digest(&one(n))).collect();
    for (i, a) in digests.iter().enumerate() {
        for b in &digests[i + 1..] {
            assert_ne!(a, b);
        }
    }
    let nonempty = encoded(&one(node("n").with_locked_reason("x")));
    assert!(nonempty.ends_with(b"\x021:x\x000:"));
}

#[test]
fn optional_absence_and_present_empty_differ() {
    let minimal = vector("minimal").sha256;
    assert_ne!(vector("title_empty").sha256, minimal);
    assert_ne!(vector("capabilities_empty").sha256, minimal);
    let refs = [
        node("n").with_task_capsule_ref(""),
        node("n").with_result_ref(""),
        GraphNode::new("n", "g", GraphNodeKind::TASK, GraphNodeState::Planned)
            .expect("valid node")
            .with_parent_node_id("n")
            .expect("valid parent"),
    ];
    for node in refs {
        assert_ne!(digest(&one(node)), minimal);
    }
    let noted = graph(
        vec![node("n")],
        vec![
            GraphEdge::control("e", "g", "n", "n", ControlKind::OnPass)
                .expect("valid edge")
                .with_note(""),
        ],
    );
    let unnoted = graph(
        vec![node("n")],
        vec![GraphEdge::control("e", "g", "n", "n", ControlKind::OnPass).expect("valid edge")],
    );
    assert_ne!(digest(&noted), digest(&unnoted));
}

#[test]
fn capability_order_and_multiplicity_are_preserved() {
    let ids = [
        "capabilities_duplicates",
        "capabilities_reordered",
        "capabilities_deduplicated",
        "capabilities_empty",
        "minimal",
    ];
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            assert_ne!(digest(&fixture(a)), digest(&fixture(b)), "{a} vs {b}");
        }
    }
}

#[test]
fn composed_and_decomposed_unicode_remain_distinct() {
    assert_ne!(
        digest(&fixture("unicode_composed")),
        digest(&fixture("unicode_decomposed"))
    );
}

#[test]
fn reordered_or_incrementally_built_records_encode_identically() {
    let expected = encoded(&fixture("unicode_full"));
    assert_eq!(encoded(&fixture("unicode_full_reordered")), expected);

    let source = fixture("unicode_full");
    let mut incremental = ExecutionGraph::new("g").expect("valid graph");
    for node in source.nodes().collect::<Vec<_>>().into_iter().rev() {
        incremental.add_node(node.clone()).expect("valid node");
    }
    for edge in source.edges() {
        incremental.add_edge(edge.clone()).expect("valid edge");
    }
    assert_eq!(encoded(&incremental), expected);
}

#[test]
fn consistent_graph_identity_change_alters_the_digest() {
    assert_ne!(
        digest(&fixture("other_graph_identity")),
        digest(&fixture("minimal"))
    );
}

#[test]
fn context_only_fixtures_share_the_minimal_digest() {
    for id in ["other_project_same_content", "excluded_metadata_changed"] {
        assert_eq!(vector(id).sha256, vector("minimal").sha256);
        assert_eq!(digest(&fixture(id)), vector("minimal").sha256);
    }
}

#[test]
fn control_self_loops_and_loops_are_valid_and_hashed() {
    let self_loop = fixture("control_self_loop_same_namespace_id");
    assert!(self_loop.contains_node("n") && self_loop.contains_edge("n"));
    assert_ne!(digest(&self_loop), digest(&fixture("minimal")));

    let pair = graph(
        vec![node("a"), node("b")],
        vec![
            GraphEdge::control("ab", "g", "a", "b", ControlKind::OnReject).expect("valid edge"),
            GraphEdge::control("ba", "g", "b", "a", ControlKind::Escalate).expect("valid edge"),
        ],
    );
    assert_eq!(digest(&pair).len(), 64);
}

#[test]
fn repeated_calls_are_deterministic_and_leave_the_graph_unchanged() {
    let graph = fixture("unicode_full");
    let before = graph.clone();
    let first = digest(&graph);
    assert_eq!(digest(&graph), first);
    assert_eq!(graph, before);
}

// ---------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------

#[test]
fn empty_graphs_remain_constructible_but_have_no_digest() {
    let empties = [
        ExecutionGraph::new("g").expect("empty builder"),
        ExecutionGraph::from_parts("g", Vec::new(), Vec::new()).expect("empty builder"),
    ];
    for graph in empties {
        assert_eq!(
            graph_content_digest_v1(&graph),
            Err(GraphContentDigestError::EmptyGraph)
        );
        let mut emitted = Vec::new();
        assert_eq!(
            encode_v1(&graph, 0, |chunk| emitted.extend_from_slice(chunk)),
            Err(GraphContentDigestError::EmptyGraph)
        );
        assert!(emitted.is_empty());
    }
}

#[test]
fn byte_accounting_accepts_the_maximum_and_rejects_crossing_it() {
    let overflow = Err(GraphContentDigestError::EncodedLengthOverflow);
    assert_eq!(MAX_ENCODED_BYTES, u64::MAX / 8);
    assert_eq!(
        checked_total(MAX_ENCODED_BYTES - 3, 3),
        Ok(MAX_ENCODED_BYTES)
    );
    assert_eq!(checked_total(MAX_ENCODED_BYTES, 0), Ok(MAX_ENCODED_BYTES));
    assert_eq!(checked_total(MAX_ENCODED_BYTES - 3, 4), overflow);
    assert_eq!(checked_total(MAX_ENCODED_BYTES, 1), overflow);
    // Arithmetic overflow and a chunk larger than the limit.
    assert_eq!(checked_total(u64::MAX, 1), overflow);
    assert_eq!(checked_total(0, usize::MAX), overflow);
}

#[test]
fn encoder_counts_every_emitted_byte_up_to_the_limit() {
    let graph = fixture("minimal");
    let full = minimal_literal();

    // Ending exactly at the maximum succeeds.
    let mut emitted = Vec::new();
    let start = MAX_ENCODED_BYTES - 69;
    assert_eq!(
        encode_v1(&graph, start, |chunk| emitted.extend_from_slice(chunk)),
        Ok(MAX_ENCODED_BYTES)
    );
    assert_eq!(emitted, full);

    // One more byte: the final two-byte `0:` count would cross the limit and
    // is never emitted.
    let mut emitted = Vec::new();
    assert_eq!(
        encode_v1(&graph, start + 1, |chunk| emitted.extend_from_slice(chunk)),
        Err(GraphContentDigestError::EncodedLengthOverflow)
    );
    assert_eq!(emitted, full[..67]);

    // The domain prefix itself is counted.
    let mut emitted = Vec::new();
    assert_eq!(
        encode_v1(&graph, MAX_ENCODED_BYTES - 32, |chunk| {
            emitted.extend_from_slice(chunk)
        }),
        Err(GraphContentDigestError::EncodedLengthOverflow)
    );
    assert!(emitted.is_empty());
}

#[test]
fn error_is_a_standard_error_with_explicit_messages() {
    fn assert_error<E: std::error::Error + Copy + Eq>() {}
    assert_error::<GraphContentDigestError>();
    for error in [
        GraphContentDigestError::EmptyGraph,
        GraphContentDigestError::EncodedLengthOverflow,
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn negative_fixtures_fail_before_reaching_the_kernel() {
    let n = || node("n");
    let task = |id: &str, graph_id: &str| {
        GraphNode::new(id, graph_id, GraphNodeKind::TASK, GraphNodeState::Planned)
    };
    let ctrl = |edge_id: &str, graph_id: &str, to: &str| {
        GraphEdge::control(edge_id, graph_id, "n", to, ControlKind::OnPass).expect("valid edge")
    };
    let prec = |edge_id: &str, from: &str, to: &str| {
        GraphEdge::precedence(edge_id, "g", from, to, PrecedenceKind::RequiresAccepted)
            .expect("valid edge")
    };
    let parts = |graph_id: &str, nodes: Vec<GraphNode>, edges: Vec<GraphEdge>| {
        ExecutionGraph::from_parts(graph_id, nodes, edges)
    };

    // Graph construction.
    assert!(matches!(
        parts("g", vec![n(), n()], vec![]),
        Err(GraphError::DuplicateNodeId { .. })
    ));
    let child_h = task("n", "h").expect("valid node");
    assert!(matches!(
        parts("g", vec![child_h], vec![]),
        Err(GraphError::ChildGraphIdMismatch { .. })
    ));
    assert!(matches!(
        parts("h", vec![n()], vec![]),
        Err(GraphError::ChildGraphIdMismatch { .. })
    ));
    assert!(matches!(
        parts(
            "g",
            vec![n()],
            vec![ctrl("e", "g", "n"), ctrl("e", "g", "n")]
        ),
        Err(GraphError::DuplicateEdgeId { .. })
    ));
    assert!(matches!(
        parts("g", vec![n()], vec![ctrl("e", "g", "missing")]),
        Err(GraphError::UnknownNodeReference { .. })
    ));
    assert!(matches!(
        parts("g", vec![n()], vec![ctrl("e", "h", "n")]),
        Err(GraphError::ChildGraphIdMismatch { .. })
    ));
    assert!(matches!(
        parts("g", vec![n()], vec![prec("e", "n", "n")]),
        Err(GraphError::PrecedenceCycleRejected { .. })
    ));
    assert!(matches!(
        parts(
            "g",
            vec![n(), node("m")],
            vec![prec("e1", "n", "m"), prec("e2", "m", "n")]
        ),
        Err(GraphError::PrecedenceCycleRejected { .. })
    ));

    // Child records and carriers.
    assert!(matches!(
        task("", "g"),
        Err(GraphError::EmptyIdentifier { .. })
    ));
    assert!(matches!(
        task(&"é".repeat(201), "g"),
        Err(GraphError::IdentifierTooLong { .. })
    ));
    for invalid in ["Graph.core", "graph", "graph.co-re", "graph..core"] {
        assert!(matches!(
            CapabilityName::new(invalid),
            Err(GraphError::InvalidCapabilitySyntax { .. })
        ));
    }
    assert!(matches!(
        n().with_code_sha("A".repeat(40)),
        Err(GraphError::InvalidCodeSha { .. })
    ));
    for invalid in ["0", "\u{661}", "1.0"] {
        assert!(matches!(
            GraphNodeAttemptNumber::try_new(invalid),
            Err(GraphError::InvalidAttemptNumber { .. })
        ));
    }
    assert!(GraphVersionV1::try_new("01").is_err());
    assert!(GraphNodeState::parse("IN_PROGRESS").is_err());
    assert!(matches!(
        n().with_parent_node_id(""),
        Err(GraphError::EmptyIdentifier { .. })
    ));
    assert!(matches!(
        n().with_workstream_id(""),
        Err(GraphError::EmptyIdentifier { .. })
    ));
    assert!(matches!(
        n().with_workspace_id(""),
        Err(GraphError::EmptyIdentifier { .. })
    ));
}
