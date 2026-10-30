//! `GraphNodeResult` — evidence produced by one node.
//!
//! A validated physical record. Constructing or holding one grants no verdict,
//! state change or acceptance; the apply-v1 `ATTACH_RESULT` operation checks
//! its graph/node/attempt/SHA association before attaching it once.

use receipts_workspace_execution::CommitSha;

use crate::error::GraphError;
use crate::node_result_vocabulary::{GraphNodeCheckResult, GraphNodeResultOutcome};
use crate::orchestration::OrchestrationDateTimeV1;

/// Why a result or check value was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphNodeResultError {
    Identifier(GraphError),
    InvalidCodeSha,
    EmptyCommand,
    InvalidExitCode,
}

impl std::fmt::Display for GraphNodeResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identifier(error) => error.fmt(f),
            Self::InvalidCodeSha => f.write_str("code_sha must be 40 lowercase hex digits"),
            Self::EmptyCommand => f.write_str("check command must contain at least one argument"),
            Self::InvalidExitCode => f.write_str("exit_code must be a canonical decimal integer"),
        }
    }
}

impl std::error::Error for GraphNodeResultError {}

impl From<GraphError> for GraphNodeResultError {
    fn from(error: GraphError) -> Self {
        Self::Identifier(error)
    }
}

/// One deterministic check: exact argv, canonical integer exit code
/// (`0` or `-?[1-9][0-9]*`, no magnitude limit), optional result/output ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodeCheck {
    command: Vec<String>,
    exit_code: String,
    result: Option<GraphNodeCheckResult>,
    output_ref: Option<String>,
}

impl GraphNodeCheck {
    pub fn try_new(
        command: Vec<String>,
        exit_code: impl Into<String>,
        result: Option<GraphNodeCheckResult>,
        output_ref: Option<String>,
    ) -> Result<Self, GraphNodeResultError> {
        let exit_code = exit_code.into();
        if command.is_empty() {
            return Err(GraphNodeResultError::EmptyCommand);
        }
        if !is_canonical_integer(&exit_code) {
            return Err(GraphNodeResultError::InvalidExitCode);
        }
        Ok(Self {
            command,
            exit_code,
            result,
            output_ref,
        })
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn exit_code(&self) -> &str {
        &self.exit_code
    }
    pub fn result(&self) -> Option<GraphNodeCheckResult> {
        self.result
    }
    pub fn output_ref(&self) -> Option<&str> {
        self.output_ref.as_deref()
    }
}

pub(crate) fn is_canonical_integer(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    match digits.as_bytes() {
        [b'0'] => value.len() == 1,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

/// Evidence produced by one node. Optional fields keep their absence; lists
/// keep order and duplicates. Nothing here is a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodeResult {
    node_id: String,
    graph_id: String,
    outcome: GraphNodeResultOutcome,
    produced_at: OrchestrationDateTimeV1,
    code_sha: Option<CommitSha>,
    attempt_id: Option<String>,
    evidence_refs: Option<Vec<String>>,
    checks: Option<Vec<GraphNodeCheck>>,
    review_id: Option<String>,
    routing_decision_id: Option<String>,
}

impl GraphNodeResult {
    /// Required fields; every optional field starts absent.
    pub fn try_new(
        node_id: impl Into<String>,
        graph_id: impl Into<String>,
        outcome: GraphNodeResultOutcome,
        produced_at: OrchestrationDateTimeV1,
    ) -> Result<Self, GraphNodeResultError> {
        let node_id = node_id.into();
        let graph_id = graph_id.into();
        GraphError::validate_identifier("node_id", &node_id)?;
        GraphError::validate_identifier("graph_id", &graph_id)?;
        Ok(Self {
            node_id,
            graph_id,
            outcome,
            produced_at,
            code_sha: None,
            attempt_id: None,
            evidence_refs: None,
            checks: None,
            review_id: None,
            routing_decision_id: None,
        })
    }
    pub fn with_code_sha(mut self, code_sha: &str) -> Result<Self, GraphNodeResultError> {
        self.code_sha =
            Some(CommitSha::parse(code_sha).map_err(|_| GraphNodeResultError::InvalidCodeSha)?);
        Ok(self)
    }
    pub fn with_attempt_id(mut self, id: impl Into<String>) -> Result<Self, GraphNodeResultError> {
        let id = id.into();
        GraphError::validate_identifier("attempt_id", &id)?;
        self.attempt_id = Some(id);
        Ok(self)
    }
    pub fn with_evidence_refs(mut self, refs: Vec<String>) -> Self {
        self.evidence_refs = Some(refs);
        self
    }
    pub fn with_checks(mut self, checks: Vec<GraphNodeCheck>) -> Self {
        self.checks = Some(checks);
        self
    }
    pub fn with_review_id(mut self, id: impl Into<String>) -> Result<Self, GraphNodeResultError> {
        let id = id.into();
        GraphError::validate_identifier("review_id", &id)?;
        self.review_id = Some(id);
        Ok(self)
    }
    pub fn with_routing_decision_id(
        mut self,
        id: impl Into<String>,
    ) -> Result<Self, GraphNodeResultError> {
        let id = id.into();
        GraphError::validate_identifier("routing_decision_id", &id)?;
        self.routing_decision_id = Some(id);
        Ok(self)
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn outcome(&self) -> GraphNodeResultOutcome {
        self.outcome
    }
    pub fn produced_at(&self) -> &OrchestrationDateTimeV1 {
        &self.produced_at
    }
    pub fn code_sha(&self) -> Option<&str> {
        self.code_sha.as_ref().map(CommitSha::as_str)
    }
    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }
    pub fn evidence_refs(&self) -> Option<&[String]> {
        self.evidence_refs.as_deref()
    }
    pub fn checks(&self) -> Option<&[GraphNodeCheck]> {
        self.checks.as_deref()
    }
    pub fn review_id(&self) -> Option<&str> {
        self.review_id.as_deref()
    }
    pub fn routing_decision_id(&self) -> Option<&str> {
        self.routing_decision_id.as_deref()
    }
}
