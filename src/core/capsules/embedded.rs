//! Embedded records belonging only to the TaskCapsule shape.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    id: String,
    description: String,
    kind: CriterionKind,
    check_command: Option<Vec<String>>,
    rationale: Option<String>,
}

impl Criterion {
    pub fn try_new(
        id: String,
        description: String,
        kind: CriterionKind,
        check_command: Option<Vec<String>>,
        rationale: Option<String>,
    ) -> Result<Self, CapsuleError> {
        validate_identifier("acceptance_criteria.id", &id)?;
        if description.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "acceptance_criteria.description",
            });
        }
        Ok(Self {
            id,
            description,
            kind,
            check_command,
            rationale,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn kind(&self) -> CriterionKind {
        self.kind
    }
    pub fn check_command(&self) -> Option<&[String]> {
        self.check_command.as_deref()
    }
    pub fn rationale(&self) -> Option<&str> {
        self.rationale.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    ref_type: RefType,
    target: String,
    digest: Option<String>,
    section: Option<String>,
}

impl Ref {
    pub fn try_new(
        ref_type: RefType,
        target: String,
        digest: Option<String>,
        section: Option<String>,
    ) -> Result<Self, CapsuleError> {
        if target.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "reference.target",
            });
        }
        Ok(Self {
            ref_type,
            target,
            digest,
            section,
        })
    }
    pub fn ref_type(&self) -> RefType {
        self.ref_type
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewPolicy {
    required: Option<bool>,
    distinct_provider: Option<DistinctProvider>,
    reviewer_floor: Option<QualityFloor>,
}

impl ReviewPolicy {
    pub fn try_new(
        required: Option<bool>,
        distinct_provider: Option<DistinctProvider>,
        reviewer_floor: Option<QualityFloor>,
    ) -> Result<Self, CapsuleError> {
        Ok(Self {
            required,
            distinct_provider,
            reviewer_floor,
        })
    }
    pub fn required(&self) -> Option<bool> {
        self.required
    }
    pub fn distinct_provider(&self) -> Option<DistinctProvider> {
        self.distinct_provider
    }
    pub fn reviewer_floor(&self) -> Option<QualityFloor> {
        self.reviewer_floor
    }
}

/// `max_cost`: None = absent; Some(None) = null; Some(Some(n)) = finite number.
#[derive(Debug, Clone, PartialEq)]
pub struct CostPolicy {
    max_cost: Option<Option<f64>>,
    priority: Option<CostPriority>,
}

impl CostPolicy {
    pub fn try_new(
        max_cost: Option<Option<f64>>,
        priority: Option<CostPriority>,
    ) -> Result<Self, CapsuleError> {
        if let Some(Some(value)) = max_cost
            && !value.is_finite()
        {
            return Err(CapsuleError::NonFiniteMaxCost);
        }
        Ok(Self { max_cost, priority })
    }
    pub fn max_cost(&self) -> Option<Option<f64>> {
        self.max_cost
    }
    pub fn priority(&self) -> Option<CostPriority> {
        self.priority
    }
}
