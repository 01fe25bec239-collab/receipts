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

# REVIEW_CAPSULE_SPEC

Everything a fresh RUNTIME-A4 needs to audit **one exact commit** — and nothing that would bias it.

## Schema

```
ReviewCapsule {
  review_id                string
  task_id                  string
  attempt_id               string
  baseline_sha             string
  implementation_sha       string        THE review anchor — exact, immutable
  objective                string        as given to the implementer
  acceptance_criteria      Criterion[]
  non_goals                string[]      so scope creep is detectable
  architecture_refs        Ref[]
  contract_refs            Ref[]
  diff                     DiffRef       baseline_sha..implementation_sha
  allowed_write_paths      glob[]        to detect write-scope violations
  checks                   CheckResult[] worker-executed, with verbatim output
  test_results             TestResult[]
  security_requirements    SecurityRequirement[]
  review_scope             enum          FULL | SECURITY | REGRESSION | REPAIR_VERIFICATION
  severity_policy          SeverityPolicy   what counts as blocking
  reproduction_required    boolean       STANDARD+ : true
  structured_output_schema SchemaRef
  context_epoch            integer
}
```

## Deliberately excluded

- **The A3's conversational history.** A4 audits the artifact, not the author's reasoning. Reading the implementer's justification is how reviewers get talked into a bad diff.
- **The previous reviewer's verdict**, except in `REPAIR_VERIFICATION` scope, where the prior blocking findings are precisely what must be confirmed closed.
- **Any hint of which model implemented it**, unless a policy needs it for independence checking. Model identity invites deference.

## Exact-SHA discipline

`implementation_sha` is the anchor. The reviewer checks out that commit read-only. If the branch has advanced, the review still applies only to that SHA.

At acceptance the gate verifies `review_sha == implementation_sha` and that no commit followed the review. Otherwise the review described code that is not the code being accepted.

## Severity policy

```
SeverityPolicy {
  blocking: [ARCHITECTURE_VIOLATION, CONTRACT_VIOLATION, SECURITY_BOUNDARY_VIOLATION,
             WRITE_SCOPE_VIOLATION, UNDISCLOSED_CHANGE, MISSING_REQUIRED_NEGATIVE_TEST,
             UNREPRODUCIBLE_EVIDENCE, OVERSTATED_LABEL, TEST_WEAKENED_OR_DELETED,
             ACCEPTANCE_CRITERION_UNMET]
  nonblocking_default: [STYLE, NAMING, MINOR_PERFORMANCE, DOC_GAP]
}
```

Configurable per assurance profile, but the blocking list may not be shortened below a floor defined in `ASSURANCE_PROFILES.md` — a project cannot configure away the checks that make acceptance meaningful.

## Output

A structured verdict conforming to `structured_output_schema`: verdict, blocking findings, nonblocking findings, per-dimension assessment, reproduction commands and the reviewer's own output, evidence check (claim vs label vs observed), and write-scope check. Free prose is not a verdict.
