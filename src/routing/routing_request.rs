//! Frozen RoutingRequest vocabulary and validated in-process storage only.
//! No parsing, policy evaluation, or executor selection.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingRequestRole {
    Implementer,
    Reviewer,
    Manager,
    Evaluator,
    Renderer,
}

impl RoutingRequestRole {
    pub const ALL: [Self; 5] = [
        Self::Implementer,
        Self::Reviewer,
        Self::Manager,
        Self::Evaluator,
        Self::Renderer,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Implementer => "IMPLEMENTER",
            Self::Reviewer => "REVIEWER",
            Self::Manager => "MANAGER",
            Self::Evaluator => "EVALUATOR",
            Self::Renderer => "RENDERER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingTaskClass {
    FrontierImplementation,
    FrontierReview,
    FrontierArchitecture,
    SecurityCriticalCode,
    BalancedReasoning,
    EconomyDocs,
    EconomySummary,
    EconomyStatus,
    PresentationOnly,
}

impl RoutingTaskClass {
    pub const ALL: [Self; 9] = [
        Self::FrontierImplementation,
        Self::FrontierReview,
        Self::FrontierArchitecture,
        Self::SecurityCriticalCode,
        Self::BalancedReasoning,
        Self::EconomyDocs,
        Self::EconomySummary,
        Self::EconomyStatus,
        Self::PresentationOnly,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FrontierImplementation => "FRONTIER_IMPLEMENTATION",
            Self::FrontierReview => "FRONTIER_REVIEW",
            Self::FrontierArchitecture => "FRONTIER_ARCHITECTURE",
            Self::SecurityCriticalCode => "SECURITY_CRITICAL_CODE",
            Self::BalancedReasoning => "BALANCED_REASONING",
            Self::EconomyDocs => "ECONOMY_DOCS",
            Self::EconomySummary => "ECONOMY_SUMMARY",
            Self::EconomyStatus => "ECONOMY_STATUS",
            Self::PresentationOnly => "PRESENTATION_ONLY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingQualityFloor {
    Frontier,
    Balanced,
    Economy,
}

impl RoutingQualityFloor {
    pub const ALL: [Self; 3] = [Self::Frontier, Self::Balanced, Self::Economy];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frontier => "FRONTIER",
            Self::Balanced => "BALANCED",
            Self::Economy => "ECONOMY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingPriority {
    Highest,
    High,
    Secondary,
    Low,
}

impl RoutingPriority {
    pub const ALL: [Self; 4] = [Self::Highest, Self::High, Self::Secondary, Self::Low];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Highest => "HIGHEST",
            Self::High => "HIGH",
            Self::Secondary => "SECONDARY",
            Self::Low => "LOW",
        }
    }
}

/// Schema constraints stored without cross-field policy.
/// `max_cost` is a finite, bounded f64 carrier, not an arbitrary-precision JSON
/// number codec. Finite negative values are valid; no currency or unit is implied.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingRequestConstraints {
    distinct_provider_from: Option<String>,
    avoid_providers: Option<Vec<String>>,
    user_pinned_model: Option<String>,
    user_pinned_provider: Option<String>,
    max_cost: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingRequestConstraintsError {
    EmptyDistinctProviderFrom,
    EmptyAvoidProvider { index: usize },
    EmptyUserPinnedModel,
    EmptyUserPinnedProvider,
    NonFiniteMaxCost,
}

impl RoutingRequestConstraints {
    /// Validates only physical schema invariants and preserves accepted input.
    pub fn try_new(
        distinct_provider_from: Option<String>,
        avoid_providers: Option<Vec<String>>,
        user_pinned_model: Option<String>,
        user_pinned_provider: Option<String>,
        max_cost: Option<f64>,
    ) -> Result<Self, RoutingRequestConstraintsError> {
        if distinct_provider_from.as_deref() == Some("") {
            return Err(RoutingRequestConstraintsError::EmptyDistinctProviderFrom);
        }
        if let Some(providers) = &avoid_providers
            && let Some(index) = providers.iter().position(String::is_empty)
        {
            return Err(RoutingRequestConstraintsError::EmptyAvoidProvider { index });
        }
        if user_pinned_model.as_deref() == Some("") {
            return Err(RoutingRequestConstraintsError::EmptyUserPinnedModel);
        }
        if user_pinned_provider.as_deref() == Some("") {
            return Err(RoutingRequestConstraintsError::EmptyUserPinnedProvider);
        }
        if max_cost.is_some_and(|value| !value.is_finite()) {
            return Err(RoutingRequestConstraintsError::NonFiniteMaxCost);
        }
        Ok(Self {
            distinct_provider_from,
            avoid_providers,
            user_pinned_model,
            user_pinned_provider,
            max_cost,
        })
    }

    pub fn distinct_provider_from(&self) -> Option<&str> {
        self.distinct_provider_from.as_deref()
    }

    pub fn avoid_providers(&self) -> Option<&[String]> {
        self.avoid_providers.as_deref()
    }

    pub fn user_pinned_model(&self) -> Option<&str> {
        self.user_pinned_model.as_deref()
    }

    pub fn user_pinned_provider(&self) -> Option<&str> {
        self.user_pinned_provider.as_deref()
    }

    pub fn max_cost(&self) -> Option<f64> {
        self.max_cost
    }
}

/// In-process non-temporal core, NOT the complete wire RoutingRequest.
/// `deadline` is deferred without an authoritative timestamp binding.
/// `execution_context` is deferred because frozen prose and schema disagree.
/// `context_size_hint` uses a bounded non-negative u64 carrier, not an
/// arbitrary-precision JSON integer codec; no units are implied.
/// Omitted priorities stay omitted; capabilities are stored without evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingRequestNonTemporalCore {
    request_id: String,
    task_id: Option<String>,
    role: RoutingRequestRole,
    task_class: RoutingTaskClass,
    quality_floor: RoutingQualityFloor,
    required_capabilities: Vec<String>,
    preferred_capabilities: Option<Vec<String>>,
    quality_priority: Option<RoutingPriority>,
    cost_priority: Option<RoutingPriority>,
    constraints: Option<RoutingRequestConstraints>,
    context_size_hint: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingRequestCoreError {
    EmptyRequestId,
    RequestIdTooLong,
    EmptyTaskId,
    TaskIdTooLong,
    EmptyRequiredCapabilities,
    EmptyRequiredCapability { index: usize },
    EmptyPreferredCapability { index: usize },
}

impl RoutingRequestNonTemporalCore {
    /// Validates only physical schema invariants and preserves accepted input.
    #[allow(clippy::too_many_arguments)] // Explicit schema fields; no hidden defaults.
    pub fn try_new(
        request_id: String,
        task_id: Option<String>,
        role: RoutingRequestRole,
        task_class: RoutingTaskClass,
        quality_floor: RoutingQualityFloor,
        required_capabilities: Vec<String>,
        preferred_capabilities: Option<Vec<String>>,
        quality_priority: Option<RoutingPriority>,
        cost_priority: Option<RoutingPriority>,
        constraints: Option<RoutingRequestConstraints>,
        context_size_hint: Option<u64>,
    ) -> Result<Self, RoutingRequestCoreError> {
        if request_id.is_empty() {
            return Err(RoutingRequestCoreError::EmptyRequestId);
        }
        if request_id.chars().count() > 200 {
            return Err(RoutingRequestCoreError::RequestIdTooLong);
        }
        if let Some(id) = &task_id {
            if id.is_empty() {
                return Err(RoutingRequestCoreError::EmptyTaskId);
            }
            if id.chars().count() > 200 {
                return Err(RoutingRequestCoreError::TaskIdTooLong);
            }
        }
        if required_capabilities.is_empty() {
            return Err(RoutingRequestCoreError::EmptyRequiredCapabilities);
        }
        if let Some(index) = required_capabilities.iter().position(String::is_empty) {
            return Err(RoutingRequestCoreError::EmptyRequiredCapability { index });
        }
        if let Some(capabilities) = &preferred_capabilities
            && let Some(index) = capabilities.iter().position(String::is_empty)
        {
            return Err(RoutingRequestCoreError::EmptyPreferredCapability { index });
        }
        Ok(Self {
            request_id,
            task_id,
            role,
            task_class,
            quality_floor,
            required_capabilities,
            preferred_capabilities,
            quality_priority,
            cost_priority,
            constraints,
            context_size_hint,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    pub fn role(&self) -> RoutingRequestRole {
        self.role
    }

    pub fn task_class(&self) -> RoutingTaskClass {
        self.task_class
    }

    pub fn quality_floor(&self) -> RoutingQualityFloor {
        self.quality_floor
    }

    pub fn required_capabilities(&self) -> &[String] {
        &self.required_capabilities
    }

    pub fn preferred_capabilities(&self) -> Option<&[String]> {
        self.preferred_capabilities.as_deref()
    }

    pub fn quality_priority(&self) -> Option<RoutingPriority> {
        self.quality_priority
    }

    pub fn cost_priority(&self) -> Option<RoutingPriority> {
        self.cost_priority
    }

    pub fn constraints(&self) -> Option<&RoutingRequestConstraints> {
        self.constraints.as_ref()
    }

    pub fn context_size_hint(&self) -> Option<u64> {
        self.context_size_hint
    }
}
