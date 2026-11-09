//! Integration-layer adapters binding consumer-owned ports to real engines.
//!
//! [`WorkspaceWriteScopeMatcher`] implements Orchestration's `WriteScopePort`
//! with Workspace's shared write-path parser and intersection engine (no
//! second glob language). Production composition of this port remains the
//! later composition-root task; this crate exists for library integration
//! controls only.

use receipts_orchestration::scheduler::{WriteScopeOverlapV1, WriteScopePort};
use receipts_workspace_execution::{WritePathPattern, WritePathSet, intersects};

/// Orchestration's write-scope port over the real Workspace matcher.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceWriteScopeMatcher;

fn parse(paths: &[String]) -> Option<WritePathSet> {
    paths
        .iter()
        .map(|p| WritePathPattern::parse(p).ok())
        .collect::<Option<Vec<_>>>()
        .map(WritePathSet::new)
}

impl WriteScopePort for WorkspaceWriteScopeMatcher {
    fn overlap(&self, left: &[String], right: &[String]) -> WriteScopeOverlapV1 {
        let (Some(left), Some(right)) = (parse(left), parse(right)) else {
            // An unparsable pattern proves nothing about overlap.
            return WriteScopeOverlapV1::Undetermined;
        };
        match intersects(&left, &right) {
            Ok(None) => WriteScopeOverlapV1::Disjoint,
            Ok(Some(witness)) => WriteScopeOverlapV1::Overlaps { witness },
            // ResourceLimit is uncertainty, never disjointness.
            Err(_) => WriteScopeOverlapV1::Undetermined,
        }
    }
}
