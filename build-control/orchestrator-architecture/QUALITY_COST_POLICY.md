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

# QUALITY_COST_POLICY

## Task classes

| Class | Floor | Typical work |
|---|---|---|
| `FRONTIER_IMPLEMENTATION` | FRONTIER | Production code, complex debugging, migrations, high-impact refactors |
| `FRONTIER_REVIEW` | FRONTIER | Independent A4 audit |
| `FRONTIER_ARCHITECTURE` | FRONTIER | Decomposition, interface design, architecture decisions |
| `SECURITY_CRITICAL_CODE` | FRONTIER | Security-sensitive implementation and security review |
| `BALANCED_REASONING` | BALANCED | Routine refactors, well-specified low-risk code, triage |
| `ECONOMY_DOCS` | ECONOMY | README, changelog, non-architectural documentation |
| `ECONOMY_SUMMARY` | ECONOMY | Summaries of already-authoritative state |
| `ECONOMY_STATUS` | ECONOMY | Status messages (prefer a template with no model at all) |
| `PRESENTATION_ONLY` | ECONOMY | Final prose rendering after engineering facts are frozen |

## Frontier floor (§41, I-9)

Frontier-floor work is **never silently downgraded**. When no eligible frontier candidate exists:

```
preferred frontier → unavailable
  → next eligible frontier → unavailable
    → next eligible frontier → unavailable
      → policy: WAIT | BLOCK | ASK | HUMAN_REQUIRED
```

| Policy | Behaviour |
|---|---|
| `WAIT` | Hold until `retry_after` or a candidate recovers; task stays scheduled |
| `BLOCK` | Mark BLOCKED with the reason; continue other work |
| `ASK` | Ask the user (accept a lower tier, wait, or pin an alternative) |
| `HUMAN_REQUIRED` | For security-critical work where no eligible reviewer exists |

Default: `WAIT` with a bounded timeout, then `ASK`. Quietly using a weak fast model produces work that fails audit and costs more overall — the outcome this policy exists to prevent.

## Economy usage (§43)

Deliberate for low-risk work: README formatting, changelogs, summaries, repetitive documentation, status rendering, metadata, simple structured transformations, final prose after decisions are frozen.

An economy task must satisfy **all** of: no production code path; no architecture or security decision; deterministically checkable or trivially reviewable; failure is cheap and visible.

Documentation encoding an architecture or security decision routes at `FRONTIER_REASONING` — the reasoning is the artifact, not the prose.

## Final response rendering (§44)

```
authoritative completion record  →  economy renderer or template  →  user-facing text
```

The renderer may present test results, A4 verdicts, SHAs, security status, integration decisions, and goal state. It may **never** alter them (I-13). It receives structured state and emits prose; it has no write path to the state store.

For simple status, a deterministic zero-model template is preferred. Spending a model call to say "3 tasks running" is waste.

## Cost priorities

`quality_priority` and `cost_priority` (HIGHEST/HIGH/SECONDARY/LOW) tune ranking **within** the eligible set. They never move the floor. Security-critical work is `quality_priority: HIGHEST`, `cost_priority: LOW` by default.

## Budget interaction

Budget exhaustion never causes a silent downgrade. It causes `BLOCKED` with a stated reason and a resume path — the honest failure, not the cheap one.
