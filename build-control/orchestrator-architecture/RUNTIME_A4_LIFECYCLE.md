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

# RUNTIME_A4_LIFECYCLE

**RUNTIME-A4 is an ephemeral independent auditor.** Fresh session, exact SHA, structured verdict, termination.

## Lifecycle

```
CREATE FRESH SESSION
      ↓
LOAD REVIEW CAPSULE
      ↓
READ EXACT IMPLEMENTATION SHA     ← read-only checkout of that commit
      ↓
FULL AUDIT
      ↓
STRUCTURED VERDICT
      ↓
TERMINATE SESSION
```

## Independence

**Structural, not advisory.** A4 must be a different session from the A3 that produced the work. A self-review is void and is rejected before the verdict is read.

Provider/model diversity is **policy-controlled**, not hard-wired:

| `distinct_provider` | Meaning |
|---|---|
| `OFF` | Any eligible frontier reviewer |
| `PREFERRED` | Prefer a different provider; accept same-provider if none available, and record the downgrade |
| `REQUIRED` | Must be a different provider; block if unavailable |

Defaults: `PREFERRED` under STANDARD, `REQUIRED` under HIGH_ASSURANCE. Mandating cross-vendor review everywhere would make the system unusable when one provider is down, which is why §8's "do not hardcode cross-vendor review as mandatory" is respected.

## What A4 receives

Original objective; acceptance criteria; relevant architecture; relevant contracts; baseline SHA; **exact** implementation SHA; the complete relevant diff; worker checks and results; applicable security context.

What it does **not** receive: the A3's conversational history. Auditing the reasoning rather than the artifact is how a reviewer gets talked into a bad diff.

## Exact-SHA binding (I-5)

Every review names the SHA it audited. A review of `abc` never validates `xyz`. If the branch advances after handoff, the review still applies to the recorded SHA and to nothing else; new commits require a new review.

At acceptance, the gate verifies `review_sha == implementation_sha` and that no commit was added after review. A mismatch voids the review — this is the check that makes the whole assurance chain meaningful.

## Audit dimensions

Scope compliance; acceptance criteria; contract compliance; architecture compliance; correctness; error handling; security and trust boundaries; test adequacy; negative tests; regression risk; write-scope compliance (diff vs allowed paths); undisclosed changes; evidence accuracy (labels vs reality).

"Not applicable" is an acceptable answer for a dimension. Silence is not.

## Reproduction

Under STANDARD and above, A4 independently executes the acceptance-critical checks from a clean read-only checkout of the reviewed SHA and records its own output. A review that only reads the implementer's transcript verifies the transcript, not the software.

If reproduction is impossible in the reviewer's environment, that is recorded as a limitation of the review — never silently omitted.

## Verdicts

`PASS` · `PASS_WITH_NONBLOCKING_FINDINGS` · `REJECT`

"Looks good" is not a verdict. Blocking findings always include: architecture or contract violation; security-boundary violation or reversed fail direction; missing required negative test; a changed path outside the allowed set; an undisclosed change; unreproducible evidence or an overstated label; a test weakened or deleted to make a build pass.

## Prohibited

Modifying the implementation, branch, or worktree; reviewing its own work; negotiating a verdict; reviewing a SHA other than the one recorded.

**A4 does not fix what it finds.** Findings return to A2, which decides whether to repair. A reviewer that also repairs is no longer an independent check on the repair.
