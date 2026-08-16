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

# TASK_CAPSULE_SPEC

**A Task Capsule is the complete context a RUNTIME-A3 receives.** It exists so an ephemeral worker can execute correctly without any part of the A1/A2 conversation.

If a task cannot be expressed as a capsule, it is not yet a well-defined task.

## Schema

```
TaskCapsule {
  # identity
  task_id                  string        immutable
  workstream_id            string
  parent_task_id           string?       set for repair attempts
  attempt_number           integer       1 for initial
  task_type                enum          IMPLEMENTATION | REPAIR | TEST | REFACTOR
                                         | MIGRATION | DOCS | INVESTIGATION | SECURITY_FIX

  # what
  objective                string        one outcome, one sentence
  acceptance_criteria      Criterion[]   machine-testable where possible
  non_goals                string[]      what this task explicitly does not do

  # code state
  baseline_sha             string        workstream branch baseline
  start_sha                string        exact commit the worktree was created from

  # boundaries
  allowed_write_paths      glob[]
  forbidden_write_paths    glob[]        explicit traps, not just "everything else"

  # context (references, never inlined blobs)
  relevant_context_refs    Ref[]         files/sections the task needs
  architecture_refs        Ref[]
  contract_refs            Ref[]         id + version

  # graph
  dependencies             task_id[]     all must be accepted before dispatch

  # routing
  quality_floor            enum          FRONTIER | BALANCED | ECONOMY
  required_capabilities    Capability[]  hard filter
  preferred_capabilities   Capability[]  soft ranking

  # verification
  verification_plan        Check[]       exact commands + expected outcome
  review_policy            ReviewPolicy  { required, distinct_provider, reviewer_floor }
  assurance_profile        enum          LIGHT | STANDARD | HIGH_ASSURANCE

  # budget
  cost_policy              CostPolicy    { max_cost, priority }
  time_budget              duration
  turn_budget              integer

  # workspace
  branch                   string        runtime-a3/<task-id>
  worktree                 path          assigned; never invented by the worker
  remote_publish_policy    enum          LOCAL_ONLY | PUSH_ON_ACCEPT | PUSH_ALWAYS

  # control
  stop_conditions          StopCondition[]
  handoff_schema           SchemaRef
  context_epoch            integer       capsule invalid if epoch advanced
}
```

## Design rules

1. **References, not blobs.** `relevant_context_refs` point at repository paths and sections. The worker reads them at `start_sha` — so it sees the real current text, not a possibly-stale copy embedded by a manager.
2. **Boundaries are explicit.** `forbidden_write_paths` names the specific traps (other workstreams, orchestrator state, contracts), because "everything else" is easy to rationalise around.
3. **Capabilities, not model names.** A capsule never names a model (I-17). Routing is the router's job.
4. **Verification is a plan, not a hope.** `verification_plan` carries exact commands; "run the tests" is not a plan.
5. **Epoch-stamped.** If the context epoch advanced, the capsule is stale and must be regenerated before dispatch — otherwise a worker acts on a superseded architecture.

## Criterion

```
Criterion { id, description, kind: DETERMINISTIC|SEMANTIC, check?: Check, rationale? }
```

`SEMANTIC` criteria are permitted only where the judgement is genuinely human-like, and must be marked so A4 knows it is exercising judgement rather than running a check.

## Generation

Built by RUNTIME-A2 from the DAG. Validated before dispatch: schema valid; dependencies accepted; write paths inside the workstream; no path collision with a concurrently running task; epoch current; budgets available; verification plan non-empty for code tasks.

A capsule failing validation is never dispatched — dispatching an underspecified task wastes frontier compute and produces work nobody can audit.
