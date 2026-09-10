//! Immutable capsule-local findings and historical executed-check evidence. No execution.

use super::*;
use crate::orchestration::OrchestrationDateTimeV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairCapsuleFindingV1 {
    finding_id: String,
    severity: RepairFindingSeverityV1,
    category: RepairFindingCategoryV1,
    description: String,
    blocking: bool,
    path: Option<String>,
    line: Option<RepairFindingLineV1>,
    evidence_ref: Option<Ref>,
    confidence: Option<RepairFindingConfidenceV1>,
    source: Option<RepairFindingSourceV1>,
}

impl RepairCapsuleFindingV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        finding_id: String,
        severity: RepairFindingSeverityV1,
        category: RepairFindingCategoryV1,
        description: String,
        blocking: bool,
        path: Option<String>,
        line: Option<RepairFindingLineV1>,
        evidence_ref: Option<Ref>,
        confidence: Option<RepairFindingConfidenceV1>,
        source: Option<RepairFindingSourceV1>,
    ) -> Result<Self, CapsuleError> {
        validate_identifier("finding_id", &finding_id)?;
        if description.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "description",
            });
        }
        Ok(Self {
            finding_id,
            severity,
            category,
            description,
            blocking,
            path,
            line,
            evidence_ref,
            confidence,
            source,
        })
    }
    pub fn finding_id(&self) -> &str {
        &self.finding_id
    }
    pub fn severity(&self) -> RepairFindingSeverityV1 {
        self.severity
    }
    pub fn category(&self) -> RepairFindingCategoryV1 {
        self.category
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn blocking(&self) -> bool {
        self.blocking
    }
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    pub fn line(&self) -> Option<&RepairFindingLineV1> {
        self.line.as_ref()
    }
    pub fn evidence_ref(&self) -> Option<&Ref> {
        self.evidence_ref.as_ref()
    }
    pub fn confidence(&self) -> Option<RepairFindingConfidenceV1> {
        self.confidence
    }
    pub fn source(&self) -> Option<RepairFindingSourceV1> {
        self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairCapsuleFailedCheckV1 {
    source: RepairCheckSourceV1,
    command: Vec<String>,
    exit_code: RepairExitCodeV1,
    code_sha: String,
    timed_out: Option<bool>,
    started_at: Option<OrchestrationDateTimeV1>,
    finished_at: Option<OrchestrationDateTimeV1>,
    output_ref: Option<Ref>,
    result: Option<RepairCheckResultV1>,
}

impl RepairCapsuleFailedCheckV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        source: RepairCheckSourceV1,
        command: Vec<String>,
        exit_code: RepairExitCodeV1,
        code_sha: String,
        timed_out: Option<bool>,
        started_at: Option<OrchestrationDateTimeV1>,
        finished_at: Option<OrchestrationDateTimeV1>,
        output_ref: Option<Ref>,
        result: Option<RepairCheckResultV1>,
    ) -> Result<Self, CapsuleError> {
        if command.is_empty() {
            return Err(CapsuleError::EmptyField { field: "command" });
        }
        validate_repair_sha("code_sha", &code_sha)?;
        Ok(Self {
            source,
            command,
            exit_code,
            code_sha,
            timed_out,
            started_at,
            finished_at,
            output_ref,
            result,
        })
    }
    pub fn source(&self) -> RepairCheckSourceV1 {
        self.source
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn exit_code(&self) -> &RepairExitCodeV1 {
        &self.exit_code
    }
    pub fn code_sha(&self) -> &str {
        &self.code_sha
    }
    pub fn timed_out(&self) -> Option<bool> {
        self.timed_out
    }
    pub fn started_at(&self) -> Option<&OrchestrationDateTimeV1> {
        self.started_at.as_ref()
    }
    pub fn finished_at(&self) -> Option<&OrchestrationDateTimeV1> {
        self.finished_at.as_ref()
    }
    pub fn output_ref(&self) -> Option<&Ref> {
        self.output_ref.as_ref()
    }
    pub fn result(&self) -> Option<RepairCheckResultV1> {
        self.result
    }
}

pub(super) fn validate_repair_sha(field: &'static str, value: &str) -> Result<(), CapsuleError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(CapsuleError::InvalidSha { field });
    }
    Ok(())
}
