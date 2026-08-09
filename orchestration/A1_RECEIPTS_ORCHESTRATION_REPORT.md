# A1-RECEIPTS — Build Orchestration Report

**Date:** 9 August 2026 (revised same day for ADR-001 reconciliation)  
**Authority:** `Receipts_Final_Architecture(1).md`  
**Instruction:** `Pasted text(20260809-041820).txt`  
**Architecture correction of record:** ADR-001 — APPROVED 2026-08-09

> **Revision note.** This report was first produced at A1 bootstrap, when no architecture deviation had been identified. `ARCHITECTURE_DEVIATION_REQUEST-001` was subsequently discovered during contract freeze and approved by the architecture authority on 2026-08-09. Receipts does not install a `WorktreeCreate` hook, does not own worktree creation, does not replace Claude Code's default Git worktree behavior, and binds workspace identity observationally from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations. `WorktreeRemove` is also omitted from the MVP installed hook set after current-documentation verification, not retained for symmetry. No other architecture semantics changed. All affected statements below have been corrected.

# 1. SYSTEM UNDERSTANDING

Receipts is not the multi-agent system that builds it. The build uses A1→A2→A3→A4, but the product itself remains a claim/evidence/policy/admission layer around Claude Code. Agents can assert; the broker produces/captures evidence; deterministic and review evidence stay separate; evidence is bound to an exact code state; policy derives admission; Claude hooks enforce only Claude-mediated gates; human override is explicit and non-proof.

The exact MVP is one repo / one machine / one demo ecosystem, with `IMPLEMENTED`, `TESTED`, `LINT_CLEAN`, and `REVIEWED`; a short-lived CLI broker; SQLite; whole-tree staleness; Codex + Claude-session review; Claude L1/L2 gates; integrity signals; human override; ledger verification; and export.

# 2. ARCHITECTURE INVARIANTS

1. Agents may ASSERT claims but cannot prove their own claims.
2. Every evidence item is bound to exactly one CodeStateFingerprint.
3. Evidence is valid only while its fingerprint matches current state and its recipe/schema compatibility remains valid.
4. Deterministic evidence and model-review evidence are different families and cannot substitute for each other.
5. Admission is derived from policy + claim statuses + current fingerprint; stored admissions are audit artifacts, not truth.
6. Worker agents cannot write to the evidence ledger; the broker is the sole writer.
7. Verification commands come only from approved VerificationRecipes; agent-supplied commands are never execution authority.
8. Human override is always available, human-controlled, fingerprint-scoped, and permanently recorded.
9. `ADMITTED_WITH_OVERRIDE` must never be rendered as `ADMITTED`, `VERIFIED`, or `PROVED`.
10. Receipts governs Claude-Code-mediated actions only; it does not claim to stop a human using another terminal.
11. Model/provider identity is configuration, not architecture.
12. Git worktrees are workspace isolation, not security isolation.
13. MVP topology is Claude Code hooks → short-lived receipts CLI → SQLite. No daemon in MVP.
14. Receipts must not expand into a generic multi-agent orchestrator or resurrect an AgentAdapter/FAM architecture.
15. `ReviewProvider` remains deliberately small and cannot become a general agent runtime.
16. MCP is not introduced unless an existing hooks/skills/CLI boundary cannot satisfy a concrete requirement.
17. Any architecture change caused by mutable external capabilities requires an explicit ARCHITECTURE_DEVIATION_REQUEST and approval.

# 3. COMPONENT MAP

| A2 | Decision | Owns | Excludes |
|---|---|---|---|
| A2-CORE | KEEP | Repository identity; CodeStateFingerprint; task/claim domain model; claim status derivation; whole-tree staleness; VerificationPolicy semantics; pure `admit()`; changed-path cause calculation; git adapter semantics needed by fingerprinting. | SQLite/event persistence; subprocess runner; provider CLIs; hook packaging; override UI; benchmark harness. |
| A2-LEDGER | KEEP | Append-only LedgerEvent spine; canonical event serialization; hash chain; SQLite schema and WAL settings; projections/rebuild; transaction boundaries; `verify-ledger`; export mechanics and hash verification. | Domain admission rules; recipe execution; review-provider selection; hook behavior. |
| A2-RUNNER | KEEP | VerificationRecipe schema/semantics; human approval state; recipeDigest; safe argv process execution; cwd/executable resolution; ExecutionReceipt; raw logs; timeout handling; per-(repoId, recipeKey) lock/concurrency; flaky consecutive-run signal storage. | Admission policy; reviewer model calls; Claude hooks; override semantics. |
| A2-CLAUDE-INTEGRATION | KEEP | Plugin manifest; hooks.json; hook normalization; hook/CLI decision adapters; skills; custom reviewer-agent packaging; permission configuration; L1/L2 Claude gates; factual additionalContext; status rendering. | Core policy decisions; ledger storage internals; provider semantics; benchmark oracles. |
| A2-REVIEW | KEEP | ReviewRequest/ReviewResult/ReviewFinding; ReviewProvider interface; Codex provider; Claude-session fallback; provider configuration; health/capabilities; structured parsing; read-only enforcement; provider selection facts and downgrade production. | Admission's final policy decision; deterministic proof; ledger persistence internals. |
| A2-INTEGRITY-SECURITY | KEEP | Trust-boundary requirements; test-change integrity signals; command/path safety requirements; ledger-path protection requirements; recipe/policy protection requirements; override/waiver semantics; prompt-injection-safe context rules; security tests; enforcement-scope audit. | Hook packaging details; SQLite implementation; provider implementation; benchmark execution. |
| A2-EVALUATION | KEEP | 12 benchmark tasks and oracles; arms A–E (F optional); reproducible reset fixtures; repeated-run harness; metrics; raw results; result-integrity checks; no-significance guard. | Product implementation; release prose beyond measured evaluation outputs. |
| A2-DOCS-RELEASE | KEEP | README; architecture/trust/enforcement/provider/hook docs; install/demo docs; name-collision evidence; release checklist; evaluation-report publication from measured M6 outputs only. | Inventing benchmark results; changing architecture; implementing runtime logic. |

**Decision:** no manager is merged, split, or renamed.

# 4. MILESTONE → COMPONENT MAP

| Milestone | Scope | Owner | Contributors | Contracts | Depends on |
|---|---|---|---|---|---|
| M0 | Fingerprint + ledger spine | A2-CORE | A2-LEDGER, A2-INTEGRITY-SECURITY | CORE-001; LEDGER-001; STORAGE-001; ERROR-001 | None; architecture authority only |
| M1 | Recipes + runner + receipts | A2-RUNNER | A2-CORE, A2-LEDGER, A2-INTEGRITY-SECURITY | CORE-001; RUNNER-001; RUNNER-002; EVIDENCE-001; PROCESS-001; LEDGER-001 | M0 |
| M2 | Claims + admit() | A2-CORE | A2-LEDGER, A2-RUNNER, A2-INTEGRITY-SECURITY | CORE-002; CORE-003; EVIDENCE-001; POLICY-001; ADMISSION-001; OVERRIDE-001 | M0 + M1 |
| M3 | Claude Code integration L1/L2 | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY, A2-LEDGER | HOOKS-001; HOOKS-002; ADMISSION-001; CORE-002; ERROR-001 | M2 |
| M4 | Review providers | A2-REVIEW | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY | REVIEW-001; REVIEW-002; ADMISSION-001; PROCESS-001; LEDGER-001 | M3 |
| M5 | Integrity signals + override | A2-INTEGRITY-SECURITY | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-RUNNER, A2-REVIEW | OVERRIDE-001; EXPORT-001; ADMISSION-001; EVIDENCE-001; HOOKS-002 | M4 |
| M6 | Evaluation harness | A2-EVALUATION | All product A2s | All product contracts frozen | M5 |
| M7 | Documentation + release evidence | A2-DOCS-RELEASE | All A2s, especially A2-EVALUATION | ARCH authority; EXPORT-001; all public schemas/contracts | M6 |

# 5. CONTRACT MAP

| Contract | Subject | Owner | Consumers | State |
|---|---|---|---|---|
| CONTRACT-CORE-001 | RepositoryIdentity + CodeStateFingerprint | A2-CORE | A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-CLAUDE-INTEGRATION | FROZEN-SEMANTICS |
| CONTRACT-CORE-002 | Task + TaskState + AgentIdentity | A2-CORE | A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-EVALUATION | FROZEN-SEMANTICS |
| CONTRACT-CORE-003 | Claim + ClaimKind + ClaimStatus | A2-CORE | A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-CLAUDE-INTEGRATION | FROZEN-SEMANTICS |
| CONTRACT-EVIDENCE-001 | Evidence envelope + CodeEvidence | A2-CORE | A2-LEDGER, A2-RUNNER, A2-REVIEW, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |
| CONTRACT-RUNNER-001 | VerificationRecipe + approval + recipeDigest | A2-RUNNER | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |
| CONTRACT-RUNNER-002 | ExecutionReceipt | A2-RUNNER | A2-CORE, A2-LEDGER, A2-EVALUATION | FROZEN-SEMANTICS |
| CONTRACT-POLICY-001 | VerificationPolicy + profile resolution | A2-CORE | A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-INTEGRITY-SECURITY, A2-EVALUATION | FROZEN-SEMANTICS |
| CONTRACT-ADMISSION-001 | Admission + AdmissionDecision + downgrade representation | A2-CORE | A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-REVIEW, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |
| CONTRACT-OVERRIDE-001 | Override + Waiver | A2-INTEGRITY-SECURITY | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-EVALUATION | FROZEN-SEMANTICS |
| CONTRACT-REVIEW-001 | ReviewRequest + ReviewResult + ReviewFinding | A2-REVIEW | A2-CORE, A2-LEDGER, A2-CLAUDE-INTEGRATION, A2-EVALUATION | FROZEN-SEMANTICS |
| CONTRACT-REVIEW-002 | ReviewProvider interface + ProviderResolution | A2-REVIEW | A2-CORE, A2-CLAUDE-INTEGRATION | FROZEN-SEMANTICS |
| CONTRACT-LEDGER-001 | LedgerEvent + canonical serialization + hash chain | A2-LEDGER | All components that append events or consume export | FROZEN-SEMANTICS |
| CONTRACT-STORAGE-001 | Storage transaction boundary + projection rebuild | A2-LEDGER | A2-CORE, A2-RUNNER, A2-REVIEW, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |
| CONTRACT-HOOKS-001 | HookInputNormalization | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS; external fixtures current-doc-bound |
| CONTRACT-HOOKS-002 | HookDecision + CLI exit-code contract | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS; exact hook JSON current-doc-bound |
| CONTRACT-ERROR-001 | Typed error model | A2-CORE | All components | FROZEN-SEMANTICS |
| CONTRACT-EXPORT-001 | Portable export bundle + independent verification | A2-LEDGER | A2-INTEGRITY-SECURITY, A2-EVALUATION, A2-DOCS-RELEASE | FROZEN-SEMANTICS |
| CONTRACT-PROCESS-001 | Process execution safety: argv/cwd/env/timeout/cancel/output | A2-RUNNER | A2-REVIEW, A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |

Full frozen semantics are in `04_CONTRACT_INDEX.md`.

# 6. DEPENDENCY DAG

```text
Architecture
  ↓
M0 CORE + LEDGER
  ↓
M1 RUNNER
  ↓
M2 CLAIMS/POLICY/ADMISSION
  ↓
M3 CLAUDE INTEGRATION
  ↓
M4 REVIEW PROVIDERS
  ↓
M5 INTEGRITY + OVERRIDE + EXPORT
  ↓
M6 EVALUATION
  ↓
M7 DOCS/RELEASE
```

A2 initialization can occur in parallel. A3 implementation cannot bypass milestone prerequisites.

# 7. CONFIGURATION SURFACE MAP

| Surface | Owner | Consumers | Authority | Mutability | Agent edit | Human approval | Validation | Tests |
|---|---|---|---|---|---|---|---|---|
| .claude-plugin/plugin.json | A2-CLAUDE-INTEGRATION | Claude Code plugin loader | Claude official plugin manifest schema | Identity/version metadata mutable by release owner; component topology frozen | Implementation A3 only in owned task; runtime supervised agents: no | Release-owner approval | JSON parse + Claude plugin load smoke | Plugin load contract test |
| hooks/hooks.json | A2-CLAUDE-INTEGRATION | Claude Code hook engine | Claude hooks schema + CONTRACT-HOOKS-002 | Event set frozen by architecture as corrected by ADR-001 — `WorktreeCreate` and `WorktreeRemove` MUST NOT appear; timeouts/matcher syntax current-doc-bound | Implementation A3 yes; runtime agents no direct authority | A2 + A1 integration approval | Schema/fixture validation against documented inputs | Hook fixture + live smoke |
| skills/*/SKILL.md | A2-CLAUDE-INTEGRATION | Human/Claude slash-skill surface | Claude skill frontmatter | Five MVP skills frozen; wording mutable | Implementation A3 yes | A2 approval; override skill requires security sign-off | Frontmatter parse; invocation smoke | Per-skill contract tests |
| agents/receipts-reviewer.md | A2-CLAUDE-INTEGRATION with A2-REVIEW | Claude-session fallback | Claude custom-agent frontmatter + REVIEW-002 | Read-only tool list frozen; model/config remains provider config | Implementation A3 yes | A2-REVIEW + A2-INTEGRITY-SECURITY approval | Frontmatter load; forbidden-write test | Provider fixture + live read-only smoke |
| .receipts/recipes.yaml | A2-RUNNER | Runner; Core staleness; Integrity signals | recipe.schema.json / RUNNER-001 | Human-editable config; every approved entry digest-tracked | Runtime worker agents: DENY | Human approval required for create/change | JSON Schema + semantic executable/cwd validation + approval digest | Recipe approval and invalidation tests |
| .receipts/policy.yaml | A2-CORE | Admission engine; Review; Integrity; UI | policy.schema.json / POLICY-001 | Human-editable; requiredClaims frozen at task OPEN; policy changes evented | Runtime worker agents: DENY | Human approval required | JSON Schema + strictest-path resolution validation | Policy table/property tests |
| .receipts/providers.yaml | A2-REVIEW | Review-provider resolver | REVIEW-002 + provider config schema | Human-editable provider/model/binary settings | Runtime agents should not mutate automatically | Human approval recommended because binary/model path affects review authority | Schema + provider health check | Provider selection/downgrade tests |
| schemas/recipe.schema.json | A2-RUNNER | recipes.yaml tooling | RUNNER-001 | Frozen per contract version | Implementation A3 only | Contract-owner + A1 | JSON Schema self-parse + fixtures | Positive/negative fixtures |
| schemas/policy.schema.json | A2-CORE | policy.yaml tooling | POLICY-001 | Frozen per contract version | Implementation A3 only | Contract-owner + A1 | JSON Schema self-parse + fixtures | Positive/negative fixtures |
| schemas/receipt.schema.json | A2-RUNNER | Ledger/export/evaluation | RUNNER-002 | Frozen MVP fields | Implementation A3 only | Contract-owner + A1 | Schema fixtures | Round-trip receipt tests |
| schemas/finding.schema.json | A2-REVIEW | Codex --output-schema; Claude structured output; ledger | REVIEW-001 | Frozen finding fields/severity enum | Implementation A3 only | A2-REVIEW + A1 | Schema fixtures + provider parse | Malformed/complete provider fixtures |
| settings.json (plugin root default settings, if used) | A2-CLAUDE-INTEGRATION with A2-INTEGRITY-SECURITY | Claude permission system | Current Claude plugin/settings docs + security requirements | Permission intent frozen; exact dynamic-path rules current-doc-bound | Implementation A3 yes; runtime worker cannot weaken | Security approval required | Claude settings parse + deny rule tests | Negative permission tests |
| CLI command surface (`bin/receipts`) | A2-CLAUDE-INTEGRATION facade; domain owners behind it | Hooks, skills, humans | HOOKS-002 + domain contracts | Command names frozen for MVP; flags may be implementation-bound but cannot change semantics | Implementation A3 by bounded task | A1 integration approval | CLI parser/exit contract | Black-box command tests |
| Export JSON bundle | A2-LEDGER | Humans, evaluation, future CI L4 | EXPORT-001 | Versioned and append-only compatible | Generated only by broker/exporter | No manual mutation accepted as evidence | Independent verifier | Tamper and round-trip tests |

# 8. PROCESS / SHELL SURFACE MAP

| Process | Owner | Argv shape | cwd | Environment | Timeout | Exit semantics | Output | Injection boundary | Cancellation | Fixtures |
|---|---|---|---|---|---|---|---|---|---|---|
| git rev-list | A2-CORE | git rev-list --max-parents=0 HEAD | repo worktree root | minimal inherited env; no secrets required | short bounded | 0 => root SHA(s); nonzero => fallback repository UUID path | capture stdout/stderr bounded | Explicit argv; no shell | terminate child; typed error | temporary git repos incl. shallow/unusual history |
| git rev-parse | A2-CORE | git rev-parse HEAD | repo worktree root | minimal inherited env | short bounded | 0 => head SHA; nonzero => repository-state error | capture bounded | Explicit argv | terminate child | detached HEAD and normal branch fixtures |
| git ls-files | A2-CORE | git ls-files -s | repo worktree root | minimal inherited env | short bounded | 0 => staged tracked index tuples | stream/parse; do not log full repo unnecessarily | Explicit argv | terminate child | tracked/staged/mode fixtures |
| git status | A2-CORE | git status --porcelain=v1 -z | repo worktree root | minimal inherited env | short bounded | 0 => dirty/untracked set | NUL-safe parse | Explicit argv | terminate child | spaces/newlines/symlink/untracked ignored fixtures |
| git diff / changed-path evidence | A2-CORE + A2-INTEGRITY-SECURITY | Exact argv frozen by A2-CORE; no shell; baselineSha supplied as one argv element | repo worktree root | minimal inherited env | short bounded | 0 => diff/path metadata; nonzero typed git error | store diff separately under broker data; digest reference in ledger | Validate baseline SHA; explicit argv | terminate child | rename/delete/test-file fixtures |
| VerificationRecipe subprocess | A2-RUNNER | `[resolvedExecutable, ...approvedRecipe.argv]` exactly; never shell string | absolute realpath-approved cwd | allowlisted env only; MVP field recording follows architecture subset | recipe timeout | exit 0 positive receipt; nonzero negative receipt; timeout recorded | raw stdout/stderr gzip + digest + bounded excerpts | Human-approved recipe only; resolved executable; no shell | TERM then bounded KILL strategy; receipt records timeout | fake executables + real demo ecosystem fixtures |
| Codex review provider | A2-REVIEW | codex exec --sandbox read-only --ignore-user-config --ignore-rules --json --output-schema <finding-schema> -o <out> -C <repo> <review-prompt> | repo worktree root / explicit -C | no repository secrets injected; use existing Codex auth; provider config selects optional model | broker-enforced | 0 plus parseOk required; any failure/timeout/malformed => ReviewClaim UNPROVEN | JSONL stdout + final output file; raw stored; structured parse into ReviewResult | Explicit argv; never --full-auto; read-only sandbox | terminate provider; ReviewResult TIMEOUT/FAILED | fake CLI fixtures + authenticated live smoke |
| Claude-session review fallback | A2-REVIEW | TO BE FROZEN BY A2-REVIEW from verified `claude -p`, `--output-format json`, `--json-schema`, and explicit read-only agent/tool constraints | repo worktree root | must preserve intended local auth without loading an authority path that can recurse into Receipts hooks | broker-enforced | nonzero/malformed/timeout => ReviewClaim UNPROVEN | structured JSON envelope; raw stored | No write tools; no shared conversation; prevent hook recursion | terminate provider | fake CLI + authenticated local smoke; this is A3-blocking until invocation is frozen |
| Claude hook → receipts CLI | A2-CLAUDE-INTEGRATION | <receipts-executable> <hook-operation> ...event-derived safe args only | hook input cwd after normalization | CLAUDE_PLUGIN_ROOT/DATA read from environment; no untrusted shell expansion | hook-specific; <= documented budgets | Hook contract decides allow/block; no worktree hook exists to fail (ADR-001) | hook-facing output bounded <10,000 chars | Exec form command+args; event JSON parsed from stdin | kill on timeout; fail behavior event-specific | recorded hook JSON fixtures + live plugin smoke |
| Broker workspace discovery (read-only) | A2-CORE with A2-CLAUDE-INTEGRATION | read-only git worktree/toplevel metadata queries, explicit argv, no shell | invocation cwd, realpath-resolved | minimal inherited env | short bounded | 0 => workspace identity bound; nonzero => unbound, retry at next invocation | capture bounded | Explicit argv; no shell; no write | terminate child; typed error | worktree/non-worktree/removed-worktree fixtures |

# 9. MCP DECISION

**MCP NOT REQUIRED FOR MVP.**

A local MCP server supplies no missing capability. Claude hooks already provide lifecycle observation and blocking, skills provide the command UX, and the short-lived CLI is the intended single broker authority. Adding MCP would create another model-invoked authority path and broaden the trust surface.

# 10. TEST OWNERSHIP MAP

| Layer | Owner | Scope |
|---|---|---|
| Unit | Each owning A2; A2-CORE owns cross-domain pure units | Pure functions and component-local behavior. |
| Contract | Contract owner A2; A1 is gate authority | Serialization, enums, field requirements, producer/consumer compatibility. |
| Property / invariant | A2-CORE | Fingerprint/staleness/admission invariants; override non-upgrade property with security contribution. |
| Storage | A2-LEDGER | WAL, transaction, append-only event spine, projection rebuild, hash chain. |
| Git fixture | A2-CORE | Repository identity, tracked/staged/dirty/untracked/ignored/revert behavior. |
| Hook | A2-CLAUDE-INTEGRATION | All installed hook inputs, decisions, and limits, plus negative tests asserting no `WorktreeCreate` or `WorktreeRemove` entry ships in `hooks/hooks.json` and that neither event is normalized or encoded (ADR-001). |
| Provider | A2-REVIEW | Codex/Claude fake and live fixtures, parsing, read-only, timeout, downgrade. |
| Security | A2-INTEGRITY-SECURITY | Ledger/config protection, override rejection, command/path safety, prompt-injection-safe output. |
| Integration | A2-CLAUDE-INTEGRATION with A1 gate | Hook → CLI → core → ledger/runner/review paths. |
| End-to-end | A2-EVALUATION | Black-box task lifecycle on clean fixtures after M5. |
| Evaluation | A2-EVALUATION | 12 tasks, arms A–E, repetitions, raw metrics, guard against invented significance. |
| Release smoke | A2-DOCS-RELEASE | Install, demo, README truthfulness, export verification, package/name evidence. |

# 11. BUILD CONTROL PACKAGE FILE CONTENTS

The authoritative individual file contents are the sixteen Markdown files in this package. This combined report is an index/summary; the individual files are intended to be copied into the future repository build-control directory after repository bootstrap is separately authorized.

Files:
- `00_AGENT1_DECOMPOSITION_AND_INDEX.md`
- `01_ARCHITECTURE_AUTHORITY.md`
- `02_COMPONENT_OWNERSHIP.md`
- `03_IMPLEMENTATION_DEPENDENCY_GRAPH.md`
- `04_CONTRACT_INDEX.md`
- `05_MILESTONE_PLAN.md`
- `06_TASK_LEDGER.md`
- `07_COMPONENT_STATUS.md`
- `08_DEPENDENCY_REQUESTS.md`
- `09_DECISION_LOG.md`
- `10_OPEN_ISSUES.md`
- `11_INTEGRATION_GATES.md`
- `12_EVIDENCE_REQUIREMENTS.md`
- `13_RELEASE_GATES.md`
- `14_AGENT_HANDOFF_PROTOCOL.md`
- `15_ARCHITECTURE_DEVIATION_PROTOCOL.md`

Alongside them: this report, `README_PACKAGE.md`, and `MANIFEST.sha256`. The approved deviation record itself, `ARCHITECTURE_DEVIATION_REQUEST_001.md`, is canonical in the contract-freeze package and is referenced, not duplicated, here.

# 12. OPEN QUESTIONS THAT ACTUALLY BLOCK IMPLEMENTATION

| Issue | Blocking level | Question | Resolution |
|---|---|---|---|
| OI-001 | BLOCKS A3-LEDGER | Select Node/TypeScript runtime baseline, package manager, build/test framework, and SQLite driver. Architecture fixes topology/semantics, not library choice. | A2-LEDGER proposes; A1 approves before first ledger A3 task. |
| OI-002 | BLOCKS A3-LEDGER | Freeze canonical JSON serialization algorithm used by LedgerEvent hash chain so independent verification is byte-stable. | A2-LEDGER proposes deterministic canonicalization with fixtures; A1 freezes CONTRACT-LEDGER-001 serialization appendix. |
| OI-003 | BLOCKS A3-REVIEW-CLAUDE-FALLBACK | Freeze same-vendor `claude -p` invocation that is read-only, separate-session, structured, and does not recursively load Receipts hooks while still supporting the intended local authentication path. | A2-REVIEW + A2-CLAUDE-INTEGRATION verify locally; A2-INTEGRITY-SECURITY signs off. |
| OI-004 | BLOCKS A3-CLAUDE-PERMISSIONS | Verify exact deny-rule representation for protecting project `.receipts/policy.yaml` and `.receipts/recipes.yaml`, and ledger-path access where the persistent path is outside the repo and dynamically rooted at CLAUDE_PLUGIN_DATA. | A2-CLAUDE-INTEGRATION + A2-INTEGRITY-SECURITY test current Claude version; freeze fixtures. |
| OI-005 | BLOCKS A3-RUNNER-APPROVAL | Choose concrete interactive human recipe-approval UX and persistence representation without allowing an agent to manufacture approval. | A2-RUNNER + A2-INTEGRITY-SECURITY propose; A1 freezes before implementation. |
| OI-006 | NONBLOCKING UNTIL RELEASE | Product name collision check across GitHub, npm, PyPI, crates.io, and web. `Receipts` is provisional. | A2-DOCS-RELEASE performs before name adoption/release. |
| OI-007 | NONBLOCKING UNTIL M6 | Exact demo language ecosystem and benchmark fixture implementation details; architecture indicates a small TypeScript demo but benchmark code must be authored reproducibly. | A2-EVALUATION proposes after product contracts are stable. |
| OI-008 | NONBLOCKING / DEFERRED | Gemini provider. MVP includes it only if implementation cost is under one day; no Gemini syntax is frozen at A1 bootstrap. | A2-REVIEW may propose after Codex + Claude fallback are complete. |
| OI-009 | NONBLOCKING / POST-MVP | Does configuring a `WorktreeRemove` hook also displace Claude Code's default worktree cleanup? Unresolvable from current documentation; needs a local version smoke test. | A2-CLAUDE-INTEGRATION post-MVP. Both worktree hooks stay uninstalled per ADR-001 until then. |

None of these block A2 manager initialization. They block the specific A3 work named. **None of them is an architecture blocker, and none blocks contract freeze.** ADR-001 was the only contract-freeze blocker and it is APPROVED and closed.

# 13. GO / BLOCKED DECISION

**GO — begin component-manager initialization.**

**CONTRACT FREEZE — READY / FROZEN.** ADR-001 is APPROVED; `CONTRACT-PLUGIN-001` and `CONTRACT-PLUGIN-002` are 1.0.0 FROZEN; no architecture-blocking issue remains.

**BLOCKED — source implementation remains blocked.**

A2 managers may now initialize, validate their boundaries, resolve blocking implementation details, and produce future bounded A3 task prompts only after required contracts/issues are cleared. Do not create source implementation, repository, branches, worktrees, or dependencies as part of this A1 phase.

---

# External-interface freshness record

## Current primary-source verification (accessed 9 August 2026)

Mutable external interfaces were re-checked before freezing implementation-facing contracts.

- Claude Code plugins: https://code.claude.com/docs/en/plugins
- Claude Code plugins reference: https://code.claude.com/docs/en/plugins-reference
- Claude Code hooks reference: https://code.claude.com/docs/en/hooks
- Claude Code permissions: https://code.claude.com/docs/en/permissions
- Claude Code skills: https://code.claude.com/docs/en/skills
- Claude Code custom subagents: https://code.claude.com/docs/en/sub-agents
- Claude Code programmatic/headless mode: https://code.claude.com/docs/en/headless
- Claude Code CLI reference: https://code.claude.com/docs/en/cli-usage
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive/
- Codex CLI reference: https://developers.openai.com/codex/cli/reference/

Freshness findings:
1. The documented Claude plugin layout remains compatible with the architecture: `.claude-plugin/plugin.json` at the plugin root, plus root-level `skills/`, `agents/`, and `hooks/`.
2. Claude command hooks still support exec form through `command` + `args`; with `args`, no shell is involved.
3. `TaskCompleted`, `PostToolBatch`, `SubagentStart`, `SubagentStop`, `WorktreeCreate`, and `WorktreeRemove` remain documented hook events. `TaskCompleted` can block completion with exit code 2. **Corrected by ADR-001 (APPROVED 2026-08-09):** this bootstrap reading of the worktree events was incomplete. Configuring `WorktreeCreate` *replaces* Claude Code's default Git worktree creation, requires the hook to create and return the worktree path, and aborts creation on any non-zero exit. `WorktreeRemove` grants no decision control and its failures are logged in debug mode only. Receipts installs neither hook in MVP.
4. `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` remain documented plugin path variables available to hook processes.
5. Codex still supports `codex exec`, `--sandbox read-only`, `--json`, `--output-schema`, `-o/--output-last-message`, `--ignore-user-config`, `--ignore-rules`, and `--skip-git-repo-check`.
6. Current Codex CLI documentation now describes `--full-auto` as a deprecated compatibility flag. Receipts already forbids passing `--full-auto`, so this does not require an architecture deviation.
7. Claude plugin subagents ignore `permissionMode`, `hooks`, and `mcpServers` frontmatter. Read-only behavior for the plugin reviewer must therefore be established through its explicit tool allowlist and through the provider invocation boundary, not through plugin-subagent `permissionMode`.
8. Claude `-p` supports JSON output and JSON-schema structured output. The exact same-vendor fallback invocation remains an A2-REVIEW implementation-freeze item because it must avoid accidental loading/recursion of Receipts hooks while preserving the intended local authentication path.

Conclusion at the time of A1 bootstrap: no architecture deviation had been identified. **This conclusion was superseded during contract freeze.** `ARCHITECTURE_DEVIATION_REQUEST-001` was raised against the `WorktreeCreate` hook mapping and was **APPROVED** by the architecture authority on 2026-08-09. The approved minimal correction removes `WorktreeCreate` (and, after current-doc verification, `WorktreeRemove`) from the MVP installed hook set and binds workspace identity observationally. No other architecture semantics changed. See `15_ARCHITECTURE_DEVIATION_PROTOCOL.md` and `ARCHITECTURE_DEVIATION_REQUEST_001.md` in the contract-freeze package.

