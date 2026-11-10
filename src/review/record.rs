//! Records a pure proposal (a repair decision, a disposition set, an
//! acceptance decision) through the sink contract: claim, seal, publish.
//!
//! Review itself executes a reviewer between claim and seal; these operations
//! execute nothing, but they publish through the same exactly-one claim and
//! the same sealed, atomic, idempotent publication, so no Review path reaches
//! the durable store any other way.

use crate::ports::{ReviewClaim, ReviewRecordProposal, ReviewRecordSink, SealedReview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// The package is sealed and published (or was, exactly, before).
    Published,
    /// The proposal holds no record and no event, so there is nothing to
    /// claim or publish.
    NothingToRecord,
}

/// Claims `proposal.binding()`, seals the proposal under `outcome` and
/// publishes it. A claim that is already sealed is accepted only when it
/// sealed exactly this package under this outcome (then publication simply
/// completes); a claim that is in flight, terminal or abandoned, or sealed
/// with anything else, is an error and nothing is written.
pub fn record_proposal(
    sink: &mut dyn ReviewRecordSink,
    proposal: &ReviewRecordProposal,
    outcome: &str,
) -> Result<Recorded, String> {
    if ReviewRecordProposal::from_journal(&proposal.to_journal())? != *proposal {
        return Err("the proposal differs from its recovery journal".into());
    }
    if proposal.decision_records.is_empty() && proposal.events.is_empty() {
        return Ok(Recorded::NothingToRecord);
    }
    let sealed = SealedReview::new(proposal, outcome, true, 0);
    // Refuse a label or package that cannot settle before touching the sink.
    sealed.settlement()?;
    let binding = proposal.binding();
    match sink.claim(binding)? {
        ReviewClaim::Acquired => sink.seal(binding, &sealed)?,
        ReviewClaim::Sealed(existing)
            if existing.outcome == sealed.outcome
                && existing.proposal()?.to_journal() == proposal.to_journal() => {}
        ReviewClaim::Sealed(_) => {
            return Err("a different package is already sealed for this claim".into());
        }
        ReviewClaim::InFlight => {
            return Err(
                "the claim is held without a sealed result; recovery is an operator decision"
                    .into(),
            );
        }
        ReviewClaim::Terminal(outcome) => {
            return Err(format!(
                "the claim already finished without a package: {outcome}"
            ));
        }
        ReviewClaim::Abandoned => {
            return Err("the claim was abandoned; a new generation is a trusted decision".into());
        }
    }
    sink.publish(binding, proposal)?;
    Ok(Recorded::Published)
}
