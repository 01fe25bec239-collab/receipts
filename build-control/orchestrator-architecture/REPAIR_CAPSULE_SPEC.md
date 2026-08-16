<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# REPAIR_CAPSULE_SPEC

Derived from the Task Capsule, carrying the audit findings. Its purpose: a **completely different agent, on a different provider**, can continue the work with no access to the original session.

## Schema

```
RepairCapsule extends TaskCapsule {
  original_task_id         string
  attempt_number           integer            2, 3, …
  parent_attempt_id        string
  current_sha              string             rejected implementation SHA
  original_objective       string             unchanged from the parent
  original_acceptance_criteria Criterion[]    unchanged
  a4_findings              Finding[]          full structured findings
  blocking_findings        Finding[]          subset that MUST be closed
  failed_checks            CheckResult[]      with verbatim output
  repair_scope             string             exactly what may change
  prior_attempt_summary    string             factual: what was tried, what failed
  relevant_context_refs    Ref[]              possibly widened by the findings
  required_quality_floor   enum               never lower than the parent's
}
```

## Rules

1. **Objective and acceptance criteria are unchanged.** A repair fixes the implementation, never the target. Loosening criteria to make a repair pass is the failure mode this rule prevents.
2. **Repair scope is bounded by the findings.** No new features, no adjacent refactoring, no "improving things while we're here". Unrelated changes in a repair are themselves a blocking finding.
3. **Quality floor never decreases.** A rejected frontier task does not get repaired by an economy model.
4. **No conversational inheritance.** `prior_attempt_summary` is a factual record produced by the orchestrator from the handoff and findings — not a transcript, and not the previous agent's self-assessment.
5. **The rejected SHA is preserved**, referenced, and never deleted.

## Why a fresh agent, not a resumed one

The original A3 already produced work a reviewer rejected. Its context contains the reasoning that led there. A fresh agent reading the findings and the current code starts from the reviewer's frame instead of the author's — and, in practice, an author defending its own design is exactly what a repair does not need.

There is a real trade-off: the fresh agent loses undocumented context. That is why `prior_attempt_summary` and possibly-widened `relevant_context_refs` exist, and why handoff quality is enforced at A3 termination.

## Executor selection

Routing consults the router as for any dispatch:

| Situation | Preference |
|---|---|
| Mechanical findings, provider available | Same tier; same provider acceptable |
| Original provider unavailable | Another eligible frontier executor |
| Contract/architecture misunderstanding | Prefer a **different model** |
| Security-boundary finding | Frontier floor; stricter reviewer independence |

## Validation before dispatch

Blocking findings non-empty (otherwise it is not a repair); `current_sha` exists; attempt number within bound; quality floor ≥ parent; repair scope non-empty; epoch current.
