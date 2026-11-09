//! Orchestration-owned write-scope overlap port (S10 collision input).
//!
//! OR depends on no matcher implementation. The composition (and the
//! receipts-e2e integration layer) binds this port to Workspace's shared
//! path-language engine. Uncertainty is never evidence of disjointness: an
//! undetermined answer defers the second writer instead of admitting it.

/// What the bound matcher proved about two write-path sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteScopeOverlapV1 {
    /// The sets provably share no path.
    Disjoint,
    /// A concrete common path both sets accept.
    Overlaps { witness: String },
    /// Invalid input or an exhausted resource limit: no proof either way.
    Undetermined,
}

/// Consumer-owned port; implementations translate their engine's answer.
pub trait WriteScopePort {
    /// Proves every pattern in a nonempty candidate scope is valid.
    /// False includes invalid input and inability to validate; never assume validity.
    fn validate(&self, paths: &[String]) -> bool;
    fn overlap(&self, left: &[String], right: &[String]) -> WriteScopeOverlapV1;
}

/// The scheduling consequence of an overlap answer for two writers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteScopeCollisionV1 {
    /// Proven disjoint: the two writers may run concurrently.
    Concurrent,
    /// Proven overlap: the writers must not run concurrently.
    Conflict { witness: String },
    /// Unknown overlap: defer (refuse concurrent admission), never pass.
    Defer,
}

/// Classifies two writers' scopes through the bound port.
pub fn classify_write_collision(
    port: &impl WriteScopePort,
    left: &[String],
    right: &[String],
) -> WriteScopeCollisionV1 {
    match port.overlap(left, right) {
        WriteScopeOverlapV1::Disjoint => WriteScopeCollisionV1::Concurrent,
        WriteScopeOverlapV1::Overlaps { witness } => WriteScopeCollisionV1::Conflict { witness },
        WriteScopeOverlapV1::Undetermined => WriteScopeCollisionV1::Defer,
    }
}
