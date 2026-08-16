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

# ASSURANCE_PROFILES

Not every task deserves the same scrutiny. Charging maximum assurance everywhere makes the system too slow and expensive to use, which in practice means it gets bypassed — the worst outcome for assurance.

## Profiles (§70)

| | **LIGHT** | **STANDARD** (default) | **HIGH_ASSURANCE** |
|---|---|---|---|
| Worker implementation | ✔ | ✔ | ✔ |
| Worker executes checks | ✔ | ✔ | ✔ |
| Captured worker evidence | ✔ sufficient | ✔ | ✔ |
| Independent A4 review | optional | **required** | **required** |
| A4 reproduces acceptance checks | – | **required** | **required** |
| Exact code-state binding | ✔ | ✔ | ✔ |
| Broker re-runs deterministic checks | – | – | **required** |
| Reviewer quality floor | BALANCED | FRONTIER | FRONTIER |
| `distinct_provider` | OFF | PREFERRED | **REQUIRED** |
| Security pipeline | – | on security tasks | **always** |
| Write-scope verification | ✔ | ✔ | ✔ |

Exact code-state binding and write-scope verification appear in every profile. They are cheap and they are what make any evidence interpretable.

## Selection

| Task type | Default |
|---|---|
| Production code | STANDARD |
| Security-sensitive code | HIGH_ASSURANCE |
| Architecture / interface | HIGH_ASSURANCE |
| Migration | HIGH_ASSURANCE |
| Routine refactor | STANDARD |
| Tests | STANDARD |
| Documentation (non-architectural) | LIGHT |
| Status / metadata | LIGHT |

Set per task in the capsule; A1 may raise the project floor. **A profile may be raised at any time; lowering it below the project floor requires an explicit recorded decision** — otherwise assurance erodes quietly under schedule pressure.

## The blocking floor

Whatever the profile, the following are always blocking findings: architecture violation, contract violation, security-boundary violation, write-scope violation, undisclosed change, unreproducible evidence, overstated evidence label, and a test weakened or deleted to make a build pass.

`ASSURANCE_PROFILES` tunes how much verification is performed. It never tunes away the definition of an unacceptable result.

## Cost

LIGHT ≈ 1 implementation. STANDARD ≈ implementation + audit (+ reproduction). HIGH_ASSURANCE ≈ implementation + broker re-execution + frontier cross-provider audit + security pipeline.

Roughly: STANDARD is about twice LIGHT; HIGH_ASSURANCE about twice STANDARD. That ratio is why profile selection is a real decision rather than a formality — and why `EXPECTED_COST_TO_ACCEPTED_RESULT.md` includes review cost in every routing estimate.
