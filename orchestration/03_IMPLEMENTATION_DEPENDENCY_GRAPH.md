# 03 — Implementation Dependency Graph

## Contract-first DAG

```text
ARCHITECTURE AUTHORITY
        |
        +--> CONTRACT-CORE-001 ------------------+
        +--> CONTRACT-LEDGER-001                 |
        +--> CONTRACT-STORAGE-001                v
        +--> CONTRACT-ERROR-001                [M0]
                                                 |
                          +----------------------+
                          v
   CONTRACT-RUNNER-001/002 + CONTRACT-PROCESS-001 + CONTRACT-EVIDENCE-001
                          |
                         [M1]
                          |
                          v
 CORE-002/003 + POLICY-001 + ADMISSION-001 + OVERRIDE-001
                          |
                         [M2]
                          |
                          v
              HOOKS-001/002 + current Claude docs
                          |
                         [M3]
                          |
                          v
          REVIEW-001/002 + current Codex/Claude docs
                          |
                         [M4]
                          |
                          v
          integrity signals + override + EXPORT-001
                          |
                         [M5]
                          |
                          v
                       [M6 EVAL]
                          |
                          v
                    [M7 DOCS/RELEASE]
```

## Parallelism

A2 managers may initialize in parallel after A1 bootstrap. They may research, validate current interfaces, write component specifications, and prepare proposed A3 task packets.

Implementation parallelism is narrower:
- M0 contains parallel CORE and LEDGER implementation work only after their shared contracts are frozen.
- M1 waits for M0 interfaces.
- M2 waits for M0 and M1.
- M3 waits for M2.
- M4 waits for M3's integration surface and M2's review/admission contracts.
- M5 waits for M4.
- M6 waits for the complete M0–M5 product behavior.
- M7 may draft structure earlier but cannot publish measured claims until M6 is complete.

## Start rule

For every proposed A3 task:

`A3_START_ALLOWED = contracts_frozen AND prerequisites_accepted AND blocking_open_issues_cleared AND workspace_assigned`

If false, the A2 must return a dependency request rather than an implementation prompt.

## Integration order

No A2 merges directly into another A2's worktree. A1 integrates accepted component outputs in milestone order after A4 review and component acceptance evidence.
