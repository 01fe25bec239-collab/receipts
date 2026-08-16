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

# SCENARIO_VALIDATION

Host session entry follows `evidence/HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`: verify cached host/install capability freshness, re-probe when changed or unknown, then select mode without resetting graph state.

Each scenario: state before → decision → responsible subsystem → persisted data → state after → escalation if any.

---

**S1 — Claude-hosted SPEC with 3 workstreams.**
Before: empty project, Claude Code host. Decision: `START_GOAL` → ClaudeHostAdapter emits `USER_GOAL_SUBMITTED` → Goal Orchestrator persists the goal verbatim → RUNTIME-A1 bound (frontier), rehydrates, decomposes into 3 workstreams → DAG built, cycle-checked → 3 RUNTIME-A2 roles created with branches. Subsystem: HOST-INTEGRATION → ORCHESTRATION → STATE-CONTEXT. Persisted: goal, spec refs, DAG, roles, branches, events. After: wave 1 tasks `READY`.

**S2 — Same from Codex.** Identical below the adapter. CodexHostAdapter's native plugin hooks emit the same `USER_GOAL_SUBMITTED` — this is the normal current case (`selected_mode = EMBEDDED`, registry `C-01`/`C-02`); every downstream step is byte-identical because the core is shared. Validates P-01…P-06. Where discovery instead selects the supervised/hybrid fallback, the companion supervisor emits the same event derived from process output, and the only difference is event provenance (`OBSERVED` vs some `INFERRED`).

**S3 — Claude → Codex continuation.** Before: GOAL-17 mid-flight, Claude unavailable. Decision: user opens Codex; adapter discovers the project; A1 lease expired, rebind; **mandatory rehydration**; resume. Persisted: unchanged state; new binding; `CONTEXT_REHYDRATED` event. After: goal continues, same DAG, same branches. In-flight A3s treated as crashes (S9).

**S4 — Codex → Claude.** Symmetric. Validates P-18 in both directions.

**S5 — New frontier model released after the executor's training cutoff.** Before: model unknown to every executor. Decision: refresh trigger (TTL or provider list change) → `MODEL_DISCOVERED` → `UNASSESSED`. Official capability verified → `CAPABILITY_VERIFIED`. Under `ASK_ON_UNCERTAINTY`, an uncalibrated frontier candidate prompts the user before critical dispatch. Vendor benchmark claims recorded as `UNVERIFIED` and **do not** promote it. Subsystem: MODEL-ROUTING. After: usable via `CALIBRATING` on bounded tasks; no architecture change required. **This is the §32 requirement, satisfied.**

**S6 — A3 rate-limited after A4 rejection.** Before: A3 on provider X implemented; A4 rejected; X now `RATE_LIMITED`. Decision: A2 requests REPAIR at frontier floor; router filters X out; selects another eligible frontier executor; fresh A3 receives a Repair Capsule. Persisted: rejected SHA, findings, capsule, workspace, new routing decision with `fallback_from`. After: repair proceeds. **No work lost, because none of it lived in X's session.**

**S7 — A2 executor hits session limit.** Decision: binding released with reason; role stays ACTIVE; `FRONTIER_FAILOVER` rebinds; mandatory rehydration; resume from persisted next actions. Persisted: binding history. After: same A2, same ownership, branch, decisions, children.

**S8 — A1 executor unavailable.** Identical mechanism at the top level. Lease expiry prevents deadlock; single-active-binding prevents two authoritative A1s. After: project identity and DAG intact.

**S9 — A3 crashes with dirty worktree.** Decision: orphan detected (expired lease, no handoff) → **capture before touching anything** → recovery record. Replacement A3 must choose explicitly; MVP default `RESET_TO_LAST_ACCEPTED`. Persisted: recovery record with dirty diff, modified/untracked files, checks executed. After: fresh attempt; partial work never auto-accepted. Escalation if repeated.

**S10 — Parallel A3 write collision.** Before: two ready tasks with overlapping `allowed_write_paths`. Decision: scheduler detects the intersection at admission and refuses concurrency — serialize (add edge), defer assembly, assign a canonical writer, or re-cut the boundary. Subsystem: ORCHESTRATION. After: one runs; the other waits. **The collision never reaches git.**

**S11 — Economy cheaper per call, more expensive after repairs.** Decision: the estimator computes expected cost to an accepted result using `p(reject)` from calibration; with high `p`, the economy candidate's total exceeds the frontier candidate's. Router selects frontier. Persisted: decision with score components and `decisive_factor`. After: correct choice, explainable. **The §36 requirement, demonstrated.**

**S12 — README task routed to economy.** Decision: task class `ECONOMY_DOCS` under the owning engineering A2 — **no docs manager exists**. Economy executor; LIGHT assurance. After: cheap task, correct ownership. If the doc encoded an architecture decision, it would route `FRONTIER_REASONING` instead.

**S13 — Security A4 enters `SAFETY_CHECK_PENDING`.** Decision: preserve task state; mark the attempt PENDING (not failed); **do not fan out across providers**; wait per policy; resume if the provider does; classify terminally on timeout. Persisted: interruption event, partial evidence. After: continues or escalates. Escalation: `HUMAN_REQUIRED` if terminal.

**S14 — Legitimate defensive audit `POLICY_BLOCKED`.** Decision: preserve the blocked attempt; classify as defensive; **narrow** the capsule to reduce exploit-generation detail; run deterministic tooling; retry another provider only if all five conditions hold — critically, the capsule was narrowed, not merely re-sent. Persisted: block, narrowing, retry, all logged. After: completed defensively or escalated. **An unchanged retry elsewhere is prohibited (I-12).**

**S15 — Every eligible security reviewer unavailable/blocked.** Decision: deterministic tooling only; if insufficient → **`HUMAN_REQUIRED`**, never a false `PASS`. Persisted: exact SHA, all tool findings, prior reviewer findings, interruption records. After: human inherits everything gathered.

**S16 — All frontier coding models rate-limited.** Decision: no eligible candidate at the required floor → WAIT (bounded) → ASK. **Never a silent downgrade** (I-9). Persisted: `ROUTING_FAILED_NO_CANDIDATE`, availability states, `retry_after`. After: resumes when a provider recovers, or the user decides.

**S17 — Context compaction / session replacement.** Decision: `CONTEXT_COMPACTED` (OBSERVED on Claude, INFERRED on Codex) triggers mandatory rehydration; the manifest's digests are recompared; changed `MANDATORY` sources are flagged and decisions made in that window are marked for review. After: executor operating on current authoritative context, not on a host-generated summary.

**S18 — Goal evaluator finds an unmet requirement after apparent completion.** Before: all tasks accepted, all workstreams integrated. Decision: deterministic layer passes; **semantic layer (frontier) finds an original criterion unsatisfied**; state `INCOMPLETE` with a specific gap and its evidence; A1 creates a task; loop continues; final evaluation `COMPLETE`. Persisted: every evaluation record. After: the goal actually met. **This is the difference between "ran out of tasks" and "achieved the goal" (I-14).**

---

**S19 — BUILD dependency inversion (V1.2).**
*Before:* V1.1 recorded reciprocal arrows between `BUILD-A2-ORCHESTRATION`, `BUILD-A2-MODEL-ROUTING`, and `BUILD-A2-REVIEW-INTEGRATION`, producing 2-cycles and the 3-cycle `REVIEW → ORCHESTRATION → MODEL-ROUTING → REVIEW`, while asserting "no cycles".

*Decision:* separate the three relations. Reciprocal arrows were runtime collaborations and contract consumptions, not concrete build dependencies.

*Contracts frozen beforehand (M0):* `TaskCapsule`, `RepairCapsule`, **`ReviewRequest`**, `ReviewCapsule`, `A4Review`, `RoutingRequest`, `RoutingDecision`, `ModelObservation`, `IntegrationRequest`, `IntegrationDecision`.

*Hard dependencies after inversion:*
- `ORCHESTRATION` → `STATE-CONTEXT` only.
- `MODEL-ROUTING` → `STATE-CONTEXT`, `RUNTIME-ADAPTERS` (needs real capability probing; a stub cannot honestly report a runtime's flags).
- `REVIEW-INTEGRATION` → `STATE-CONTEXT`, `WORKSPACE-EXECUTION`, `RUNTIME-ADAPTERS` (needs real SHAs and real execution).

None of the three depends on either of the others. `ORCHESTRATION` lands in W2; `MODEL-ROUTING` and `REVIEW-INTEGRATION` in W3.

*Parallel-safe boundaries:* `ORCHESTRATION` emits `ReviewRequest` — a normative contract **owned by `REVIEW-INTEGRATION`** with a schema under `schemas/` — and consumes `A4Review`. REVIEW-INTEGRATION constructs the `ReviewCapsule` itself, so capsule construction never crosses the boundary. Neither module imports the other. `REVIEW-INTEGRATION` emits `RoutingRequest` and consumes `RoutingDecision`; calibration returns as `ModelObservation` data, not a call back into review.

*Integration test point:* M4. A single end-to-end test drives goal → capsule → dispatch → audit → repair → acceptance across all three, exercising every frozen boundary at once. Until then each uses test doubles for the others.

*After:* `CYCLE_COUNT = 0`, topological order exists, wave-order violations = 0 — all checked programmatically rather than by prose inspection. **The three managers are buildable without cyclic concrete dependency.**


---

## V1.3 scenarios — graph, tiers, entitlement, policy

**S20 — FREE user, Claude host, fresh public install.** Clone → no product account → entitlement resolves `FREE` with **no network call**. Goal submitted → `GraphCompiler` → `ExecutionGraph` v1 → rendered as a tree → FREE policy executes via the one eligible host-native runtime → deterministic checks → graph completes. Pro capabilities visible and `LOCKED_REQUIRES_PRO`. **Entitlement service never contacted.** Subsystems: HOST-INTEGRATION → ORCHESTRATION → WORKSPACE-EXECUTION.

**S21 — FREE user, Codex host.** Identical below the adapter. Host posture selected by `HostCapabilityReport`. **EMBEDDED does not mean merely "hooks discovered"**: it requires `plugin_installed`, `hooks_supported`, `hooks_configured`, `hooks_enabled` and `hooks_allowed_by_admin_policy` all true, `hooks_trusted` true-or-null, and `hook_coverage_class` sufficient — discovery alone (a host that merely *supports* hooks) selects SUPERVISED/HYBRID instead. Observable capability matches Claude exactly; no "secondary client" degradation. Validates P-19…P-21.

**S22 — FREE user invokes a PRO capability.** Request for multi-runtime review → `FeatureAdmission(review.independent_a4)` → `LOCKED_REQUIRES_PRO`. Verified: **provider dispatch count = 0**, **graph corruption = 0**, upgrade info shown, node marked `LOCKED_REQUIRES_PRO` with a reason, **no authoritative state mutated**. The gate is admission control, not a badge.

**S23 — FREE capability discovery.** `SHOW_CAPABILITIES` filtered `ALL` / `FREE` / `PRO`. Every Pro entry carries name, description, requirement and live status. Pro is never hidden from a Free user — nobody buys what they cannot see. Zero model tokens consumed.

**S24 — PRO activation.** `PRODUCT_LOGIN` → device/browser flow → signed entitlement → local verification against public key → `PRO_ACTIVE`. **Same graph**, same `graph_id`, same accepted work. No recompile, no restart, no new project. Pro capabilities unlock on the next admission call.

**S25 — Cross-host entitlement.** Activate Pro in Claude → close Claude → open Codex → same user-local entitlement resolves `PRO_ACTIVE` → same capability set → same project graph at the same version resumes. **No second purchase.**

**S26 — Licence service outage.** FREE: works unchanged, service never needed. PRO with valid cache: `PRO_GRACE` within `offline_grace_until`. PRO with no valid cache: `ENTITLEMENT_UNKNOWN` — **not** silently `FREE` — history readable, Free capability usable, new Pro dispatch refused with a clear reason.

**S27 — Entitlement expiry mid-run.** Running Pro node completes or checkpoints (killing it risks workspace corruption). No **new** Pro-only node starts. Graph readable, accepted evidence intact, status explains expiry, Free capability continues.

**S28 — Policy-disallowed provider.** Provider CLI authenticated, product `PRO_ACTIVE`, provider technically healthy, `policy_status = VERIFIED_DISALLOWED`. Router **must not dispatch**. Returns `PROVIDER_POLICY_DISALLOWED` — **not** `AUTH_REQUIRED`, **not** `RATE_LIMITED`, **not** `LOCKED_REQUIRES_PRO`. The user is told the real reason.

**S29 — PRO with one eligible provider.** Two providers installed, one policy-eligible. Reports `PRO_ACTIVE`, `MULTI_RUNTIME_FEATURE = UNLOCKED`, `CROSS_PROVIDER_REVIEW = UNAVAILABLE_NO_SECOND_ELIGIBLE_RUNTIME`. Same-provider fresh-context review used only if assurance policy allows, labelled `PROVIDER_DIVERSITY = SAME_PROVIDER`. **Never** told to upgrade — they already paid.

**S30 — PRO with two eligible providers.** Goal → graph → RUNTIME-A1/A2 → A3 on provider A → fresh A4 on provider B → REJECT → repair expands the graph → new A3 → new A4 → PASS → integration. Exact-SHA provenance preserved throughout; every rejected SHA retained.

**S31 — Host-specific gate bypass attempt.** A Free user invokes a Pro operation directly through a host-specific path, bypassing the plugin UI. Shared core admission still refuses. Required: **`HOST_GATE_BYPASS = BLOCKED`**. Host UI is never an enforcement boundary.

**S32 — Graph repair without a dependency cycle.** A rejection expands the graph with new attempt and review nodes joined by a **control** edge (`ON_REJECT`), never a precedence edge. Fixture `03_repair_expansion.json` proves precedence cycles = 0 while the conceptual loop is fully represented.

**S33 — Goal Evaluator graph mutation.** All existing nodes accepted → evaluator checks the original spec → finds a missing requirement → records a `GraphMutation` with actor `GOAL_EVALUATOR` and a reason → appends work → `graph_version` increments → completion stays `INCOMPLETE` → work resumes. The plan is never silently rewritten.

**S34 — Native host hook flow.** `host hook → HostAdapter → NormalizedHostEvent → shared core`, demonstrated on whichever host discovery reports native hooks for. Where a host has none, the same event arrives marked `INFERRED`. Parity is tested at normalized semantics, not at the mechanism.

**S35 — Public package secret inspection.** Inspect all public artifacts. Required: provider secrets = 0, licence signing private keys = 0, production service secrets = 0, customer identifiers = 0.

---

## V1.3.1 scenarios — source closure, axis separation, activation provenance

**S36 (A) — `CODEX_NATIVE_PLUGIN_CURRENT`.** Codex with native plugin support. Plugin installs with `.codex-plugin/plugin.json` and `hooks/hooks.json`. Hooks reviewed and trusted by the user. `SessionStart` → `CodexHostAdapter` → `NormalizedHostEvent` → shared core. Compaction arrives via `PreCompact`/`PostCompact` as `OBSERVED`, not inferred. Discovery reports EMBEDDED.

**S37 (B) — `CODEX_OLD_VERSION_FALLBACK`.** Native hooks unavailable, disabled by `[features] hooks = false`, or plugin hooks untrusted. Discovery reports `hooks_supported=false` or `hooks_trusted=false` (`HostCapabilityReport` canonical field names) → SUPERVISED/HYBRID. **Identical normalized event semantics**, some events marked `INFERRED`. Parity rows still pass. The install flow tells the user their hooks are untrusted and points at `/hooks` rather than appearing silently broken.

**S38 (C) — `ANTHROPIC_SUBSCRIPTION_EXTERNAL_WORKER_DENIED`.** `OUR_PRODUCT = PRO_ACTIVE`, Claude CLI authenticated by subscription, `execution_context = THIRD_PARTY_LOCAL_EXTERNAL_WORKER`, policy `VERIFIED_DISALLOWED` (`C-10`). `DispatchAdmissionDecision` → `DENY`, `failing_axis = PROVIDER_POLICY`, `denial_reason = PROVIDER_POLICY_DISALLOWED`. **Required: provider dispatch = 0.** Entitlement axis passes — the user is not told to upgrade. Fixture: `fixtures/admission/C_anthropic_subscription_worker_denied.json`.

**S39 (D) — `OPENAI_SUBSCRIPTION_EXTERNAL_WORKER_UNRESOLVED`.** Same shape, policy `NEEDS_REVIEW`. **Required: provider dispatch = 0 by conservative default.** `denial_reason = PROVIDER_POLICY_UNKNOWN` — distinct from disallowed, because the honest answer is "not established", not "forbidden". Fixture: `D_openai_subscription_worker_unresolved.json`.

**S40 (E) — `OPENAI_USER_API_PROGRAMMATIC_PATH`.** User API key configured, policy `VERIFIED_ALLOWED` (`C-11`), technically healthy, Pro capability admitted. All six axes PASS → `ALLOW`, dispatch proceeds. Fixture: `E_openai_user_api_allowed.json`. **This is the path that makes Pro useful today.**

**S41 (F) — `FRESH_FREE_OFFLINE`.** `activation_state = NEVER_ACTIVATED`, licensing service unavailable, no entitlement. **Required: FREE graph execution continues.** No network call attempted, no `ENTITLEMENT_UNKNOWN`. Fixture: `F_fresh_free_offline.json`.

**S42 (G) — `PREVIOUS_PRO_CACHE_LOST_OFFLINE`.** `activation_state = ACTIVATED_KNOWN`, entitlement missing or corrupt, service unavailable. **Required: `ENTITLEMENT_UNKNOWN`, new Pro dispatch = 0, Free capability and full history remain readable.** Explicitly **not** resolved as FREE. Fixture: `G_previous_pro_cache_lost.json`.

**S43 (H) — `FEATURE_ADMISSION_AXIS_SEPARATION`.** `FeatureAdmissionDecision` is structurally incapable of representing a provider failure, policy failure, or safety failure — its outcome enum contains only `ALLOW`, `LOCKED_REQUIRES_PRO`, `ENTITLEMENT_UNKNOWN`, `ENTITLEMENT_EXPIRED`. Validated as `FEATURE_ADMISSION_PROVIDER_OUTCOMES = 0` and `FEATURE_ADMISSION_SAFETY_OUTCOMES = 0` by schema inspection, not by review.

**S44 (I) — `DISPATCH_ADMISSION_EXACT_REASON`.** For each of the six axes, a composed decision names that axis and no other. A rate limit never surfaces as an entitlement problem; a licence expiry never surfaces as a provider problem.

**S45 (J) — `GRAPH_EDGE_EXCLUSIVITY`.** `PRECEDENCE` + `control_kind` → schema failure. `CONTROL` + `precedence_kind` → schema failure. Also covered: each class missing its required kind. Four negative fixtures under `fixtures/graphs-negative/`. **Required: `INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED = 0`.**

**S46 (K) — `OPENAI_EXTERNAL_CHECKOUT_ENTITLEMENT`.** The Codex plugin directs the user to our external checkout (permitted per `C-13-OPENAI`). Payment completes on our site; our service issues a signed entitlement; the local core verifies it. **Entitlement authority remains ours.** OpenAI neither processes nor bills our subscription.

---

## North-star demo (§96)


S1 → parallel A3s → automatic A4s → one rejection (S6) → provider rate limit → frontier failover → repair passes → integration → manager replacement (S7) → rehydration (S17) → host switch (S3) → safety interruption (S13) → all workstreams integrate → evaluator finds a gap (S18) → new task → final review → **COMPLETE**.

## Coverage

Scenario count is derived programmatically in `PACKAGE_VALIDATION_REPORT.md`. V1.3 adds S20–S35 covering graph semantics, the FREE and PRO tiers, entitlement admission, offline behaviour, and provider policy.

Originally 19 scenarios, each mapping to at least one invariant. Gaps found while walking them: none that invalidate the architecture; three that sharpened it — the S10 admission-time collision check, the S14 five-condition narrowed-retry test, and **S19, which exposed that V1.1's dependency matrix was cyclic while claiming otherwise**. S19 is the reason V1.2 exists.
