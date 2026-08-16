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

# SECURITY_REVIEW_ARCHITECTURE

## The problem with naive security review (§72)

> Security review must not mean only: ask one LLM "is this secure?"

A single model asked that question produces plausible prose, misses whole classes of defect, and cannot be reproduced. It is the appearance of a security control.

## Pipeline

```
implementation SHA
        │
   ┌────┼─────────────┬────────────────┬──────────────┐
   ▼    ▼             ▼                ▼              ▼
frontier   static     dependency /   project      secret /
defensive  analysis   vulnerability  security     config
LLM review            tooling        tests        checks
   └────┬─────────────┴────────────────┴──────────────┘
        ▼
  normalized structured findings (deduplicated, severity-ranked)
        ▼
  A4 SECURITY verdict
```

Deterministic tooling and model reasoning cover different failure classes: tools find known patterns reliably and novel design flaws never; a frontier reviewer is the reverse. Neither alone is a security review.

## Tooling is project-specific

Exact tools depend on language and ecosystem. **No universal scanner is invented.** The pipeline defines the *shape* — deterministic checks plus model reasoning, normalized into one finding schema — and the concrete tool set is a per-project configuration (Q-06).

Where no deterministic tooling is configured, the review says so explicitly and its verdict is qualified. A model-only security review is labelled as one rather than presented as a full audit.

## Findings

```
SecurityFinding { finding_id, source: LLM_REVIEW|STATIC_ANALYSIS|DEPENDENCY_SCAN|TEST|CONFIG_CHECK,
                  severity: INFO|LOW|MEDIUM|HIGH|CRITICAL,
                  category, path, line?, description, evidence_ref,
                  confidence, reproduction?, false_positive_risk }
```

Deduplicated across sources; agreement between an independent tool and the model raises confidence, disagreement is preserved rather than averaged away.

## Routing

Security review is `SECURITY_CRITICAL_CODE`: frontier floor, `distinct_provider: PREFERRED` under STANDARD and `REQUIRED` under HIGH_ASSURANCE. Never economy — a cheap security review is worse than none, because it produces a passing verdict nobody should trust.

## Defensive framing

Tasks are classified defensive where applicable, and capsules are scoped to reduce unnecessary exploit-generation detail. Reviewing for vulnerabilities does not require producing working exploits, and requesting them makes providers refuse legitimate work (`SAFETY_INTERRUPTION_PROTOCOL.md`).

## Failure is never a pass (§77)

If required security review cannot complete — every eligible reviewer unavailable, blocked, or interrupted — the outcome is `HUMAN_REQUIRED`. **Never a false `PASS`.**

Preserved: exact SHA, static findings, dependency findings, test results, prior reviewer findings, and safety-interruption records. The human inherits everything gathered rather than starting over.
