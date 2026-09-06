//! Closed capsule-local selectors; no routing or runtime behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    Implementation,
    Repair,
    Test,
    Refactor,
    Migration,
    Docs,
    Investigation,
    SecurityFix,
}
impl TaskType {
    pub const ALL: [Self; 8] = [
        Self::Implementation,
        Self::Repair,
        Self::Test,
        Self::Refactor,
        Self::Migration,
        Self::Docs,
        Self::Investigation,
        Self::SecurityFix,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Implementation => "IMPLEMENTATION",
            Self::Repair => "REPAIR",
            Self::Test => "TEST",
            Self::Refactor => "REFACTOR",
            Self::Migration => "MIGRATION",
            Self::Docs => "DOCS",
            Self::Investigation => "INVESTIGATION",
            Self::SecurityFix => "SECURITY_FIX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CriterionKind {
    Deterministic,
    Semantic,
}
impl CriterionKind {
    pub const ALL: [Self; 2] = [Self::Deterministic, Self::Semantic];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Deterministic => "DETERMINISTIC",
            Self::Semantic => "SEMANTIC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefType {
    RepoPath,
    StateQuery,
    ArtifactId,
    Url,
}
impl RefType {
    pub const ALL: [Self; 4] = [
        Self::RepoPath,
        Self::StateQuery,
        Self::ArtifactId,
        Self::Url,
    ];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::RepoPath => "REPO_PATH",
            Self::StateQuery => "STATE_QUERY",
            Self::ArtifactId => "ARTIFACT_ID",
            Self::Url => "URL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QualityFloor {
    Frontier,
    Balanced,
    Economy,
}
impl QualityFloor {
    pub const ALL: [Self; 3] = [Self::Frontier, Self::Balanced, Self::Economy];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Frontier => "FRONTIER",
            Self::Balanced => "BALANCED",
            Self::Economy => "ECONOMY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistinctProvider {
    Off,
    Preferred,
    Required,
}
impl DistinctProvider {
    pub const ALL: [Self; 3] = [Self::Off, Self::Preferred, Self::Required];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Preferred => "PREFERRED",
            Self::Required => "REQUIRED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssuranceProfileSelector {
    Light,
    Standard,
    HighAssurance,
}
impl AssuranceProfileSelector {
    pub const ALL: [Self; 3] = [Self::Light, Self::Standard, Self::HighAssurance];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "LIGHT",
            Self::Standard => "STANDARD",
            Self::HighAssurance => "HIGH_ASSURANCE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostPriority {
    Highest,
    High,
    Secondary,
    Low,
}
impl CostPriority {
    pub const ALL: [Self; 4] = [Self::Highest, Self::High, Self::Secondary, Self::Low];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Highest => "HIGHEST",
            Self::High => "HIGH",
            Self::Secondary => "SECONDARY",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemotePublishPolicy {
    LocalOnly,
    PushOnAccept,
    PushAlways,
}
impl RemotePublishPolicy {
    pub const ALL: [Self; 3] = [Self::LocalOnly, Self::PushOnAccept, Self::PushAlways];
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LocalOnly => "LOCAL_ONLY",
            Self::PushOnAccept => "PUSH_ON_ACCEPT",
            Self::PushAlways => "PUSH_ALWAYS",
        }
    }
}
