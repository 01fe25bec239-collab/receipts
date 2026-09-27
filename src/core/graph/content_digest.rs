//! Pure graph child-content SHA-256 digest, profile v1.
//!
//! Hashes the canonical node and edge set of an already validated
//! [`ExecutionGraph`] (GRAPH_MUTATION_PROTOCOL's content digest) using the
//! fixed v1 byte grammar accepted by A1:
//!
//! ```text
//! D  = "receipts.graph-content.sha256.v1" 00
//! U(n) = shortest ASCII decimal n, then ':'      S(s) = U(UTF-8 bytes) || bytes
//! OS = OI = 00 | 01 S    OC = 00 | 01 U(count) S..    L = 00 | 01 | 02 S
//! B  = D U(node_count) nodes-by-id U(edge_count) edges-by-id
//! ```
//!
//! Node: `S(node_id) S(graph_id) S(kind) S(state) OS(title) OS(parent_node_id)
//! OI(attempt_number) OC(required_capabilities) OS(task_capsule_ref)
//! OS(workstream_id) OS(code_sha) OS(workspace_id) OS(result_ref)
//! L(locked_reason) OI(created_in_version)`. Edge: `S(edge_id) S(graph_id)
//! S(from_node) S(to_node) (00 S(precedence_kind) | 01 S(control_kind))
//! OS(note)`. Decimal carriers are hashed as their canonical text.
//!
//! Only child records are hashed. Project identity, outer version/metadata and
//! referenced payloads are excluded; the digest grants no authority and does
//! not bind a project, graph version or persisted snapshot. v1 is fixed: there
//! is no profile negotiation or fallback.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::edge::{GraphEdge, GraphEdgeRelation};
use crate::execution_graph::ExecutionGraph;
use crate::node::{GraphNode, GraphNodeAttemptNumber};
use crate::version::GraphVersionV1;

const DOMAIN_V1: &[u8] = b"receipts.graph-content.sha256.v1\0";

/// Largest accepted encoded message in bytes, keeping its SHA-256 bit length
/// below 2^64. A message-length limit, not a decimal value or digit limit.
pub(crate) const MAX_ENCODED_BYTES: u64 = u64::MAX / 8;

/// Why no graph-content digest was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphContentDigestError {
    /// The graph has no nodes; persisted graph content must be nonempty.
    EmptyGraph,
    /// The encoded message would exceed [`u64::MAX`] / 8 bytes.
    EncodedLengthOverflow,
}

impl std::fmt::Display for GraphContentDigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphContentDigestError::EmptyGraph => {
                f.write_str("graph content digest requires at least one node")
            }
            GraphContentDigestError::EncodedLengthOverflow => {
                f.write_str("encoded graph content exceeds the SHA-256 message length limit")
            }
        }
    }
}

impl std::error::Error for GraphContentDigestError {}

/// Computes the v1 child-content digest as exactly 64 lowercase hex
/// characters. Fails explicitly, returning no digest, for an empty graph or
/// an over-long encoding.
pub fn graph_content_digest_v1(graph: &ExecutionGraph) -> Result<String, GraphContentDigestError> {
    let mut hasher = Sha256::new();
    encode_v1(graph, 0, |bytes| hasher.update(bytes))?;
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(hex)
}

/// Streams the v1 encoding of `graph` into `sink`, starting from `written`
/// already-counted bytes, and returns the new total. Every chunk is counted
/// before it reaches the sink, so an over-limit chunk is never emitted.
pub(crate) fn encode_v1(
    graph: &ExecutionGraph,
    written: u64,
    sink: impl FnMut(&[u8]),
) -> Result<u64, GraphContentDigestError> {
    if graph.node_count() == 0 {
        return Err(GraphContentDigestError::EmptyGraph);
    }
    let mut encoder = Encoder { written, sink };
    encoder.bytes(DOMAIN_V1)?;
    encoder.count(graph.node_count())?;
    for node in graph.nodes() {
        encoder.node(node)?;
    }
    encoder.count(graph.edge_count())?;
    for edge in graph.edges() {
        encoder.edge(edge)?;
    }
    Ok(encoder.written)
}

/// Adds `chunk` bytes to `written`, rejecting conversion or arithmetic
/// overflow and any total beyond [`MAX_ENCODED_BYTES`].
pub(crate) fn checked_total(written: u64, chunk: usize) -> Result<u64, GraphContentDigestError> {
    u64::try_from(chunk)
        .ok()
        .and_then(|chunk| written.checked_add(chunk))
        .filter(|total| *total <= MAX_ENCODED_BYTES)
        .ok_or(GraphContentDigestError::EncodedLengthOverflow)
}

struct Encoder<F> {
    written: u64,
    sink: F,
}

impl<F: FnMut(&[u8])> Encoder<F> {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), GraphContentDigestError> {
        self.written = checked_total(self.written, bytes.len())?;
        (self.sink)(bytes);
        Ok(())
    }

    /// `U(n)`.
    fn count(&mut self, n: usize) -> Result<(), GraphContentDigestError> {
        self.bytes(format!("{n}:").as_bytes())
    }

    /// `S(s)`.
    fn text(&mut self, s: &str) -> Result<(), GraphContentDigestError> {
        self.count(s.len())?;
        self.bytes(s.as_bytes())
    }

    /// `OS(s)`; also `OI` over canonical decimal text.
    fn optional_text(&mut self, s: Option<&str>) -> Result<(), GraphContentDigestError> {
        match s {
            None => self.bytes(&[0]),
            Some(s) => {
                self.bytes(&[1])?;
                self.text(s)
            }
        }
    }

    fn node(&mut self, node: &GraphNode) -> Result<(), GraphContentDigestError> {
        self.text(node.node_id())?;
        self.text(node.graph_id())?;
        self.text(node.kind().as_str())?;
        self.text(node.state().as_str())?;
        self.optional_text(node.title())?;
        self.optional_text(node.parent_node_id())?;
        self.optional_text(node.attempt_number().map(GraphNodeAttemptNumber::as_str))?;
        match node.required_capabilities() {
            None => self.bytes(&[0])?,
            Some(capabilities) => {
                self.bytes(&[1])?;
                self.count(capabilities.len())?;
                for capability in capabilities {
                    self.text(capability.as_str())?;
                }
            }
        }
        self.optional_text(node.task_capsule_ref())?;
        self.optional_text(node.workstream_id())?;
        self.optional_text(node.code_sha())?;
        self.optional_text(node.workspace_id())?;
        self.optional_text(node.result_ref())?;
        match node.locked_reason() {
            None => self.bytes(&[0])?,
            Some(None) => self.bytes(&[1])?,
            Some(Some(reason)) => {
                self.bytes(&[2])?;
                self.text(reason)?;
            }
        }
        self.optional_text(node.created_in_version().map(GraphVersionV1::as_str))
    }

    fn edge(&mut self, edge: &GraphEdge) -> Result<(), GraphContentDigestError> {
        self.text(edge.edge_id())?;
        self.text(edge.graph_id())?;
        self.text(edge.from_node())?;
        self.text(edge.to_node())?;
        match edge.relation() {
            GraphEdgeRelation::Precedence(kind) => {
                self.bytes(&[0])?;
                self.text(kind.as_str())?;
            }
            GraphEdgeRelation::Control(kind) => {
                self.bytes(&[1])?;
                self.text(kind.as_str())?;
            }
        }
        self.optional_text(edge.note())
    }
}
