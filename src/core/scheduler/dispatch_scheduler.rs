//! Deterministic dispatch scheduling over one graph version (OR-W2-1/2).
//!
//! [`schedule_dispatch_v1`] decides, for each candidate the composition
//! offers, exactly one of: admitted, denied by D11, deferred on a named
//! capacity/collision axis, or refused before evaluation. It performs no
//! dispatch, routing, mutation or persistence; the caller acts on the
//! admitted list.
//!
//! Fixed check order per candidate (the first failing check is the
//! authoritative axis):
//!
//! 1. Refusals: duplicate candidate, node not in the graph, no workstream,
//!    empty write scope, request bound to another node/graph, request bound
//!    to a context epoch other than the trusted current one, node not ready
//!    per [`TaskDag`] (unavailable interface evidence included).
//! 2. D11: [`derive_dispatch_admission`]. A typed derivation refusal is a
//!    refusal; DENY is a denial (zero dispatch). Missing or stale evidence
//!    never becomes ALLOW (see ADR-D11).
//! 3. Budgets: remaining worker starts, then cost (an unknown estimate under
//!    a cost cap defers; it is never treated as free).
//! 4. Concurrency: global, then the node's workstream, then the D11-selected
//!    provider. A workstream or provider without a configured limit defers.
//! 5. Write collision against every in-flight writer, then every writer
//!    admitted earlier in this round, through [`WriteScopePort`]. Only proven
//!    disjointness admits; an overlap defers with its witness and an
//!    undetermined answer defers.
//!
//! Candidates that pass step 1 are processed in priority order: longest
//! downstream precedence chain, then most transitive dependents, then oldest
//! `ready_sequence`, then highest `user_priority`, then node id. Capacity
//! consumed by an earlier admission is unavailable to later candidates;
//! that is the backpressure: once a limit is full, every later candidate on
//! that axis is deferred rather than failed.

use std::collections::{BTreeMap, BTreeSet};

use super::write_scope_port::{WriteScopeCollisionV1, WriteScopePort, classify_write_collision};
use super::{
    DispatchAdmissionDerivationV1, DispatchAdmissionRefusalV1, DispatchAdmissionRequestV1,
    derive_dispatch_admission,
};
use crate::dag::{InterfaceEvidenceVerifier, TaskDag};
use crate::{ExecutionGraph, ExecutionGraphEnvelope};

/// One node the composition proposes for dispatch, with its D11 evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchCandidateV1 {
    pub node_id: String,
    /// The capsule's `allowed_write_paths` (Workspace path language).
    pub write_paths: Vec<String>,
    /// Estimated cost in the goal budget's unit; `None` is unknown.
    pub cost_estimate: Option<u64>,
    /// Trusted monotonic order in which the node became ready (smaller is older).
    pub ready_sequence: u64,
    /// User priority; higher runs first among otherwise equal candidates.
    pub user_priority: i64,
    pub admission: DispatchAdmissionRequestV1,
}

/// A writer already running, as recorded by trusted core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlightWriterV1 {
    pub node_id: String,
    pub workstream_id: String,
    pub provider_id: String,
    pub write_paths: Vec<String>,
}

/// Concurrency limits. Absent workstream/provider entries are unconfigured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConcurrencyLimitsV1 {
    pub global: usize,
    pub per_workstream: BTreeMap<String, usize>,
    pub per_provider: BTreeMap<String, usize>,
}

/// Remaining goal budget. `remaining_cost: None` means no cost cap applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchBudgetV1 {
    pub remaining_worker_starts: u64,
    pub remaining_cost: Option<u64>,
}

/// Trusted inputs for one scheduling round.
pub struct SchedulingContextV1<'a> {
    pub envelope: &'a ExecutionGraphEnvelope,
    pub interface: Option<InterfaceEvidenceVerifier<'a>>,
    /// The project's current context epoch, read by trusted core from State.
    pub current_context_epoch: &'a str,
    pub limits: &'a ConcurrencyLimitsV1,
    pub budget: DispatchBudgetV1,
    pub in_flight: &'a [InFlightWriterV1],
}

/// Why a candidate was not evaluated at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingRefusalV1 {
    DuplicateCandidate,
    UnknownNode,
    NodeWithoutWorkstream,
    EmptyWriteScope,
    /// The admission request names another node or graph.
    BindingMismatch,
    /// The admission request is bound to a superseded (or future) epoch.
    StaleContextEpoch,
    /// Dependencies are unmet or their evidence is unavailable.
    NotReady,
    /// The D11 evaluator refused the evidence before deciding.
    Evidence(DispatchAdmissionRefusalV1),
    /// ALLOW without a selected provider cannot be placed on a provider axis.
    NoSelectedProvider,
}

/// Concurrency and budget axes a deferral can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferralV1 {
    WorkerBudgetExhausted,
    CostEstimateUnknown,
    CostBudgetExhausted,
    GlobalConcurrency,
    WorkstreamConcurrency {
        workstream_id: String,
    },
    WorkstreamLimitUnconfigured {
        workstream_id: String,
    },
    ProviderConcurrency {
        provider_id: String,
    },
    ProviderLimitUnconfigured {
        provider_id: String,
    },
    /// Proven overlap with a running or just-admitted writer.
    WriteConflict {
        with: String,
        witness: String,
    },
    /// The matcher could not prove disjointness.
    WriteOverlapUndetermined {
        with: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingDecisionV1 {
    Admitted {
        derivation: DispatchAdmissionDerivationV1,
    },
    /// D11 DENY: zero dispatch; the derivation names the failing axis.
    Denied {
        derivation: DispatchAdmissionDerivationV1,
    },
    Deferred {
        derivation: DispatchAdmissionDerivationV1,
        reason: DeferralV1,
    },
    Refused(SchedulingRefusalV1),
}

/// The deterministic priority of one candidate (compared in field order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPriorityV1 {
    /// Nodes on the longest downstream precedence chain, this node included.
    pub critical_path: usize,
    /// Distinct transitive precedence dependents.
    pub blocking_count: usize,
    pub ready_sequence: u64,
    pub user_priority: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledCandidateV1 {
    pub node_id: String,
    /// `None` for candidates refused before prioritisation.
    pub priority: Option<DispatchPriorityV1>,
    pub decision: SchedulingDecisionV1,
}

/// Decisions in processing order: prioritised candidates first, then
/// refused ones in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchScheduleV1 {
    pub decisions: Vec<ScheduledCandidateV1>,
}

impl DispatchScheduleV1 {
    /// Admitted node ids in dispatch order.
    pub fn admitted(&self) -> Vec<&str> {
        self.decisions
            .iter()
            .filter(|d| matches!(d.decision, SchedulingDecisionV1::Admitted { .. }))
            .map(|d| d.node_id.as_str())
            .collect()
    }

    pub fn decision(&self, node_id: &str) -> Option<&SchedulingDecisionV1> {
        self.decisions
            .iter()
            .find(|d| d.node_id == node_id)
            .map(|d| &d.decision)
    }
}

struct Writer<'a> {
    node_id: &'a str,
    write_paths: &'a [String],
}

/// Runs one deterministic scheduling round. See the module docs.
pub fn schedule_dispatch_v1(
    context: &SchedulingContextV1<'_>,
    candidates: &[DispatchCandidateV1],
    port: &impl WriteScopePort,
) -> DispatchScheduleV1 {
    let graph = context.envelope.graph();
    let ready: BTreeSet<String> = TaskDag::new(context.envelope, context.interface)
        .ready_nodes()
        .ready
        .into_iter()
        .collect();
    let metrics = PrecedenceMetrics::new(graph);

    let mut seen = BTreeSet::new();
    let mut refused = Vec::new();
    let mut queue = Vec::new();
    for candidate in candidates {
        let refusal = if !seen.insert(candidate.node_id.as_str()) {
            Some(SchedulingRefusalV1::DuplicateCandidate)
        } else {
            precheck(context, &ready, candidate).err()
        };
        match refusal {
            Some(refusal) => refused.push(ScheduledCandidateV1 {
                node_id: candidate.node_id.clone(),
                priority: None,
                decision: SchedulingDecisionV1::Refused(refusal),
            }),
            None => {
                let (critical_path, blocking_count) = metrics.of(&candidate.node_id);
                let priority = DispatchPriorityV1 {
                    critical_path,
                    blocking_count,
                    ready_sequence: candidate.ready_sequence,
                    user_priority: candidate.user_priority,
                };
                queue.push((priority, candidate));
            }
        }
    }
    queue.sort_by(|(a, x), (b, y)| {
        b.critical_path
            .cmp(&a.critical_path)
            .then(b.blocking_count.cmp(&a.blocking_count))
            .then(a.ready_sequence.cmp(&b.ready_sequence))
            .then(b.user_priority.cmp(&a.user_priority))
            .then(x.node_id.cmp(&y.node_id))
    });

    let mut global = context.in_flight.len();
    let mut per_workstream: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_provider: BTreeMap<String, usize> = BTreeMap::new();
    for writer in context.in_flight {
        *per_workstream.entry(&writer.workstream_id).or_default() += 1;
        *per_provider.entry(writer.provider_id.clone()).or_default() += 1;
    }
    let mut writers: Vec<Writer<'_>> = context
        .in_flight
        .iter()
        .map(|w| Writer {
            node_id: &w.node_id,
            write_paths: &w.write_paths,
        })
        .collect();
    let mut workers = context.budget.remaining_worker_starts;
    let mut cost = context.budget.remaining_cost;

    let mut decisions = Vec::with_capacity(candidates.len());
    for (priority, candidate) in queue {
        let workstream = graph
            .node(&candidate.node_id)
            .and_then(|n| n.workstream_id())
            .expect("prechecked workstream");
        let decision = match derive_dispatch_admission(&candidate.admission) {
            Err(refusal) => SchedulingDecisionV1::Refused(SchedulingRefusalV1::Evidence(refusal)),
            Ok(derivation) if !derivation.dispatch_permitted() => {
                SchedulingDecisionV1::Denied { derivation }
            }
            Ok(derivation) => match derivation.decision().core().selected_provider() {
                None => SchedulingDecisionV1::Refused(SchedulingRefusalV1::NoSelectedProvider),
                Some(provider) => {
                    let provider = provider.to_owned();
                    let deferral = (|| {
                        if workers == 0 {
                            return Some(DeferralV1::WorkerBudgetExhausted);
                        }
                        if let Some(remaining) = cost {
                            match candidate.cost_estimate {
                                None => return Some(DeferralV1::CostEstimateUnknown),
                                Some(estimate) if estimate > remaining => {
                                    return Some(DeferralV1::CostBudgetExhausted);
                                }
                                Some(_) => {}
                            }
                        }
                        if global >= context.limits.global {
                            return Some(DeferralV1::GlobalConcurrency);
                        }
                        match context.limits.per_workstream.get(workstream) {
                            None => {
                                return Some(DeferralV1::WorkstreamLimitUnconfigured {
                                    workstream_id: workstream.to_owned(),
                                });
                            }
                            Some(limit)
                                if per_workstream.get(workstream).copied().unwrap_or(0)
                                    >= *limit =>
                            {
                                return Some(DeferralV1::WorkstreamConcurrency {
                                    workstream_id: workstream.to_owned(),
                                });
                            }
                            Some(_) => {}
                        }
                        match context.limits.per_provider.get(&provider) {
                            None => {
                                return Some(DeferralV1::ProviderLimitUnconfigured {
                                    provider_id: provider.clone(),
                                });
                            }
                            Some(limit)
                                if per_provider.get(&provider).copied().unwrap_or(0) >= *limit =>
                            {
                                return Some(DeferralV1::ProviderConcurrency {
                                    provider_id: provider.clone(),
                                });
                            }
                            Some(_) => {}
                        }
                        writers.iter().find_map(|writer| {
                            match classify_write_collision(
                                port,
                                &candidate.write_paths,
                                writer.write_paths,
                            ) {
                                WriteScopeCollisionV1::Concurrent => None,
                                WriteScopeCollisionV1::Conflict { witness } => {
                                    Some(DeferralV1::WriteConflict {
                                        with: writer.node_id.to_owned(),
                                        witness,
                                    })
                                }
                                WriteScopeCollisionV1::Defer => {
                                    Some(DeferralV1::WriteOverlapUndetermined {
                                        with: writer.node_id.to_owned(),
                                    })
                                }
                            }
                        })
                    })();
                    match deferral {
                        Some(reason) => SchedulingDecisionV1::Deferred { derivation, reason },
                        None => {
                            workers -= 1;
                            if let (Some(remaining), Some(estimate)) =
                                (cost.as_mut(), candidate.cost_estimate)
                            {
                                *remaining -= estimate;
                            }
                            global += 1;
                            *per_workstream.entry(workstream).or_default() += 1;
                            *per_provider.entry(provider).or_default() += 1;
                            writers.push(Writer {
                                node_id: &candidate.node_id,
                                write_paths: &candidate.write_paths,
                            });
                            SchedulingDecisionV1::Admitted { derivation }
                        }
                    }
                }
            },
        };
        decisions.push(ScheduledCandidateV1 {
            node_id: candidate.node_id.clone(),
            priority: Some(priority),
            decision,
        });
    }
    decisions.extend(refused);
    DispatchScheduleV1 { decisions }
}

fn precheck(
    context: &SchedulingContextV1<'_>,
    ready: &BTreeSet<String>,
    candidate: &DispatchCandidateV1,
) -> Result<(), SchedulingRefusalV1> {
    let node = context
        .envelope
        .graph()
        .node(&candidate.node_id)
        .ok_or(SchedulingRefusalV1::UnknownNode)?;
    if node.workstream_id().is_none() {
        return Err(SchedulingRefusalV1::NodeWithoutWorkstream);
    }
    if candidate.write_paths.is_empty() {
        return Err(SchedulingRefusalV1::EmptyWriteScope);
    }
    let binding = &candidate.admission.binding;
    if binding.node_id != candidate.node_id
        || binding.graph_id.as_deref() != Some(context.envelope.graph_id())
    {
        return Err(SchedulingRefusalV1::BindingMismatch);
    }
    if binding.context_epoch != context.current_context_epoch {
        return Err(SchedulingRefusalV1::StaleContextEpoch);
    }
    if !ready.contains(&candidate.node_id) {
        return Err(SchedulingRefusalV1::NotReady);
    }
    Ok(())
}

/// Downstream precedence metrics, memoised per node.
struct PrecedenceMetrics<'g> {
    successors: BTreeMap<&'g str, Vec<&'g str>>,
}

impl<'g> PrecedenceMetrics<'g> {
    fn new(graph: &'g ExecutionGraph) -> Self {
        let mut successors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for edge in graph.precedence_edges() {
            successors
                .entry(edge.from_node())
                .or_default()
                .push(edge.to_node());
        }
        Self { successors }
    }

    /// (critical path, transitive dependents). The precedence subgraph is
    /// acyclic by construction (`ExecutionGraph` refuses cycles).
    // ponytail: per-candidate traversal, O(candidates x edges); memoise across candidates if graphs grow large.
    fn of(&self, node: &str) -> (usize, usize) {
        let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
        let critical = self.depth(node, &mut depth);
        let mut seen = BTreeSet::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            for next in self.successors.get(current).into_iter().flatten() {
                if seen.insert(*next) {
                    stack.push(next);
                }
            }
        }
        (critical, seen.len())
    }

    fn depth<'a>(&'a self, node: &'a str, memo: &mut BTreeMap<&'a str, usize>) -> usize {
        if let Some(value) = memo.get(node) {
            return *value;
        }
        let below = self
            .successors
            .get(node)
            .into_iter()
            .flatten()
            .map(|next| self.depth(next, memo))
            .max()
            .unwrap_or(0);
        memo.insert(node, below + 1);
        below + 1
    }
}
