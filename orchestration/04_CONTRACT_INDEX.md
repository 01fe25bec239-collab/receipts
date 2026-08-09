# 04 — Contract Index

| Contract | Subject | Owner | Consumers | Status |
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
| CONTRACT-HOOKS-001 | HookInputNormalization | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS; external fixtures current-doc-bound; worktree events excluded per ADR-001 |
| CONTRACT-HOOKS-002 | HookDecision + CLI exit-code contract | A2-CLAUDE-INTEGRATION | A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS; exact hook JSON current-doc-bound; worktree events excluded per ADR-001 |
| CONTRACT-ERROR-001 | Typed error model | A2-CORE | All components | FROZEN-SEMANTICS |
| CONTRACT-EXPORT-001 | Portable export bundle + independent verification | A2-LEDGER | A2-INTEGRITY-SECURITY, A2-EVALUATION, A2-DOCS-RELEASE | FROZEN-SEMANTICS |
| CONTRACT-PROCESS-001 | Process execution safety: argv/cwd/env/timeout/cancel/output | A2-RUNNER | A2-REVIEW, A2-CORE, A2-INTEGRITY-SECURITY | FROZEN-SEMANTICS |


## Frozen semantic definitions

### CONTRACT-CORE-001 — RepositoryIdentity + CodeStateFingerprint
- `repoId`: SHA-256 of first root commit SHA; fallback UUID persisted in broker ledger for repositories without a root commit.
- `headSha`: `git rev-parse HEAD`.
- `dirty`: whether staged/unstaged/untracked-not-ignored state differs from clean HEAD.
- `workingTreeDigest`: deterministic digest over staged tracked tuples plus fresh hashes for modified/untracked-not-ignored entries.
- `fingerprint`: SHA-256 over repository identity, HEAD, and working tree digest.
- Equal fingerprints mean evidence may be revalidated; a revert may restore validity.
- MVP is whole-tree, not path-scoped.

### CONTRACT-CORE-002 — Task + TaskState + AgentIdentity
- Task fields remain those in architecture: stable taskId, title, repoId, baselineSha, declaredPaths, policyProfile, frozen requiredClaims, state, optional externalRef, timestamps.
- State transitions remain architecture-defined; `ADMITTED* -> SUBMITTED` on fingerprint change is mandatory.
- AgentIdentity is provenance (`agent_id`, `agent_type`) or explicit human identity; it is never evidence authority.

### CONTRACT-CORE-003 — Claim + ClaimKind + ClaimStatus
- Two claim kinds: deterministic and review.
- MVP claim types: IMPLEMENTED, TESTED, LINT_CLEAN, REVIEWED.
- Status enum: UNPROVEN, PROVED, REJECTED, STALE, WAIVED.
- Agent assertion creates/updates claim provenance only; it does not prove.
- WAIVED is fingerprint-scoped and is invalidated on state change.

### CONTRACT-EVIDENCE-001 — Evidence + CodeEvidence
- Evidence family enum: CODE, DETERMINISTIC, REVIEW.
- Every evidence row has exactly one task, claim, fingerprint, createdAt, broker producer marker, payload reference, and ledger-chain relation.
- Deterministic and review evidence are type-separated.
- CodeEvidence records diff facts and integrity signals; it is broker-captured.

### CONTRACT-RUNNER-001 — VerificationRecipe
- Recipe is human-approved configuration only.
- Broker executes approved argv only; agent-supplied shell strings are never authority.
- Entry digest is part of evidence validity; recipe change invalidates prior evidence for that key.
- Recipe includes test globs required for integrity signals.
- Concrete approval UX is implementation-blocked by OI-005, but the authority rule is frozen.

### CONTRACT-RUNNER-002 — ExecutionReceipt
Required MVP fields are architecture I.2 exactly: receiptId, claimId, recipeKey, recipeDigest, repoId, baselineSha, headSha, workingTreeDigest, fingerprint, argv, cwd, resolvedExecutable, startedAt, finishedAt, exitCode, timedOut, stdoutDigest, stderrDigest, rawLogRef, runnerVersion, invokedByAgent, parsed.
- Exit 0 may prove the mapped deterministic claim if fingerprint/recipe/schema are valid.
- Nonzero is negative evidence; timeout is explicitly recorded.
- Receipt does not prove test meaningfulness or toolchain integrity.

### CONTRACT-POLICY-001 — VerificationPolicy
- LIGHT, STANDARD, and HIGH_ASSURANCE semantics remain architecture-defined.
- MVP exercises LIGHT and STANDARD; HIGH_ASSURANCE ships as config.
- `distinct_vendor` is policy, not invariant.
- Strictest matching path override wins.
- Task requiredClaims freeze at OPEN; later relaxation requires recorded policy amendment.

### CONTRACT-ADMISSION-001 — Admission + AdmissionDecision
- Decision enum: ADMIT, BLOCK, ADMIT_WITH_OVERRIDE.
- Pure function inputs: policy, derived claim statuses, current fingerprint, and time only where policy explicitly permits review max-age.
- Stored admission is audit evidence, never source of truth.
- Every stale-caused BLOCK includes the paths that changed.
- Downgrades are explicit strings/facts, never silent fallback.
- Recomputed value wins over stored disagreement and disagreement is evented.

### CONTRACT-OVERRIDE-001 — Override + Waiver
- Interactive human only; agent context rejected.
- Non-empty reason required.
- Record actor, reason, task, timestamp, fingerprint, and full unmet list.
- Fingerprint-scoped; no standing override.
- Override causes ADMITTED_WITH_OVERRIDE, never proof.
- Waiver is similarly task+fingerprint scoped and cannot silently persist through code change.

### CONTRACT-REVIEW-001 — ReviewRequest / ReviewResult / ReviewFinding
- Review binds exact fingerprint and exact diff.
- Result status: COMPLETED | FAILED | TIMEOUT | MALFORMED.
- `parseOk=false` or non-COMPLETED leaves claim UNPROVEN.
- Model is recorded as provider-reported identity.
- Findings use INFO/LOW/MEDIUM/HIGH/CRITICAL and structured path/line/category/summary/rationale.
- Review assertions are not deterministic proof.

### CONTRACT-REVIEW-002 — ReviewProvider + ProviderResolution
Exactly four provider operations remain: `health`, `capabilities`, `review`, `cancel`, plus immutable `id` and runtime `vendor`.
- Providers may not become session managers, routers, writers, or delegators.
- Read-only enforcement is mandatory for reviewers.
- Provider health failure produces a recorded selection/downgrade fact.
- Provider/model names live in config.
- Exact Claude fallback invocation is blocked by OI-003; semantics are frozen.

### CONTRACT-LEDGER-001 — LedgerEvent
- `events` is append-only source of truth.
- Every event stores canonical JSON payload, previous hash, and current hash.
- Projections are rebuildable and non-authoritative.
- Independent hash verification must detect mutation/truncation inconsistency within the defined chain model.
- Canonical byte encoding is blocked by OI-002 before A3 ledger work.

### CONTRACT-STORAGE-001 — transaction / projections
- SQLite WAL mode, busy_timeout, one transaction per broker invocation.
- Append event + affected projection updates must be atomic.
- Projection rebuild from events must produce an equivalent logical database.
- Raw logs/diffs live outside SQLite and are referenced by digest/path.

### CONTRACT-HOOKS-001 — HookInputNormalization
- Installed MVP hook events are `SessionStart`, `PostToolUse`, `PostToolBatch`, `SubagentStart`, `SubagentStop`, `TaskCompleted`, `PreToolUse`, and `Stop`. `WorktreeCreate` and `WorktreeRemove` are excluded (ADR-001, APPROVED 2026-08-09) and are reserved names.
- Workspace identity is bound observationally from `SessionStart` / current `cwd`, repository identity, read-only Git worktree metadata discovered by the broker, and normal broker invocations from the active working directory. Workspace-binding invalidation is lazy at next session start.
- Normalize common `cwd`, session identity, and event name.
- Preserve `agent_id`/`agent_type` when present.
- Event-specific fields must be parsed from current documented schemas; unknown fields ignored, missing required fields yield typed input error.
- No hook input string may become a shell fragment.
- Current JSON fixtures are versioned by A2-CLAUDE-INTEGRATION.

### CONTRACT-HOOKS-002 — HookDecision + CLI exit codes
- `PreToolUse` uses JSON decision on exit 0; recognized protected merge/push action fails closed on broker/policy error.
- `TaskCompleted`: ADMIT => exit 0; BLOCK => exit 2 with bounded stderr; ADMIT_WITH_OVERRIDE => exit 0 with explicit non-verified status.
- `PostToolUse` observer never blocks; async.
- `PostToolBatch` recomputes but Receipts does not block there.
- `WorktreeCreate` and `WorktreeRemove` are **not installed** by Receipts in MVP, so no decision encoding exists for either (ADR-001, APPROVED 2026-08-09). There is no always-exit-0 worktree handler because there is no worktree handler. Receipts does not own worktree creation and does not replace Claude Code's default Git worktree behavior.
- Hook-facing text is <10,000 characters.
- Generic CLI operational failure uses nonzero status but cannot be confused with a successful verification receipt.

### CONTRACT-ERROR-001 — Error model
Categories: INPUT, CONFIG, GIT, STORAGE, PROCESS, PROVIDER, POLICY, INTEGRITY, INTERNAL.
Every error has stable code, safe human message, optional structured detail, and causal chain for logs.
- Policy BLOCK is not an INTERNAL error.
- Provider timeout/malformed is evidence state, not proof.
- Secrets/raw provider credentials never enter safe messages.

### CONTRACT-EXPORT-001 — export bundle
- Versioned portable JSON bundle containing hash-chain events plus enough schema/version metadata to verify independently.
- Must preserve overrides and downgrades exactly.
- Export does not upgrade evidence authority.
- Independent verifier must detect mutation.

### CONTRACT-PROCESS-001 — process safety
- All broker-owned process launches are explicit argv, not shell strings.
- cwd is validated and realpath-resolved.
- Environment is minimal/allowlisted at runner boundary; provider auth is passed through only as needed and never copied into receipts.
- Broker owns timeout/cancellation and captures stdout/stderr separately.
- Untrusted agent text may enter a provider prompt as data but never an executable/argv field.

## Freeze meaning

`FROZEN-SEMANTICS` means producer/consumer semantics may be implemented immediately once component-specific blocking issues are cleared. An A2 may refine type names, module placement, or library-specific representation but may not change fields, authority, state semantics, or security behavior without an A1 contract amendment or architecture deviation where applicable.
