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

# OPEN_QUESTIONS

**Classifications:** `BLOCKING_ARCHITECTURE` · `BLOCKING_BEFORE_IMPLEMENTATION` · `BLOCKING_BEFORE_SPECIFIC_MILESTONE` · `BLOCKING_BEFORE_PROVIDER_PATH_ENABLEMENT` · `NONBLOCKING` · `RESOLVED`

The `NONBLOCKING` class exists because §117 is right: a policy ambiguity should not block an entire architecture when the abstract gate safely excludes the ambiguous path.

## Reconciliation rule (V1.3.3)

This page is reconciled against `evidence/SOURCE_CLAIM_REGISTRY.json` and checked by `evidence/validate_package.py`:

- A question is **`RESOLVED`** when the registry holds current evidence answering it. Every claim cited by a `RESOLVED` row must carry a current-evidence label.
- Every unresolved row that depends on evidence must cite at least one claim that is genuinely **not** current-evidence (`POLICY_NEEDS_REVIEW` or `UNVERIFIED`). A question cannot stay open against evidence that exists.
- **`VERIFIED_DISALLOWED` is a resolved policy result, not an open question.** A path being closed is an answer. The path stays disabled; the question does not stay open.
- A question that had one generic vendor-independent form, while the policy matrix records **provider-specific outcomes**, is split per provider. One row per answerable question.
- `N` is the number of questions the row represents. All summary counts are the sum of `N` per classification, derived from these rows.

## Questions

| ID | N | Question | Owner | Classification | Evidence | Disposition / safe fallback |
|---|---:|---|---|---|---|---|
| **Q-V13-01** | 1 | Does Codex support native plugins, `.codex-plugin/plugin.json`, skills and lifecycle hooks? | HOST-INTEGRATION | **`RESOLVED`** | `C-01`, `C-02`, `C-02a`, `C-02b`, `C-02c`, `C-02d` | **YES.** Self-fetched current OpenAI documentation. Native plugin/hooks is the primary Codex path; supervised/hybrid remain capability-discovered fallbacks. No longer a milestone blocker |
| **Q-V13-02** | 1 | Current Claude Code plugin, hook and worktree semantics as used by this architecture | HOST-INTEGRATION | **`RESOLVED`** | `C-04`, `C-05` | **Covered portion resolved.** Plugin composition, `SubagentStart`/`SubagentStop` and `WorktreeCreate` replacement semantics are established by current primary sources. The residual local-behaviour question is **not** this row — it is `Q-02`, named precisely below |
| **Q-V13-03** | 1 | May a paid third-party orchestrator drive a customer's **Claude subscription** as an external worker? | MODEL-ROUTING | **`RESOLVED`** | `C-10` | **NO — `VERIFIED_DISALLOWED`.** Anthropic does not permit third-party developers to route requests through Free/Pro/Max credentials on behalf of users. Resolved answer; path permanently **not routable**. `USER_API` and supported cloud providers are unaffected |
| **Q-V13-04** | 1 | Same question for a **ChatGPT/Codex consumer plan** (third-party paid external worker) | MODEL-ROUTING | **`BLOCKING_BEFORE_PROVIDER_PATH_ENABLEMENT`** | `C-12` (`POLICY_NEEDS_REVIEW`), `C-11a` | **GENUINELY OPEN.** No current source permits it; `C-11a` restricts programmatic extraction generally while Codex documentation supports defined programmatic auth. Path stays `POLICY_NEEDS_REVIEW` and **NOT ROUTABLE BY DEFAULT**. Not promoted on inference |
| **Q-V13-05-OPENAI** | 1 | Is host-native in-plugin execution a distinct, permitted context **for OpenAI**? | MODEL-ROUTING | **`RESOLVED`** | `C-11` | **YES.** `CHATGPT_SUBSCRIPTION` + `HOST_NATIVE_CODEX` = `VERIFIED_ALLOWED`, routable. Sign in with ChatGPT is the documented local path |
| **Q-V13-05-ANTHROPIC** | 1 | Is host-native in-plugin execution a distinct, permitted context **for Anthropic**? | MODEL-ROUTING | **`RESOLVED`** | `C-10` | **YES.** `SUBSCRIPTION_OAUTH` + `HOST_NATIVE_CLAUDE_CODE` = `VERIFIED_ALLOWED`, routable. OAuth is intended for ordinary use inside Claude Code. This is exactly the distinction that makes `Q-V13-03` disallowed and this row allowed |
| **Q-V13-06** | 1 | Pro module distribution mechanism | BUILD-A1 | `BLOCKING_BEFORE_IMPLEMENTATION` | — | Packaging feasibility test before first paid release. Fallback: signed authenticated download post-activation |
| **Q-V13-07** | 1 | Licence seat/device semantics | BUILD-A1 | `NONBLOCKING` | — | Business decision before pricing launch. Per-subject entitlement; optional opaque device binding |
| **Q-V13-08** | 1 | Offline grace duration | BUILD-A1 | `NONBLOCKING` | — | Business policy. Field exists and is signed; the value is configuration |
| **Q-V13-09-OPENAI** | 1 | Does the OpenAI marketplace permit paid/licensed third-party plugins or external checkout? | HOST-INTEGRATION | **`RESOLVED`** | `C-13-OPENAI` | **YES, via External Checkout.** Developer operates independently and may direct users to a developer-controlled site for payment. Entitlement remains ours |
| **Q-V13-09-ANTHROPIC** | 1 | Does Anthropic offer first-party paid-plugin checkout? | HOST-INTEGRATION | `NONBLOCKING` | `C-13-ANTHROPIC` (`UNVERIFIED`) | **NOT ESTABLISHED.** No evidence found; absence of evidence is not evidence of permanent absence. Distribution itself is established (`C-13-ANTHROPIC-DIST`). Entitlement is independent of marketplace billing, so nothing blocks |
| **Q-V13-10** | 1 | Graph-engineering prior art and what is already commoditised | BUILD-A1 | **`RESOLVED`** | `C-14` | **Research closed.** Public prior art exists covering host-neutral DAG execution, caching, quality gates, selective retry, resume and zero-token rendering. Differentiation is stated against this record, never as an unexamined claim |
| **Q-V13-11** | 1 | Is `GraphRun` needed as a distinct entity? | ORCHESTRATION | `NONBLOCKING` | — | Rejected for now; version + node state + results suffice. Revisit after MVP on implementation experience |
| **Q-V13-12** | 1 | Does the on-demand core model meet latency needs? | ORCHESTRATION | `NONBLOCKING` | — | Measured after MVP. Hybrid (Option C) upgrade path, identical interface |
| Q-01 | 1 | Host command syntax | HOST-INTEGRATION | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` | — | **Reopened** (§60). Namespaced command; never hijack a built-in. Before M3 |
| Q-02 | 1 | Does `WorktreeRemove` displace Claude Code's default worktree cleanup? | HOST-INTEGRATION | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` | — | **Local smoke test only.** `C-05` documents `WorktreeCreate` replacement; it does not state removal behaviour. This is the precisely named residual of `Q-V13-02`. Fallback: do not install the handler; lazy invalidation |
| Q-03 | 1 | MVP state store | STATE-CONTEXT | **`RESOLVED`** | `C-20` | SQLite, including graph tables. Frozen in `STATE_AND_CHECKPOINT_ARCHITECTURE.md` |
| Q-06 | 1 | Deterministic security tool set | REVIEW-INTEGRATION | `BLOCKING_BEFORE_SPECIFIC_MILESTONE` | — | Per-project config model before security review work. Model-only review is explicitly labelled as such |
| Q-07 | 1 | Safety-state detectability | REVIEW-INTEGRATION | `NONBLOCKING` | — | Provider signal if one exists. `UNKNOWN` + `HUMAN_REQUIRED`; never a false PASS |
| Q-04, Q-05, Q-08…Q-12 | 7 | Carried forward unchanged from V1.2.3 | various | `NONBLOCKING` | — | Bundle row. Represents 7 questions, none of which touch the graph core, entitlement, provider policy or host posture |

## Blocking summary

Derived by summing `N` per classification over the rows above. Checked by `evidence/validate_package.py` as `OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES`.

```
BLOCKING_ARCHITECTURE                    = 0
BLOCKING_BEFORE_IMPLEMENTATION           = 1    (Q-V13-06)
BLOCKING_BEFORE_PROVIDER_PATH_ENABLEMENT = 1    (Q-V13-04)
BLOCKING_BEFORE_SPECIFIC_MILESTONE       = 3    (Q-01, Q-02, Q-06)
NONBLOCKING                              = 13
RESOLVED                                 = 8
TOTAL                                    = 26
```

## What changed from V1.3.2

Eight questions moved to `RESOLVED` because the evidence answering them is already in the registry and was simply never reflected here. Two generic questions were split because the policy matrix records **provider-specific** outcomes and a single vendor-independent answer would have been false for one provider or the other:

| Was | Now | Why |
|---|---|---|
| `Q-V13-01` open, blocking Codex plugin work | `RESOLVED` | `C-01`/`C-02` are `VERIFIED_CURRENT_SELF_FETCHED`. Leaving it open contradicted the package's own host posture |
| `Q-V13-02` open | `RESOLVED`, residual named as `Q-02` | `C-04`/`C-05` cover the semantics actually used. Only local removal behaviour remains untested |
| `Q-V13-03` open policy question | `RESOLVED` — `VERIFIED_DISALLOWED` | Disallowed is an **answer**. The path stays disabled either way, but the question is closed |
| `Q-V13-05` one generic question | `Q-V13-05-OPENAI` + `Q-V13-05-ANTHROPIC`, both `RESOLVED` | The matrix already records host-native as `VERIFIED_ALLOWED` for both providers. One generic row hid two established outcomes |
| `Q-V13-09` one generic question | `Q-V13-09-OPENAI` `RESOLVED` + `Q-V13-09-ANTHROPIC` open | External Checkout is evidenced; Anthropic first-party checkout is not. Merging them understated one and overstated the other |
| `Q-V13-10` open | `RESOLVED` | `C-14` closed the research |

**Deliberately unchanged:** `Q-V13-04` stays open at `POLICY_NEEDS_REVIEW`, not routable by default. Nothing in this pass promoted it, and no other resolution was allowed to drag it along.

**No question blocks the architecture.** The two structural unknowns are neutralised by design: host posture is discovered rather than assumed, and provider policy is a conservative gate rather than an assumption. The product functions with every subscription path disabled.

`FREEZE_READY = PENDING_FINAL_INDEPENDENT_REVIEW` regardless — §133 requires independent review for a reopen of this size.
