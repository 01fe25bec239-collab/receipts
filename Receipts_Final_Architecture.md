# Receipts — Final Architecture Correction Pass
### A claim / evidence / policy / admission layer for AI coding-agent work in Claude Code

**Date of this pass:** 9 August 2026
**Status:** implementation-ready architecture. No code written.
**Working name:** `Receipts` (provisional — see §X.7 for the required collision check before adoption).

**Label key used throughout**
`[VF]` VERIFIED CURRENT FACT — read from a live primary source during this pass, with citation.
`[DD]` DESIGN DECISION — a choice made here, not a fact.
`[AS]` ASSUMPTION — believed true, not verified this pass; must be checked before it becomes load-bearing.
`[UNVERIFIED]` could not be confirmed from a trustworthy current source.

---

## A. FINAL PRODUCT DEFINITION

**Receipts is a durable, code-state-bound evidence ledger and admission engine for coding-agent work, driven from Claude Code.**

The one-line abstraction the product is built on:

> AI coding agents produce **CLAIMS**. The broker produces or captures **EVIDENCE**. **POLICY** decides whether that evidence is sufficient. **ADMISSION** controls whether the workflow may advance.

Concretely, Receipts:

1. Records every agent assertion about work (`TESTED`, `LINT_CLEAN`, `REVIEWED`, …) as a typed **Claim** rather than as prose in a transcript.
2. Proves or disproves deterministic claims by **running the check itself** under a project-approved **VerificationRecipe**, and storing an **ExecutionReceipt** that binds the result to an exact **CodeStateFingerprint**.
3. Obtains **ReviewEvidence** for claims that cannot be decided deterministically, from an independent **ReviewProvider** (Codex, Gemini, or a separate Claude session), never treating that output as proof.
4. Automatically marks evidence **STALE** when the code state it referred to changes.
5. Evaluates a declarative **VerificationPolicy** to produce an **Admission** decision, and enforces that decision at Claude Code's own gates.
6. Records human **overrides** as first-class, non-erasable facts. An overridden task is never displayed as proven.

**What Receipts is not:** not a multi-agent orchestrator, not an agent adapter framework, not a router, not a sandbox, not a proof system in the mathematical sense, not a correctness oracle.

**The honest scope sentence, which must appear in the README:**
> Receipts establishes that *the checks this project declared* passed against *this exact code state*, and that a reviewer of a stated identity examined *this exact diff*. It does not establish that the code is correct.

---

## B. NATIVE CLAUDE CAPABILITIES WE REUSE — and why this is not "TaskCompleted + tests"

### B.1 What is natively available (verified this pass)

`[VF]` The hooks reference lists a `TaskCompleted` event: it fires "When a task is being marked as completed," and on exit code 2 it "Prevents the task from being marked as completed." Its decision pattern is "Exit code or `continue: false`," and it has **no matcher support**. (code.claude.com/docs/en/hooks.md, fetched 9 Aug 2026.)

`[VF]` The agent-teams page confirms the intended use: "Use hooks to enforce rules when teammates finish work or tasks are created or completed … `TaskCompleted`: runs when a task is being marked complete. Exit with code 2 to prevent completion and send feedback." (code.claude.com/docs/en/agent-teams.md, fetched 9 Aug 2026.)

`[VF]` The pattern is already common in the community. One widely-read write-up describes exactly it: "A `TaskCompleted` hook runs lint and tests before marking a task as done. If the hook fails, the agent keeps working until it passes." (addyosmani.com/blog/code-agent-orchestra, 26 Mar 2026.)

**So the user's finding is correct and load-bearing: "run tests before Claude can mark a task done" is a native one-liner and is not a product.**

Other native capabilities Receipts reuses rather than reimplements:

| Native capability | Verified detail | Receipts reuses it for |
|---|---|---|
| Hooks fire inside subagents | `[VF]` "tool events such as `PreToolUse` and `PostToolUse` fire the same configured hooks as in the main conversation, and the input carries the `agent_id` and `agent_type` … fields that identify the subagent" | Claim authorship / provenance without an agent framework |
| `PreToolUse` can deny | `[VF]` `permissionDecision` ∈ allow/deny/ask/defer; exit 2 blocks the tool call | Level-2 merge/push gate |
| `PostToolUse` cannot block, but can run `async` | `[VF]` exit-2 table: PostToolUse "Can block? No"; command hooks accept `async: true` | Non-blocking evidence observation |
| `PostToolBatch` fires once per parallel batch, before the next model call, and *can* block | `[VF]` exit 2 "Stops the agentic loop before the next model call" | One fingerprint recompute per batch instead of per edit |
| `WorktreeCreate` / `WorktreeRemove` | `[VF]` WorktreeCreate: command hook prints path on stdout; **any non-zero exit fails creation**. WorktreeRemove: no decision control | Workspace identity binding; cleanup |
| Plugin packaging + persistent data dir | `[VF]` plugin `hooks/hooks.json`; `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` are substituted **and exported as env vars** to hooks | Ship as a plugin; store the ledger outside the repo |
| Exec-form hooks | `[VF]` when `args` is present, `command` is spawned directly with no shell | Injection-resistant hook invocation |

### B.2 Why Receipts is materially more than `TaskCompleted` + tests

A `TaskCompleted` hook that runs the test suite is **stateless, instantaneous, untyped, and unbound to code state**. Six specific consequences, each of which is a layer Receipts owns:

**1. It has no memory, so it must recompute everything or nothing.**
The hook is a process that starts, runs, and exits. It cannot know that the same suite already passed against the identical tree ninety seconds ago. In practice teams therefore either (a) run the full suite on every task completion, paying the cost every time, or (b) run a cheap subset and get weak assurance. Receipts keeps a **content-addressed evidence cache**: a claim already PROVED against the current fingerprint is admitted without re-running. *This makes verification cheaper as well as stricter*, which is the single most important product property — see §I.

**2. It has no code-state binding, so "tests passed" is a floating fact.**
Native gating proves something at the instant of completion; nothing connects that result to the commit that is eventually merged. Between the task being marked done and the merge, the agent can edit five more files and the earlier green result silently retains its social authority in the transcript. Receipts binds every ExecutionReceipt to a `CodeStateFingerprint` and **invalidates it the moment the tree changes** (§M). Native hooks have no concept of staleness because they have no concept of persistence.

**3. It is a single boolean at a single moment, not a policy over typed claims.**
`TaskCompleted` gives one gate: block or don't. It cannot express "tests required, lint required, independent review required only for files under `src/auth/**`, different vendor preferred, high-severity findings block." Receipts evaluates a declarative policy over independently-tracked claims, each with its own status (§K, §L).

**4. It cannot represent probabilistic evidence at all.**
A `prompt`-type or `agent`-type hook can ask a model for a verdict, but the result is a yes/no with no findings structure, no reviewer identity, no vendor attribute, and no separation from deterministic results. Receipts keeps two distinct evidence families and never lets a model verdict satisfy a deterministic claim (§H, §J).

**5. It has no audit surface.**
When a regression later appears, `TaskCompleted` leaves nothing behind: no record of which agent claimed what, against which tree, using which command, producing which output. Receipts' ledger answers "what was believed, on what basis, and when did that basis expire" (§R).

**6. It is not universally available.**
`[VF]` Agent teams are "experimental and disabled by default. Enable them by setting `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` … Without that variable, no team is set up at session start, no team directories are written." `[VF]` `TaskCreated`, `TaskCompleted` and `TeammateIdle` are documented on the agent-teams page as the team quality-gate hooks, and a community catalogue states plainly that these three "require the experimental agent teams feature." `[AS]` Therefore `TaskCompleted` should be treated as **available only when agent teams are enabled**, and Receipts must not depend on it as its sole gate. This is a design constraint, not a differentiator — see §O for the two-gate design.

**And one thing native gating actively cannot do:** `[VF]` The agent-teams limitations section states "teammates sometimes fail to mark tasks as completed." A gate that only fires *on* completion is bypassed by never completing. Receipts' admission is a property of the ledger, queryable at any time, not an event.

### B.3 The nearest real prior art, stated honestly

`[VF]` NousResearch `hermes-agent` PR #52285, "record coding verification evidence," implements a "profile-scoped verification evidence ledger," records "foreground terminal test/lint/typecheck/build results as scoped evidence," and marks "prior evidence stale after successful `write_file` / patch edits." It closes with: *"This is intentionally passive: it records evidence, not guarantees."*

That is the closest thing to Receipts that exists, it validates the staleness-on-edit design, and it is **explicitly passive**. Receipts is the active form: the same ledger wired to an admission decision that blocks Claude Code's own gates, with provenance strong enough that the ledger's contents are not merely the agent's self-report. `[DD]` The differentiation must be stated exactly that way and never overstated.

---

## C. CORE INVARIANTS

These are the load-bearing rules. Any change that violates one is a redesign, not a tweak.

1. **No claim is ever proved by an agent's assertion.** A claim reaches `PROVED` only from an ExecutionReceipt the broker produced, or from a ReviewEvidence record the broker obtained.
2. **Every piece of evidence names exactly one code state.** No fingerprint, no evidence.
3. **Evidence is valid only while its fingerprint matches the current one.** Nothing is "true from now on."
4. **Deterministic and probabilistic evidence never substitute for each other.** A model verdict cannot satisfy `TESTED`; a green suite cannot satisfy `SECURITY_REVIEWED`.
5. **Admission is a pure function** of (policy, claim statuses, current fingerprint). It is derivable, reproducible, and never a stored opinion.
6. **The evidence store is not writable by worker agents.** Broker-only writes; store lives outside the repository working tree.
7. **Verification commands come only from an approved recipe.** The broker never runs a command an agent supplied.
8. **Human override is always available and always recorded.** An overridden task is `ADMITTED_WITH_OVERRIDE`, never `ADMITTED`.
9. **Enforcement claims are scoped honestly.** Receipts governs Claude-Code-mediated actions only (§S.5).
10. **Model and provider identities are configuration.** No vendor name appears in a code path.

---

## D. TRUST MODEL

### D.1 Adversaries, and which are in scope for MVP

| | Adversary | MVP posture | Mechanism |
|---|---|---|---|
| A | Dishonest / hallucinating LLM worker ("tests pass" when they didn't) | **DEFENDED** | Broker runs the check itself; agent output is never evidence |
| B | Prompt-injected worker (repo content tells it to fabricate evidence) | **DEFENDED for evidence content**, not for behaviour | The injected agent still cannot write to the ledger; it can only ask the broker to run an approved recipe |
| C | Compromised external agent process (Codex/Gemini binary or session subverted) | **PARTIAL** | Reviewers run read-only and their output is only ever `ReviewEvidence`, never deterministic proof |
| D | Malicious repository (hostile test/recipe content) | **PARTIAL** | Recipe changes require human approval and are digest-tracked; but an approved recipe that runs hostile project code is by definition executing project code — see §S.3 |
| E | Malicious human machine owner | **OUT OF SCOPE** | A local SQLite file on a machine you control cannot be defended from you |
| F | Compromised broker | **OUT OF SCOPE** | The broker is the trust anchor by construction |

### D.2 The exact trust boundary sentence

> Receipts' evidence is **tamper-evident with respect to the agents it supervises**, and **not tamper-evident with respect to the machine's owner**.

`[DD]` Do not write "tamper-proof" anywhere. Do not claim cryptographic guarantees the deployment does not provide: an HMAC key stored on the same machine as the ledger protects against a confused or injected agent, not against the user, and the README must say so.

### D.3 Mechanisms, in increasing order of strength

1. **Location.** `[DD]` The ledger lives in `${CLAUDE_PLUGIN_DATA}/receipts/<repoId>/` — **outside the repository working tree**. `[VF]` `CLAUDE_PLUGIN_DATA` is the plugin's persistent data directory and is exported into the hook process environment. Consequence: an agent editing repo files cannot reach the ledger through ordinary Edit/Write at all.
2. **Filesystem permissions.** `0700` directory, `0600` database `[DD]`.
3. **Deny rules.** `[VF]` The hooks docs warn that the `if` filter "fails open" and instruct: "use the permission system rather than a hook to enforce a hard allow or deny." `[DD]` Therefore ship *both*: `deny` permission rules for `Read`/`Edit`/`Write` on the ledger path **and** a `PreToolUse` Bash inspector, and treat the permission rule as the real control.
4. **Broker-only writes.** The ledger has exactly one writer process; nothing else has the schema.
5. **Hash chaining.** `[DD]` Each ledger event stores `prevEventHash` and `eventHash = SHA-256(canonical(event) || prevEventHash)`. This detects truncation and in-place edits by anything that does not know to recompute the chain. It does **not** stop an attacker who does.
6. **Optional HMAC.** `[DD]` V2, off by default, with the honest caveat above.

---

## E. CLAIM MODEL

### E.1 The two families

`[DD]` A claim's *kind* determines which evidence family can satisfy it. There are exactly two kinds, and this is architecture. The *claim types* themselves are data.

**DeterministicClaim** — satisfied by an ExecutionReceipt with `exitCode == 0` from a named recipe key.
**ReviewClaim** — satisfied by ReviewEvidence whose findings pass the policy's severity threshold. Never "proof."

This is why the claim type list does not need to be baked in: `TESTED`, `LINT_CLEAN`, `TYPECHECKED`, `BUILD_SUCCEEDED`, `COVERAGE_MET`, `MIGRATION_VALIDATED` are all *the same machinery pointed at a different recipe key*. Adding one is a config edit, not a code change.

### E.2 Claim type definitions

| Claim type | Asserts | Evidence family | Deterministic? | Independent review required? | Staleness |
|---|---|---|---|---|---|
| `IMPLEMENTED` | A non-empty diff exists between `baselineSha` and the current code state, confined to declared paths | Code evidence | Yes | No | On any fingerprint change |
| `TESTED` | Recipe key `test` exited 0 against this code state | Execution | Yes | No | On any fingerprint change |
| `LINT_CLEAN` | Recipe key `lint` exited 0 | Execution | Yes | No | Same |
| `TYPECHECKED` | Recipe key `typecheck` exited 0 | Execution | Yes | No | Same |
| `REVIEWED` | An independent reviewer examined this exact diff and returned structured findings | Review | **No** | Yes, by definition | Same |
| `SECURITY_REVIEWED` | As above, under the security review profile | Review | No | Yes | Same |
| `BUILD_SUCCEEDED` | Recipe key `build` exited 0 | Execution | Yes | No | Same |
| `COVERAGE_MET` | Recipe key `coverage` exited 0 **and** parsed metric ≥ threshold | Execution | Yes | No | Same |
| `API_COMPATIBLE` | Recipe key `apicheck` exited 0 | Execution | Yes | No | Same |

### E.3 Minimum set for MVP

`[DD]` **Four claim types ship in MVP: `IMPLEMENTED`, `TESTED`, `LINT_CLEAN`, `REVIEWED`.**

Rationale: `IMPLEMENTED` is required because it is what anchors a task to a code state at all. `TESTED` is the claim the whole product exists to make honest. `LINT_CLEAN` is included **specifically to prove the config-not-code property** — it must be addable to the policy without touching the broker, and demonstrating that in the demo is worth more than the lint result itself. `REVIEWED` is required because without it the product really would be TaskCompleted-plus-a-cache.

`TYPECHECKED`, `BUILD_SUCCEEDED`, `COVERAGE_MET`, `MIGRATION_VALIDATED`, `API_COMPATIBLE`, and `SECURITY_REVIEWED` are **deferred to config**, not to code: they should work on day one for any project that adds the recipe key, but they are not exercised by the MVP demo or the evaluation.

---

## F. TASK MODEL

`[DD]` Task lifecycle and claim status are separate state machines. Admission is derived, never stored as the source of truth.

### F.1 Task lifecycle

```
DRAFT ──(claims declared, baseline pinned)──► OPEN
OPEN ──(agent requests advancement)──► SUBMITTED
SUBMITTED ──(policy satisfied)──────────► ADMITTED
SUBMITTED ──(policy unsatisfied)────────► BLOCKED
SUBMITTED ──(human override)────────────► ADMITTED_WITH_OVERRIDE
BLOCKED ──(new evidence arrives)────────► SUBMITTED   (re-evaluate)
ADMITTED* ──(fingerprint changes)───────► SUBMITTED   (evidence went stale; admission withdrawn)
ADMITTED* ──(merged / closed by human)──► CLOSED
```

The `ADMITTED* → SUBMITTED` edge is the one native gating cannot express, and it is not decorative: it is what makes "admitted" mean something twenty minutes later.

### F.2 Task record

```
Task {
  taskId            string        # stable, e.g. AUTH-31
  title             string
  repoId            string
  baselineSha       string        # pinned at OPEN
  declaredPaths     [glob]        # what this task is allowed to touch
  policyProfile     string        # LIGHT | STANDARD | HIGH_ASSURANCE | <custom>
  requiredClaims    [ClaimType]   # resolved from profile at OPEN, then frozen
  state             TaskState
  externalRef       string?       # Claude Code task-list id when present
  createdAt, updatedAt
}
```

`[DD]` `requiredClaims` is frozen at `OPEN` so that a mid-task policy edit cannot silently relax a gate. Changing it requires an explicit, recorded `policy-amend` event.

### F.3 Claim record

```
Claim {
  claimId       string
  taskId        string
  type          ClaimType
  assertedBy    AgentIdentity      # from hook agent_id / agent_type, or "human"
  assertedAt    timestamp
  status        UNPROVEN | PROVED | REJECTED | STALE | WAIVED
  evidenceRefs  [evidenceId]
  provedAt      timestamp?
  provedAgainst CodeStateFingerprint?
}
```

Status semantics, stated precisely:

- `UNPROVEN` — no admissible evidence exists. Initial state.
- `PROVED` — admissible evidence exists whose fingerprint equals the current fingerprint.
- `REJECTED` — evidence exists and is negative (non-zero exit, or blocking findings). Distinct from `UNPROVEN` because it should be surfaced loudly and should not be silently retried without a code change.
- `STALE` — evidence exists, was positive, and its fingerprint no longer matches. **Not** an error; it is a cache miss with history.
- `WAIVED` — a human explicitly excused this claim for this task. Scoped to one task, recorded with reason and actor, and — `[DD]` — a waiver is itself invalidated by a fingerprint change, exactly like evidence, so a waiver cannot become a permanent hole.

### F.4 Worked example (the user's own)

```
TASK AUTH-42          profile: STANDARD        fingerprint: 9f2c…a1

  IMPLEMENTED        PROVED     ← receipt r-118, matches current fingerprint
  TESTED             STALE      ← receipt r-121 was green at 3e88…c4, tree has moved
  LINT_CLEAN         PROVED     ← receipt r-133
  SECURITY_REVIEWED  STALE      ← review v-07 examined diff at 3e88…c4

  ADMISSION = BLOCKED
  Unmet: TESTED (stale since 14:22), SECURITY_REVIEWED (stale since 14:22)
  Cause: 2 files changed after evidence was recorded — src/auth/rotate.ts, src/auth/store.ts
```

Note what the `Cause:` line does: it converts "blocked" from an obstruction into a diagnosis. `[DD]` Every BLOCKED admission must name the specific changed paths that invalidated the evidence. This is the single highest-value UX detail in the product and it is cheap, because the fingerprint diff already contains it.

---

## G. CODE-STATE FINGERPRINT

### G.1 Fields

```
CodeStateFingerprint {
  repoId             string   # stable repo identity
  headSha            string   # git rev-parse HEAD
  dirty              bool
  workingTreeDigest  string   # SHA-256 over the tracked+staged+modified state
  fingerprint        string   # SHA-256(repoId | headSha | workingTreeDigest)
}
```

### G.2 How each is computed `[DD]`

- **`repoId`** — SHA-256 of the SHA of the repository's first root commit (`git rev-list --max-parents=0 HEAD`), falling back to a UUID persisted in the ledger for repos without one. Deliberately **not** the remote URL (renames, forks, multiple remotes) and **not** the absolute path (worktrees, clones).
- **`headSha`** — `git rev-parse HEAD`.
- **`workingTreeDigest`** — computed cheaply, not by hashing the repo:
  1. `git ls-files -s` gives path + mode + **blob SHA for every tracked file as staged**. Git has already hashed the content; reuse it.
  2. `git status --porcelain=v1 -z` identifies files modified-but-unstaged and untracked-but-not-ignored.
  3. Hash the contents of only that second, small set.
  4. Digest = SHA-256 over the sorted, NUL-delimited concatenation of `(path, mode, blobSha)` tuples plus the freshly hashed entries.

  This is O(index size) for the cheap part and O(dirty files) for the expensive part. `[AS]` On a large monorepo `git ls-files -s` is still tens of megabytes of output; if measured cost exceeds ~200 ms, restrict the index scan to `declaredPaths ∪ recipeScope` and record that restriction **inside the fingerprint** so a narrowed fingerprint is never silently compared against a full one.
- **`fingerprint`** — the comparison key.

### G.3 Which fields are MVP

`[DD]` **All four.** This is the one place where cutting scope destroys the product: a fingerprint without `workingTreeDigest` cannot distinguish "tests passed on this commit" from "tests passed on this commit before I edited three files," which is precisely the failure Receipts exists to catch. `changedPathsDigest` relative to `baselineSha` is computed for *display and diagnosis*, and is **deferred** as a staleness input until V2 (§M.3).

### G.4 VALID and STALE, defined

> Evidence `E` is **VALID** for task `T` at time `t` **iff** `E.fingerprint == currentFingerprint(T.repoId)` at `t`, **and** `E.recipeDigest == currentRecipeDigest(E.recipeKey)`, **and** `E.brokerSchemaVersion` is compatible.
> Otherwise `E` is **STALE**.

Three consequences worth stating explicitly:
- Staleness is **not** time-based. `[DD]` TTL is not a staleness input in MVP. A green suite on an untouched tree is as valid tomorrow as it was today. (An optional `maxAge` exists in the policy schema for review evidence only, where reviewer model drift is a real concern, and it defaults to unset.)
- Reverting a change **restores** validity, because the fingerprint returns to its prior value. This is a feature, and it is a nice demo beat.
- Changing the recipe invalidates evidence produced under the old recipe. Verification configuration is versioned input (§L).

---

## H. EVIDENCE MODEL

```
Evidence {
  evidenceId    string
  taskId        string
  claimId       string
  family        DETERMINISTIC | REVIEW | CODE
  fingerprint   CodeStateFingerprint
  createdAt     timestamp
  producedBy    "broker"                # always; the field exists to make that explicit
  payloadRef    receiptId | reviewId | codeDiffId
  prevEventHash, eventHash              # chain (§D.3)
}
```

Three families, and the separation is enforced at the type level `[DD]`:

- **CODE evidence** — the diff itself: `baselineSha`, `headSha`, changed paths, per-path blob SHAs, insertion/deletion counts, and the test-integrity signals of §S.4. Cheap, always captured.
- **DETERMINISTIC evidence** — an `ExecutionReceipt` (§I). The only thing that can prove a DeterministicClaim.
- **REVIEW evidence** — a `ReviewResult` (§J). Can satisfy a ReviewClaim; can *never* satisfy a DeterministicClaim, and the schema makes the mistake unrepresentable.

`[DD]` Raw logs are stored outside SQLite as gzipped files under `logs/<receiptId>.{out,err}.gz`, with only digests and a bounded head/tail excerpt in the row. Two reasons: SQLite row bloat, and `[VF]` hook output strings are capped at 10,000 characters, so hook-facing summaries must be short by construction anyway.

---

## I. EXECUTION RECEIPT

The core provenance record. `[DD]` A SHA-256 of stdout alone is rejected as insufficient — it proves a string existed, not that it came from running anything against any particular code.

### I.1 Full schema

```
ExecutionReceipt {
  receiptId           string
  claimId             string
  recipeKey           string          # "test" | "lint" | ...
  recipeDigest        string          # SHA-256 of the recipe entry as approved

  # WHAT code was checked
  repoId              string
  baselineSha         string
  headSha             string
  workingTreeDigest   string
  fingerprint         string

  # WHAT command ran
  argv                [string]        # exact argument vector, no shell string
  cwd                 string          # absolute, realpath-resolved
  resolvedExecutable  string          # realpath of argv[0]
  toolVersionDigest   string          # SHA-256 of the recipe's declared version-probe output
  envAllowlist        [string]        # names of env vars passed through
  envDigest           string          # SHA-256 of the sorted passed-through name=value pairs

  # WHO/WHAT ran it
  runnerVersion       string          # broker version
  runnerHost          string
  runnerUser          string
  invokedByAgent      AgentIdentity?  # agent_id/agent_type that triggered it, or null for human

  # WHEN
  startedAt, finishedAt timestamp
  durationMs          int

  # WHAT happened
  exitCode            int
  timedOut            bool
  stdoutDigest        string
  stderrDigest        string
  stdoutBytes, stderrBytes int
  rawLogRef           string
  parsed              object?         # optional structured extract (test counts, coverage %)
}
```

### I.2 MVP subset

`[DD]` **Required in MVP:** `receiptId`, `claimId`, `recipeKey`, `recipeDigest`, `repoId`, `baselineSha`, `headSha`, `workingTreeDigest`, `fingerprint`, `argv`, `cwd`, `resolvedExecutable`, `startedAt`, `finishedAt`, `exitCode`, `timedOut`, `stdoutDigest`, `stderrDigest`, `rawLogRef`, `runnerVersion`, `invokedByAgent`, `parsed`.

**Deferred:** `toolVersionDigest`, `envDigest`, `envAllowlist`, `runnerHost`, `runnerUser`. These matter for reproducibility across machines and for detecting "it passed because the toolchain changed"; they do not matter for a single-developer local MVP, and `toolVersionDigest` in particular costs an extra subprocess per run.

### I.3 What a receipt does and does not prove

`[DD]` State this in the docs verbatim, because overclaiming here would be the project's most likely intellectual failure:

- **It proves:** the broker executed exactly this argument vector in this directory while the repository was in exactly this state, and observed this exit code and this output digest.
- **It does not prove:** that the command was a meaningful check, that the tests were non-vacuous, that the toolchain was uncompromised, or that the same command run elsewhere would produce the same result.

---

## J. REVIEW EVIDENCE MODEL

```
ReviewRequest {
  reviewId       string
  taskId, claimId string
  fingerprint    CodeStateFingerprint
  profile        string              # "security" | "general" | ...
  diffRef        string              # the exact unified diff handed over
  includeTestDiff bool               # §S.4
  contextRefs    [path]              # read-only paths the reviewer may consult
  schema         object              # requested structured output schema
}

ReviewResult {
  reviewId       string
  providerId     string              # "codex" | "gemini" | "claude-session"
  vendor         string              # provider attribute, resolved at runtime
  model          string              # as reported by the provider, not as configured
  startedAt, finishedAt
  status         COMPLETED | FAILED | TIMEOUT | MALFORMED
  findings       [ReviewFinding]
  rawRef         string
  parseOk        bool
}

ReviewFinding {
  findingId  string
  severity   INFO | LOW | MEDIUM | HIGH | CRITICAL
  category   string
  path       string?
  line       int?
  summary    string
  rationale  string
  resolvesClaimId string?
}
```

### J.1 Rules `[DD]`

1. A `ReviewResult` is **evidence about a review having occurred**, plus a set of assertions. The assertions are not proof of anything.
2. `status != COMPLETED` or `parseOk == false` ⇒ the ReviewClaim stays `UNPROVEN`. **Never `PROVED` by default.** Fail closed on the claim, and let the human override if the provider is broken.
3. `model` is recorded **as reported by the provider**, never as configured, so that a silently substituted model is visible after the fact.
4. The reviewer runs **read-only**. A reviewer that can write will fix what it finds, which destroys both the independence and the finding record.
5. Findings are routed back to the implementer **as structured data**, not as prose. This is what lets a re-review be scoped to the findings rather than re-litigating the whole diff.

---

## K. VERIFICATION POLICY

### K.1 Schema

```yaml
# .receipts/policy.yaml   (committed; digest-tracked)
version: 1
default_profile: STANDARD

profiles:
  LIGHT:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
    review:
      mode: optional

  STANDARD:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
      - claim: LINT_CLEAN
    review:
      mode: required
      profile: general
      distinct_vendor: preferred      # off | preferred | required
      blocking_severity: HIGH         # findings >= this block admission
      include_test_diff: when_tests_changed

  HIGH_ASSURANCE:
    require:
      - claim: IMPLEMENTED
      - claim: TESTED
      - claim: LINT_CLEAN
      - claim: TYPECHECKED
    review:
      mode: required
      profile: security
      distinct_vendor: required
      blocking_severity: MEDIUM
      include_test_diff: always
      max_age: 7d                     # review evidence only; unset elsewhere
    test_integrity:
      on_test_deletion: block          # expose | block

path_overrides:
  - match: "src/auth/**"
    profile: HIGH_ASSURANCE
  - match: "docs/**"
    profile: LIGHT
```

### K.2 Design notes

- `[DD]` **`distinct_vendor` is a policy field, not an invariant.** The prior pass made cross-vendor review an architectural law; that was wrong. A developer with only Claude installed must still get value, and `distinct_vendor: off` with `providerId: claude-session` (a separate `claude -p` process with no shared context) is a legitimate, weaker configuration. `preferred` means: use a different vendor if one is healthy, otherwise fall back and **record the downgrade in the admission record**. That downgrade record is important — a silent fallback would be dishonest.
- `[DD]` Cross-vendor review stays the flagship capability and the default for `STANDARD`, because a reviewer sharing the implementer's weights shares its blind spots. But it is now a configuration position, defensible on its own terms, rather than a constraint imposed on every user.
- `path_overrides` resolve to the **strictest** matching profile, not the first match `[DD]`.

### K.3 Admission function

```
admit(task, policy, now) -> Admission

  fp := currentFingerprint(task.repoId)
  for each required claim c:
      status := statusOf(c, fp)        # PROVED only if evidence.fingerprint == fp
  unmet := { c : status(c) ∉ {PROVED, WAIVED} }

  if review required:
      r := latestReviewEvidence(task, fp)
      if r is absent or stale        -> unmet += REVIEWED
      if maxSeverity(r.findings) >= blocking_severity -> unmet += REVIEWED(blocking findings)
      if distinct_vendor == required and r.vendor == implementerVendor -> unmet += REVIEWED(vendor)

  if test_integrity.on_test_deletion == block and netTestFilesDeleted(task) > 0
     and no waiver                    -> unmet += TEST_INTEGRITY

  if unmet is empty        -> ADMIT
  else if active override  -> ADMIT_WITH_OVERRIDE(unmet)
  else                     -> BLOCK(unmet, causedBy: changedPathsSince(evidence))
```

`[DD]` This function is pure, has no I/O beyond reading current state, and must be **unit-testable in isolation**. It is the first thing to write and the first thing to test.

---

## L. ADMISSION MODEL

```
Admission {
  admissionId   string
  taskId        string
  fingerprint   string
  decision      ADMIT | BLOCK | ADMIT_WITH_OVERRIDE
  unmet         [{claimType, reason, causedByPaths[]}]
  policyDigest  string
  evaluatedAt   timestamp
  downgrades    [string]     # e.g. "distinct_vendor preferred -> same vendor (gemini unhealthy)"
  overrideId    string?
}
```

`[DD]` Admissions are **recomputed on demand and also appended to the ledger** whenever a gate consults them. The stored record is an audit artifact ("this is what we decided at 14:31, on this fingerprint, under this policy digest"), never the source of truth. If the two ever disagree, the recomputed value wins and the disagreement is itself logged.

---

## M. STALENESS RULES

### M.1 MVP rule — state it in one line

> `[DD]` **Any change to the code state invalidates every piece of evidence for every task in that repository.**

Whole-tree, conservative, no dependency analysis, no path scoping. Evidence rows are not deleted; they are simply no longer VALID, and they become VALID again if the tree returns to that state.

### M.2 Why conservative first

Because the alternative — "only invalidate evidence whose scope intersects the changed paths" — requires a dependency graph, and a *wrong* dependency graph produces silently-retained false evidence, which is the exact failure Receipts exists to prevent. `[DD]` The MVP's failure mode must be "too much re-running," never "wrongly kept green."

### M.3 The known cost, and the V2 path

The honest objection: on a repo where an agent edits one doc file, whole-tree invalidation re-runs the full suite. `[DD]` Mitigations, in order:
1. Evidence is re-*validated*, not re-*run*, whenever the tree returns to a previously-seen fingerprint. Reverts and no-op churn are free.
2. `[DD]` **V2 — scoped invalidation:** each recipe key declares `scope: [glob]`. Evidence for key `k` is invalidated only if `changedPaths ∩ scope(k) ≠ ∅`. The scope is declared by a human in the recipe, not inferred, so the failure mode stays human-auditable. This is deferred, not designed away.
3. `[DD]` **V3 — dependency-aware invalidation** using an import graph. Deferred indefinitely; only justified if measured re-run cost from §V proves it necessary.

### M.4 What triggers a recompute

`[DD]` The fingerprint is recomputed lazily — on any broker invocation that needs it — and eagerly on `PostToolBatch` (once per tool batch, not once per edit) so that the ledger's staleness view is fresh enough to be shown in `/receipts:status` without the user asking twice.

---

## N. OVERRIDE / BREAK-GLASS MODEL

Fail-closed verification infrastructure that cannot be bypassed is infrastructure that gets uninstalled.

```
/receipts:override AUTH-42 --reason "CI runner offline; verified manually on staging"
```

### N.1 Rules `[DD]`

1. **Requires an interactive human confirmation**, not just a flag. An agent must not be able to invoke it; `[DD]` the broker rejects an override whose invoking process reports an `agent_id`, and the plugin's own PreToolUse hook denies `Bash(receipts override *)`.
2. **Requires a non-empty reason.** No default, no placeholder accepted.
3. **Records:** `overrideId`, `taskId`, `actor`, `reason`, `timestamp`, `fingerprint`, and the **complete unmet-claims list at the moment of override**. Recording *what was unproven* is the entire point.
4. **Scoped to one task and one fingerprint.** `[DD]` An override dies when the code state changes, exactly like evidence. There are no standing overrides.
5. **Never displays as proven.** The task state is `ADMITTED_WITH_OVERRIDE`. `/receipts:status` renders it distinctly, `receipts export` includes it, and any summary that collapses it into "verified" is a bug.
6. **Counted.** Override frequency is an evaluation metric (§V) and a product health signal: `[DD]` if users override constantly, the policy is wrong and the tool should say so rather than being ignored.

---

## O. CLAUDE HOOK MAPPING

`[DD]` **Two gates, not one**, because `TaskCompleted` is only available under experimental agent teams (§B.2.6).

| Event | Why this event | Input consumed | Broker operation | Blocking |
|---|---|---|---|---|
| `SessionStart` | Load repo context; surface unapproved recipe/policy edits early | `cwd`, `source` | `receipts session-init` → compute fingerprint, warn on recipe drift | No. `[VF]` Returns `additionalContext` |
| `PostToolUse` (matcher `Edit\|Write\|NotebookEdit`) | Observe that code changed | `tool_input.file_path`, `agent_id` | Mark fingerprint dirty (cheap flag only) | `[VF]` Cannot block. Runs with `async: true` so it never adds latency |
| `PostToolBatch` | One fingerprint recompute per parallel batch instead of per file | batch result | `receipts fingerprint --recompute`; re-evaluate stale claims | `[VF]` *Can* block, but `[DD]` **we do not block here.** Blocking mid-loop on staleness would be maddening |
| `SubagentStart` | Provenance: which worker is about to act | `agent_id`, `agent_type` | Open a worker session record | `[VF]` No blocking; context only |
| `SubagentStop` | Attribute claims to a worker; capture its final assertion | `agent_id`, `last_assistant_message` | Record `Claim(assertedBy=agent)` as `UNPROVEN` | `[DD]` Do **not** block. Blocking a subagent's exit to run a suite is the wrong place |
| **`TaskCompleted`** | **Primary semantic admission gate** | task id, `team_name` (deprecated), session fields | `receipts admit --task <id>` | **`[VF]` Exit 2 prevents the task being marked complete.** Blocks when `BLOCK` |
| **`PreToolUse`** (matcher `Bash`) | **Level-2 action gate** on merge/push | `tool_input.command` | Parse for `git merge` / `git push`; consult admission | **`[VF]` Exit 2 blocks, or JSON `permissionDecision: "deny"`** |
| `PreToolUse` (matcher `Edit\|Write`) | Protect recipe/policy/ledger from agent edits | `tool_input.file_path` | Deny writes to `.receipts/policy.yaml`, `.receipts/recipes.yaml` | Deny |
| `WorktreeCreate` | Bind a task to its workspace | worktree request | Record workspace identity | `[VF]` **Any non-zero exit fails worktree creation** — `[DD]` therefore this handler must be trivial, wrapped in a catch-all, and **always exit 0**; a broker bug here would break the user's worktrees |
| `WorktreeRemove` | Close the workspace record | path | Mark workspace closed | `[VF]` No decision control |
| `Stop` | Advisory summary at end of turn | `last_assistant_message` | Emit a one-line unproven-claims summary | `[DD]` **Non-blocking**, `additionalContext` only |

### O.1 Deliberate non-uses `[DD]`

- **`UserPromptSubmit`** — not used. `[VF]` It has a 30-second default timeout and blocks model processing until it completes; putting a verification broker on the critical path of every prompt is a self-inflicted wound.
- **`SessionEnd`** — not used for cleanup. `[VF]` SessionEnd hooks share a 1.5-second budget. Cleanup happens lazily on next session start.
- **`PermissionRequest`** — not used. Admission is a semantic decision, not a permission decision, and overloading the permission surface would confuse both systems.

### O.2 Hook implementation constraints, verified

- `[VF]` Use **exec form** (`command` + `args`) for every handler, since all reference `${CLAUDE_PLUGIN_ROOT}`: "each `args` element is one argument exactly as written," with no shell. This removes an entire class of quoting and injection bugs.
- `[VF]` Do **not** rely on the `if` filter for the merge/push gate: the docs state it "fails open, running your hook regardless of pattern, when the Bash command can't be parsed," and explicitly recommend the permission system for hard enforcement. `[DD]` So: match broadly on `Bash`, parse inside the broker, **and** ship a companion `deny` permission rule.
- `[VF]` Choose exit codes **or** JSON, never both — "Claude Code only processes JSON on exit 0." `[DD]` Convention: `PreToolUse` uses JSON (`permissionDecision`), `TaskCompleted` uses exit 2 + stderr (its documented decision pattern is exit-code based).
- `[VF]` Keep every hook-facing string under the **10,000-character** output cap; put detail behind `rawLogRef` and a `/receipts:show` command.
- `[VF]` Note for later telemetry work: "Claude Code removes `OTEL_*` exporter variables from every subprocess it spawns, including hooks," so a hook cannot inherit the session's OTel configuration. Telemetry export is deferred (§X).

---

## P. REVIEW PROVIDER INTERFACE

`[DD]` Deliberately tiny. Four operations. This is not an AgentAdapter zoo — providers cannot start sessions, cannot write, cannot delegate, and cannot spawn anything.

```
interface ReviewProvider {
  id: string
  vendor: string                       # runtime attribute, used by policy
  health(): { ok: bool, detail: string }
  capabilities(): {
      structuredOutput: bool
      maxDiffBytes: int
      readOnlyEnforced: bool
      resume: bool
  }
  review(req: ReviewRequest): Promise<ReviewResult>
  cancel(reviewId): void
}
```

### P.1 Concrete invocations (verified surfaces)

**Codex** `[VF]` — `codex exec` is the non-interactive mode; "By default, `codex exec` runs in a read-only sandbox"; `--json` makes "stdout … a JSON Lines (JSONL) stream of every event"; `--output-schema` supports structured output (and per the docs "currently requires a model from the gpt-5 family" and "cannot be combined with `codex exec resume`"); `-o/--output-last-message` writes the final message to a file; `--skip-git-repo-check`, `--ignore-user-config` and `--ignore-rules` exist for controlled automation. (developers.openai.com/codex/noninteractive.)
`[DD]` Invocation shape: `codex exec --sandbox read-only --json --output-schema <finding-schema.json> -o <out> "<review prompt>"`. `readOnlyEnforced: true`. `[VF]` Caution recorded in the provider notes: `--full-auto` overrides an explicit `--sandbox read-only`, so **never pass `--full-auto`**.

**Gemini** `[VF]` — headless via `-p/--prompt`; `--output-format json` (and `stream-json`); a `--sandbox` flag; `--model`; `--allowed-tools`; `--include-directories`. `[VF]` Known caveat: issue #11184 reports `--output-format json` returning the model's JSON wrapped in a fenced code block inside `.response`, i.e. the wrapper is structured but the payload may not be — `[DD]` so the Gemini provider must defensively strip fences and set `parseOk: false` rather than guessing. `capabilities().structuredOutput: false` until verified otherwise on the installed version.

**Claude (same-vendor independent session)** `[VF]` — `claude -p "<prompt>" --output-format json` returns a structured result envelope. `[DD]` Used when `distinct_vendor: off`, or as the `preferred`-mode fallback. Runs as a **separate process with no shared context**, which buys session independence but not model independence — and the admission record says so.

### P.2 Rules `[DD]`

- Model names and binaries live in `.receipts/providers.yaml`. **No vendor string appears in a code path**; the policy engine only ever compares `vendor` attributes for equality.
- A provider that fails health check is skipped, and the skip is recorded as a `downgrade` on the Admission — never silently.
- Provider output is *always* wrapped in `ReviewResult` with `parseOk`. Malformed output ⇒ claim stays `UNPROVEN`.
- `[DD]` Timeouts are enforced by the broker, not the provider.

---

## Q. BROKER ARCHITECTURE

### Q.1 Topology decision

`[DD]` **MVP is a CLI, not a daemon.** `plugin hooks → bin/receipts (short-lived process) → SQLite`.

Justification:
- The trust boundary that matters is "Claude Code is not the evidence authority." A separate short-lived process satisfies that completely; a daemon adds nothing to it.
- `[VF]` Command hooks default to a 600-second timeout, so a synchronous test run inside a hook is well within budget.
- There is no long-lived state to hold: SQLite *is* the state.
- A daemon introduces lifecycle bugs (stale sockets, version skew after plugin update, orphaned processes) that would dominate early development.

`[DD]` **V2 adds an optional daemon** only if one of these is observed: (a) fingerprint recomputation cost on large repos justifies an in-memory cache; (b) concurrent teammates cause SQLite contention beyond what WAL handles; (c) live-updating status UI is wanted.

### Q.2 Internal decomposition

`[DD]` The core must be deployment-independent so the CLI/daemon decision stays reversible:

```
core/                      # pure, no process assumptions
  fingerprint/             # git plumbing → CodeStateFingerprint
  ledger/                  # append-only event store + projections
  policy/                  # admit() — pure function, heavily unit-tested
  claims/                  # claim state derivation
  integrity/               # test-diff signals, hash chain
adapters/
  runner/                  # recipe execution → ExecutionReceipt
  providers/               # ReviewProvider implementations
  git/
entry/
  cli.ts                   # bin/receipts — the only thing hooks call
  (daemon.ts)              # V2
```

### Q.3 Concurrency `[DD]`

SQLite in **WAL mode**, `busy_timeout` set, one transaction per broker invocation. Under agent teams, multiple teammates are separate OS processes, so per-invocation transactions are the correct model. A per-`(repoId, recipeKey)` advisory lock file prevents two brokers running the same suite simultaneously; the second waits and then finds valid evidence rather than duplicating the run — `[DD]` which is also the cheapest possible win for the cache story.

---

## R. STORAGE SCHEMA

`[DD]` Location: `${CLAUDE_PLUGIN_DATA}/receipts/<repoId>/` — outside the repo (§D.3.1).
Files: `ledger.db`, `logs/`, `diffs/`.

```sql
-- append-only spine
CREATE TABLE events (
  seq            INTEGER PRIMARY KEY AUTOINCREMENT,
  ts             TEXT NOT NULL,
  kind           TEXT NOT NULL,         -- TASK_OPENED | CLAIM_ASSERTED | RECEIPT_RECORDED
                                        -- | REVIEW_RECORDED | ADMISSION_EVALUATED
                                        -- | OVERRIDE_GRANTED | WAIVER_GRANTED
                                        -- | RECIPE_APPROVED | POLICY_AMENDED
  payload        TEXT NOT NULL,         -- canonical JSON
  prev_hash      TEXT NOT NULL,
  hash           TEXT NOT NULL
);

CREATE TABLE tasks (
  task_id TEXT PRIMARY KEY, repo_id TEXT, title TEXT,
  baseline_sha TEXT, declared_paths TEXT, policy_profile TEXT,
  required_claims TEXT, state TEXT, external_ref TEXT,
  created_at TEXT, updated_at TEXT
);

CREATE TABLE claims (
  claim_id TEXT PRIMARY KEY, task_id TEXT NOT NULL, type TEXT NOT NULL,
  asserted_by_agent_id TEXT, asserted_by_agent_type TEXT, asserted_at TEXT,
  status TEXT NOT NULL,                 -- UNPROVEN|PROVED|REJECTED|STALE|WAIVED
  proved_at TEXT, proved_against_fingerprint TEXT,
  FOREIGN KEY(task_id) REFERENCES tasks(task_id)
);

CREATE TABLE receipts (
  receipt_id TEXT PRIMARY KEY, claim_id TEXT NOT NULL,
  recipe_key TEXT, recipe_digest TEXT,
  repo_id TEXT, baseline_sha TEXT, head_sha TEXT,
  working_tree_digest TEXT, fingerprint TEXT NOT NULL,
  argv TEXT, cwd TEXT, resolved_executable TEXT,
  started_at TEXT, finished_at TEXT, duration_ms INTEGER,
  exit_code INTEGER, timed_out INTEGER,
  stdout_digest TEXT, stderr_digest TEXT, raw_log_ref TEXT,
  runner_version TEXT, invoked_by_agent_id TEXT, parsed TEXT
);

CREATE TABLE reviews (
  review_id TEXT PRIMARY KEY, claim_id TEXT NOT NULL,
  fingerprint TEXT NOT NULL, profile TEXT,
  provider_id TEXT, vendor TEXT, model_reported TEXT,
  status TEXT, parse_ok INTEGER,
  started_at TEXT, finished_at TEXT, raw_ref TEXT, diff_ref TEXT
);

CREATE TABLE findings (
  finding_id TEXT PRIMARY KEY, review_id TEXT NOT NULL,
  severity TEXT, category TEXT, path TEXT, line INTEGER,
  summary TEXT, rationale TEXT, resolves_claim_id TEXT
);

CREATE TABLE code_evidence (
  code_evidence_id TEXT PRIMARY KEY, task_id TEXT, fingerprint TEXT,
  baseline_sha TEXT, head_sha TEXT,
  changed_paths TEXT, insertions INTEGER, deletions INTEGER,
  test_files_added INTEGER, test_files_modified INTEGER, test_files_deleted INTEGER,
  diff_ref TEXT
);

CREATE TABLE admissions (
  admission_id TEXT PRIMARY KEY, task_id TEXT, fingerprint TEXT,
  decision TEXT, unmet TEXT, downgrades TEXT,
  policy_digest TEXT, override_id TEXT, evaluated_at TEXT
);

CREATE TABLE overrides (
  override_id TEXT PRIMARY KEY, task_id TEXT, actor TEXT, reason TEXT,
  fingerprint TEXT, unmet_at_override TEXT, granted_at TEXT
);

CREATE INDEX idx_claims_task     ON claims(task_id);
CREATE INDEX idx_receipts_fp     ON receipts(fingerprint);
CREATE INDEX idx_reviews_fp      ON reviews(fingerprint);
CREATE INDEX idx_admissions_task ON admissions(task_id, evaluated_at);
```

`[DD]` `events` is the source of truth; the other tables are projections that can be rebuilt from it. This makes `receipts verify-ledger` (recompute the hash chain, rebuild projections, diff) a real, demonstrable command rather than a claim.

---

## S. SECURITY BOUNDARIES

### S.1 Claude Code layer
Claude Code is a **client of** the broker, never an authority. It can request evaluation and it can be blocked, but it cannot write evidence. `[VF]` Because hooks now fire inside subagents with `agent_id`/`agent_type`, the same gates apply to delegated work as to the main thread — this is what makes the design implementable at all, and it did not use to be true.

### S.2 Broker layer
Single writer. Ledger outside the repo tree. Recipes come only from an approved file (§L). All command execution is **exec-form with an explicit argv array** — `[DD]` the broker never passes a string to a shell, so a recipe cannot smuggle `&& curl … | sh`. Environment is allowlisted. Absolute, realpath-resolved `cwd`.

### S.3 Runtime layer (verification commands)
Here is the uncomfortable truth, stated rather than hidden: **running a project's test suite is executing the project's code.** A malicious repository whose `pnpm test` exfiltrates secrets does so under the developer's own credentials, and Receipts does not prevent this. `[DD]` Mitigations that *are* in scope: recipes require human approval (so the command is at least seen once); recipe changes invalidate evidence and re-prompt; and V2 may run recipes under the OS sandbox. `[DD]` MVP posture: **UNSOLVED and documented**, not hand-waved.

### S.4 Test-modification attacks
`TESTS PASS ≠ IMPLEMENTATION CORRECT`, most obviously when the agent weakened the tests.

`[DD]` **MVP exposes; policy decides whether it blocks.** Recorded on every task's `code_evidence`:
- test files added / modified / **deleted** (matched against `test_globs` declared in the recipe file)
- net change in test file count
- whether `.receipts/recipes.yaml` or `.receipts/policy.yaml` appear in the diff (these are also write-denied to agents, so their appearance is itself a signal)
- `[DD]` where the runner output is parseable, the **test-count delta** between the current receipt and the previous receipt for the same recipe key — a suite that went from 240 tests to 190 while staying green is the single loudest available signal, and it costs one integer.

Policy: `on_test_deletion: expose` (default in `STANDARD`) surfaces it in the admission record and forces `include_test_diff` on the review request; `block` (default in `HIGH_ASSURANCE`) makes it an unmet claim requiring an override.

`[DD]` **Mutation testing is explicitly out of scope for MVP.** It is the theoretically right answer and the wrong first move: minutes-to-hours of runtime, per-language tooling, and noisy results. Revisit only if §V shows the test-manipulation task escaping every arm.

### S.5 Enforcement scope — stated honestly

`[DD]` This section must appear near the top of the README, not buried.

| Level | What it gates | Status | Honest guarantee |
|---|---|---|---|
| **L1** | Claude Code task completion (`TaskCompleted`) | MVP, when agent teams enabled | Claude cannot mark a task done through the task list while claims are unmet |
| **L2** | Claude-Code-mediated `git merge` / `git push` (`PreToolUse` + deny rule) | MVP | Claude cannot merge or push **through Claude Code** while blocked |
| **L3** | Git `pre-commit` / `pre-push` hooks | Deferred | Would cover the developer's own terminal — but is locally bypassable with `--no-verify` |
| **L4** | CI required status check on a protected branch | Deferred | The only level that is actually **not** bypassable, because the ledger export is verified server-side |

> **Receipts does not make a bad merge impossible. It makes it impossible for Claude Code to perform one silently.** A human in another terminal can always run `git merge`.

`[DD]` The L4 path is why `receipts export` (a portable, hash-chained JSON bundle) is in the MVP even though nothing consumes it yet: it is the seam that makes the roadmap credible.

### S.6 Prompt injection
`[VF]` The hooks docs already warn that hook-injected text "framed as out-of-band system commands can trigger Claude's prompt-injection defenses." `[DD]` Therefore all `additionalContext` Receipts emits is written as **factual statements** ("AUTH-42: TESTED is stale as of 14:22; 2 files changed"), never as imperatives. And structurally: an injected agent's best available attack is to *ask the broker to run an approved recipe*, which is exactly the safe path.

---

## T. EXACT MVP

**Scope: one repo, one machine, one language ecosystem in the demo, four claim types.**

**In:**
1. `receipts init` — creates the ledger; scaffolds `.receipts/recipes.yaml` and `.receipts/policy.yaml`.
2. CodeStateFingerprint with all four fields (§G).
3. Recipe runner producing ExecutionReceipts with the MVP field subset (§I.2), for keys `test` and `lint`.
4. Claim tracking for `IMPLEMENTED`, `TESTED`, `LINT_CLEAN`, `REVIEWED`.
5. Whole-tree staleness (§M.1).
6. `admit()` with `LIGHT` / `STANDARD` profiles; `HIGH_ASSURANCE` shipped as config only.
7. One ReviewProvider — **Codex** (`codex exec --sandbox read-only --json`) — plus the `claude -p` same-vendor fallback. `[DD]` Gemini is a second provider only if it costs under a day, given the JSON-fencing caveat (§P.1).
8. Hooks: `SessionStart`, `PostToolUse`(async), `PostToolBatch`, `SubagentStop`, `TaskCompleted`, `PreToolUse`(Bash + Edit/Write), `WorktreeCreate`/`Remove` — as mapped in §O.
9. Skills: `/receipts:status`, `/receipts:verify`, `/receipts:review`, `/receipts:override`, `/receipts:recipe`.
10. Test-integrity signals recorded and displayed; `on_test_deletion: expose`.
11. `receipts verify-ledger` and `receipts export`.

**Out of MVP:** daemon, scoped/dependency staleness, mutation testing, OTel export, signatures, multi-repo, remote reviewers, web UI, CI integration, benchmark-learned routing.

---

## U. EXACT 3-MINUTE DEMO

Setup (off-camera): a small TypeScript repo with a real suite; `.receipts/policy.yaml` set to `STANDARD`; Codex authenticated; agent teams enabled.

**0:00 — 0:20 · The claim**
"Coding agents tell you tests pass. Claude Code can already run tests when a task completes. Receipts is about the gap between those two sentences."
`/receipts:status` → `AUTH-42 · OPEN · IMPLEMENTED UNPROVEN · TESTED UNPROVEN · LINT_CLEAN UNPROVEN · REVIEWED UNPROVEN`

**0:20 — 1:00 · Work happens, evidence is captured**
Claude implements refresh-token rotation in a worktree. Edits stream past. `/receipts:status` now shows `IMPLEMENTED PROVED` — with a fingerprint, a commit, and changed paths — captured automatically, nobody asked for it.

**1:00 — 1:30 · The catch (the beat that sells it)**
Claude asserts the suite passes. `/receipts:verify AUTH-42` → the broker runs it *itself*.
```
TESTED  REJECTED   receipt r-118   exit 1
  auth/rotate.test.ts  ✗ rejects reused refresh token
  237 passed, 1 failed
```
"The agent said green. The ledger says red. The ledger ran it."

**1:30 — 2:05 · Independent review, as data**
Claude fixes it; `TESTED` goes `PROVED`. `/receipts:review AUTH-42` dispatches to Codex, read-only, different vendor. A structured finding returns:
```
REVIEWED  REJECTED   codex/<model as reported>   HIGH
  src/auth/rotate.ts:64  refresh-token reuse window
```
Routed back as a finding object, not a paragraph. Claude fixes the specific finding.

**2:05 — 2:35 · Staleness — the part `TaskCompleted` cannot do**
Before re-verifying, edit one unrelated line and save.
```
TESTED  STALE   proved at 3e88…c4, tree is now 9f2c…a1
                caused by: src/auth/store.ts
```
"Nothing failed. The evidence simply stopped being about this code."
Undo the edit → `TESTED` returns to `PROVED` **without re-running anything**. "That's the cache. Verification got cheaper, not just stricter."

**2:35 — 2:55 · Admission is enforced**
Re-verify and re-review to green, then attempt a merge with a sibling task still unproven:
```
Blocked by Receipts: AUTH-43 · REVIEWED UNPROVEN
```
Clear it → merge proceeds. Then `/receipts:override AUTH-44 --reason "…"` → the status line reads **`ADMITTED_WITH_OVERRIDE`**, never "verified."

**2:55 — 3:00 · The receipts**
`receipts verify-ledger` → chain intact, projections rebuilt, zero drift. Open one receipt: argv, cwd, commit, tree digest, exit code, output digest.
"Every 'done' in this repo points at one of these."

---

## V. EVALUATION / ABLATION DESIGN

`[DD]` **All numbers below are targets to be measured. None are results. Nothing in this section may be quoted as an outcome until the harness has run.**

### V.1 Balanced task set — 12 tasks, 6 defective / 6 clean

The prior design planted a defect in every task, which biases the benchmark toward any verifier that blocks. Fixed:

| # | Task | Class |
|---|---|---|
| 1 | Refresh-token reuse window | security defect |
| 2 | Pagination off-by-one | logic defect |
| 3 | Cache write race | concurrency defect |
| 4 | Migration missing a backfill | migration defect |
| 5 | Missing input validation on a public endpoint | validation defect |
| 6 | Fix implemented by weakening/deleting tests | test-manipulation |
| 7 | Correct token rotation | clean counterpart to 1 |
| 8 | Correct pagination | clean counterpart to 2 |
| 9 | Correct cache with proper locking | clean counterpart to 3 |
| 10 | Correct migration with backfill | clean counterpart to 4 |
| 11 | Correct validation | clean counterpart to 5 |
| 12 | Correct fix with tests *strengthened* | clean counterpart to 6 |

Tasks 6 and 12 are the pair that matters most: a system that blocks 6 but also blocks 12 has learned "test changes are bad," not "test weakening is bad."

### V.2 Arms

| Arm | Configuration |
|---|---|
| A | Claude Code, normal workflow, no gating |
| B | Claude Code + native `TaskCompleted` hook running the test suite |
| C | Receipts, deterministic evidence only (no review) |
| D | Receipts, deterministic + **same-provider** independent review |
| E | Receipts, deterministic + **cross-vendor** review |
| F *(optional)* | Claude Code + official `codex-plugin-cc` review flow |

Arm B is the critical control. `[DD]` **If C does not beat B, the ledger is not earning its complexity** — that is the whole point of including it, and it must be reported honestly if it happens.

### V.3 Metrics

| Metric | Definition |
|---|---|
| Defect escape rate | planted defect reaches ADMITTED (defective tasks only) |
| False completion claim rate | agent asserted a claim that broker evidence contradicts |
| False block rate | clean task blocked (clean tasks only) — **the metric that keeps the tool installed** |
| Review false-positive rate | blocking findings on clean tasks |
| Wall-clock overhead | vs arm A |
| Token/cost overhead | vs arm A, including reviewer tokens |
| Human interventions | prompts requiring a person |
| Stale-evidence detections | times evidence was correctly invalidated |
| **Verification cache hit rate** | claims admitted from valid stored evidence without re-running — the cost story |
| Override frequency | overrides per admitted task |

### V.4 Method

`[DD]` Fixed prompts, fixed seeds where available, ≥3 runs per (task, arm) since agent runs are stochastic. Report per-task outcomes and medians. **Do not compute p-values on n=36 per arm.** Report the ablation as a difference table with explicit ranges and say plainly that the sample supports direction, not significance.

---

## W. FAILURE CRITERIA

`[DD]` Thresholds are **design targets**, not measurements. Abandon or radically simplify if:

1. **Native parity.** Arm C's defect-escape rate is within ~5 percentage points of arm B across the 6 defective tasks. → The ledger adds ceremony, not safety. *Response: cut to a `TaskCompleted` hook plus a staleness warning; abandon the product framing.*
2. **Provenance without payoff.** Receipt provenance beyond `(fingerprint, argv, exitCode)` never changes an admission decision in any run. *Response: cut the receipt to those fields; drop the provenance narrative.*
3. **Unacceptable false blocking.** Cross-vendor review (arm E) blocks >2 of the 6 clean tasks, or arm E's false-block rate exceeds arm D's by more than ~15 points. → `distinct_vendor: required` is a liability. *Response: demote cross-vendor review to opt-in advisory.*
4. **Staleness noise.** Whole-tree invalidation triggers so often that median re-runs per task exceed ~3, and scoped invalidation (§M.3.2) does not bring it under ~1.5. *Response: staleness becomes a warning, not a gate.*
5. **Cost.** Median wall-clock overhead >100% versus arm A **and** cache hit rate <30%. → The cache story fails and the tool is a tax.
6. **Rejection in practice.** In self-use over two weeks, override frequency exceeds ~20% of admitted tasks. → Users are routing around the gate, which is the real verdict.

---

## X. THINGS EXPLICITLY DEFERRED

1. Long-running daemon (§Q.1) — until measured need.
2. Scoped, then dependency-aware, staleness (§M.3).
3. Mutation testing (§S.4).
4. Cryptographic signatures / external trust anchor (§D.3.6).
5. `toolVersionDigest`, `envDigest`, host/user fields (§I.2).
6. OTel `gen_ai.*` export — `[VF]` blocked in part because Claude Code strips `OTEL_*` from spawned subprocesses; needs its own design.
7. Git-hook (L3) and CI required-check (L4) enforcement (§S.5).
8. Multi-repo and monorepo-package scoping.
9. Remote/hosted reviewers; everything stays local.
10. Benchmark-learned routing, cost-aware provider selection.
11. Web/TUI dashboard beyond `/receipts:status`.
12. `TYPECHECKED`, `BUILD_SUCCEEDED`, `COVERAGE_MET`, `MIGRATION_VALIDATED`, `API_COMPATIBLE`, `SECURITY_REVIEWED` as *exercised* claim types — they work via config, but are not demoed or evaluated.

---

## Y. REPOSITORY STRUCTURE

```
receipts/
├── .claude-plugin/
│   └── plugin.json
├── hooks/
│   └── hooks.json                    # exec-form handlers only
├── skills/
│   ├── receipts-status/SKILL.md
│   ├── receipts-verify/SKILL.md
│   ├── receipts-review/SKILL.md
│   ├── receipts-override/SKILL.md
│   └── receipts-recipe/SKILL.md
├── agents/
│   └── receipts-reviewer.md          # read-only tools; used for the claude-session provider
├── bin/
│   └── receipts                      # the only entry point hooks invoke
├── src/
│   ├── core/{fingerprint,ledger,policy,claims,integrity}/
│   ├── adapters/{runner,providers,git}/
│   └── entry/cli.ts
├── schemas/
│   ├── recipe.schema.json
│   ├── policy.schema.json
│   ├── receipt.schema.json
│   └── finding.schema.json           # handed to codex --output-schema
├── eval/
│   ├── tasks/{01..12}/               # 6 defective, 6 clean, each with an oracle
│   ├── arms/
│   └── harness/
├── docs/
│   ├── ARCHITECTURE.md
│   ├── TRUST_MODEL.md                # incl. the "not tamper-proof" statement
│   ├── ENFORCEMENT_SCOPE.md          # L1–L4 table
│   ├── HOOK_MAPPING.md
│   ├── PROVIDERS.md
│   └── EVALUATION.md                 # empty of results until measured
└── README.md
```

### Y.1 On the name `[DD]`
"Receipts" is provisional and **likely to collide** — it is a common English word and `receipts`-named packages exist across ecosystems. Before adoption, run a collision check across GitHub, npm, PyPI, crates.io, and a plain web search, and record the result in the README. Candidates to check alongside it, chosen to name the actual abstraction: `admit`, `claimcheck`, `provenant`, `attestly`, `greenproof`. The name must survive the check; do not adopt one from memory.

---

## Z. DEPENDENCY-ORDERED IMPLEMENTATION MILESTONES

**M0 — Fingerprint + ledger spine**
Build: `repoId`, `headSha`, `workingTreeDigest`, `fingerprint`; SQLite schema; hash-chained `events`; projection rebuild; `receipts verify-ledger`.
*Accept:* fingerprint changes on any tracked-file edit and returns to its prior value on revert; ignored files never affect it; `verify-ledger` detects a manually mutated row; projections rebuild byte-identically from `events`.

**M1 — Recipes + runner + receipts**
Build: recipe schema, human-approval flow, `recipeDigest`; exec-form runner; ExecutionReceipt (MVP fields); log storage.
*Accept:* a `test` recipe runs and produces a receipt with correct argv/cwd/exit/digests; an agent-proposed recipe **cannot** take effect without human approval; changing a recipe invalidates prior evidence for that key.

**M2 — Claims + `admit()`**
Build: claim types, status derivation, whole-tree staleness, pure `admit()`, `LIGHT`/`STANDARD` profiles.
*Accept:* `admit()` unit tests cover PROVED/REJECTED/STALE/WAIVED and every unmet path; blocked admissions name the changed paths that caused staleness; `admit()` performs no I/O.

**M3 — Claude Code integration (L1 + L2)**
Build: plugin manifest; `hooks.json`; `SessionStart`, `PostToolUse`(async), `PostToolBatch`, `SubagentStop`, `TaskCompleted`, `PreToolUse`(Bash + Edit/Write); companion deny rules; `/receipts:status`, `/receipts:verify`.
*Accept:* `TaskCompleted` blocks (exit 2) with a useful stderr reason when unmet; a Claude-issued `git merge` is denied while blocked and permitted when admitted; agent edits to `policy.yaml`/`recipes.yaml`/the ledger are denied; `WorktreeCreate` handler **always exits 0** even when the broker errors; every hook string is under 10,000 characters; `PostToolUse` adds no measurable latency.

**M4 — Review providers**
Build: `ReviewProvider` interface; Codex provider (`codex exec --sandbox read-only --json --output-schema`); `claude -p` fallback; finding schema; structured routing of findings back to the implementer; `distinct_vendor` policy resolution and downgrade recording.
*Accept:* a malformed or timed-out provider response leaves the claim `UNPROVEN`, never `PROVED`; `model` is stored as reported, not as configured; a healthy different-vendor provider is chosen under `preferred`, and the fallback is recorded as a downgrade; the reviewer cannot write to the repo.

**M5 — Integrity signals + override**
Build: test-glob diff signals, test-count delta, `on_test_deletion` policy; `/receipts:override` with interactive confirmation and agent rejection; `ADMITTED_WITH_OVERRIDE` rendering; `receipts export`.
*Accept:* deleting a test file is visible in the admission record and forces `include_test_diff`; an override is impossible from an agent context; an overridden task is never rendered as verified anywhere in the UI or export; export is hash-verifiable by an independent script.

**M6 — Evaluation harness**
Build: 12 tasks with oracles; arms A–E (F optional); metric collection incl. cache hit rate; per-task reporting.
*Accept:* every task is reproducible from a clean checkout; ≥3 runs per (task, arm) complete unattended; the report distinguishes defective from clean tasks and refuses to print aggregate significance claims.

**M7 — Documentation**
Build: `ARCHITECTURE.md`, `TRUST_MODEL.md`, `ENFORCEMENT_SCOPE.md`, `HOOK_MAPPING.md`, `PROVIDERS.md`, `EVALUATION.md`, README with the honest scope sentence and the L1–L4 table above the fold.
*Accept:* a reader can state, from the README alone, exactly what Receipts proves and what it does not; `EVALUATION.md` contains no number that was not produced by M6.

---

## Falsifying the abstraction (as requested)

`[DD]` The CLAIM → EVIDENCE → POLICY → ADMISSION frame **fails** in three identifiable regimes, and the product should say so rather than pretend otherwise:

1. **When the property that matters is not mechanically checkable.** API ergonomics, naming, architectural fit, and "is this the right abstraction" produce no receipt. Receipts is silent on exactly the questions senior engineers care most about. Its answer is the ReviewClaim, and a ReviewClaim is explicitly not proof.
2. **When the check is flaky.** A non-deterministic suite makes `PROVED` a coin flip, and a coin flip laundered through a hash chain looks far more authoritative than it is. `[DD]` Mitigation to build in M1: record consecutive-run disagreement for the same fingerprint and surface a `FLAKY` warning on the claim rather than letting the last run win silently.
3. **When the recipe is a lie.** `pnpm test` that runs zero tests exits 0. Receipts will faithfully record a green receipt for a vacuous check. Human recipe approval and the test-count delta are partial answers; there is no complete one.

The abstraction survives because it is scoped to a real, common, and currently-unhandled failure — *the agent said the checks passed, and either they didn't, or they did but not for this code* — and because it degrades honestly at its edges rather than silently.
