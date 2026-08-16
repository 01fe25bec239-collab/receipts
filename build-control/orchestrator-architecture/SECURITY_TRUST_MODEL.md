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

# SECURITY_TRUST_MODEL

## Threat framing

The system runs semi-autonomous agents with filesystem, shell, git, and network access, using the developer's paid provider credentials, against their real repository. The realistic threats are **capability misuse, prompt injection, credential exposure, and state corruption** — not primarily malicious agents.

## Trust boundaries

```
┌─ USER (highest authority) ─────────────────────────────┐
│  ┌─ ORCHESTRATOR CORE ─────────────────────────────┐   │
│  │  state store · routing · gates · roles          │   │
│  │  ┌─ HOST ADAPTERS ──────────────────────────┐   │   │
│  │  │  ┌─ RUNTIME ADAPTERS ──────────────┐     │   │   │
│  │  │  │  ┌─ WORKER AGENTS (lowest) ─┐   │     │   │   │
│  │  │  │  │  A3 / A4 sessions        │   │     │   │   │
│  │  │  │  └──────────────────────────┘   │     │   │   │
│  │  │  └─────────────────────────────────┘     │   │   │
│  │  └──────────────────────────────────────────┘   │   │
│  └─────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────┘
```

Authority decreases inward. **Worker agents have the least authority in the system** despite doing the most work — which is the correct arrangement, and the one every boundary below enforces.

## Boundaries (§92)

| Boundary | Control |
|---|---|
| Provider credentials | Delegated to provider tooling or OS keychain; never in the repo; redacted from every log, event, capsule, and error |
| User subscription access | Mode-separated; team mode cannot register subscription connections |
| Orchestration state DB | Core-only write path. **No worker, host adapter, or LLM output may mutate it** (I-18) |
| Provider adapters | Fixed interface; adapters cannot alter routing policy, gates, or state |
| Worker filesystem | Sandbox where available (A-07, A-12) + worktree + post-hoc diff verification |
| Shell | Explicit argv, never shell strings; allowlisted environment; orchestrator-owned timeouts |
| Git | Workers commit only on their own task branch; merges and `main` are core-only |
| Network | Off by default in worker sandboxes where the runtime supports it |
| Worktrees | Isolation only — **never described as a sandbox** (I-11) |
| Reviewers | Read-only execution policy; cannot modify what they review |
| Integration authority | Gate is core code, not an agent decision |
| Prompt injection | Repository content and agent output are untrusted data; never merged into instruction voice; never reach an argv or path field |
| Host bridge | Adapters translate; they carry no orchestration logic |
| Arbitrary provider output | Schema-validated; unparseable output is a failure, never a silent default |

## Prompt injection specifically

A repository may contain adversarial text — in a README, a test fixture, a dependency. That text reaches models. Controls: untrusted content is quoted as data with explicit provenance; instructions in repository content are never followed as system direction; no model output reaches a command, path, or state mutation without validation; and capability boundaries are enforced by sandbox and diff verification rather than by asking the model to behave.

Prompt-level defences are supplementary. **Hard boundaries use stronger mechanisms wherever they exist** — an instruction is not a control.

## Renderer restriction (I-13)

The economy renderer receives structured state and emits prose. It has **no write path** to test results, verdicts, SHAs, security status, integration decisions, or goal state. A cheap model cannot alter what the expensive machinery established.

## Overclaim prohibitions

Never claim: worktrees are sandboxes; the system proves correctness; a model-only security review is a full audit; quota state is known when it is `UNKNOWN`; or a subscription authorises programmatic multi-user use.

Each of these would be an easy, plausible sentence to write in a README — which is exactly why they are enumerated here.

## Licensing never downgrades safety (V1.3)

Four orthogonal concerns that must never trade against each other:

```
PRODUCT_ENTITLEMENT   ·   PROVIDER_POLICY   ·   MODEL_SAFETY   ·   SECURITY_REVIEW
```

**A Pro customer receives no safety bypass advantage.** No paid tier may convert `POLICY_BLOCKED` into `ALLOW`, weaken a safety gate, or relax the safety-bypass prohibition. Money buys orchestration scale, never permission.

**Not paywalled, ever:** credential protection · secret redaction · Git write-boundary enforcement · accurate PASS/FAIL reporting · state integrity · safe recovery · baseline sandboxing · safety-policy enforcement · cycle validation · crash-safe checkpointing.

Selling the absence of deliberately unsafe behaviour would poison the product's entire premise, which is that its evidence can be trusted.

## Entitlement is not a security boundary against the user

The signed entitlement establishes what the user has **paid for**, not what they are **permitted to do safely**. A user who defeats entitlement gains Pro orchestration; they do not gain a safety bypass, because safety gates are independent and apply identically in both tiers.
