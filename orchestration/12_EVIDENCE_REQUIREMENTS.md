# 12 — Evidence Requirements

| Evidence class | Required contents | Acceptance owner |
|---|---|---|
| A3 implementation evidence | Task ID; contract versions; baseline; declared paths; final diff; test command/result; relevant fixtures; known limitations; no hidden scope expansion. | Owning A2 |
| A4 review evidence | Independent session identity; exact commit/diff reviewed; findings with severity and file/line; contract/architecture/security/test assessment; approve/reject. | Owning A2 + A1 gate |
| External-interface evidence | Official documentation URL; access date; exact relevant field/flag/event; local version smoke when behavior is version-sensitive. | A2-CLAUDE-INTEGRATION / A2-REVIEW |
| Storage evidence | Migration/schema fixture; hash-chain mutation test; projection rebuild equality; WAL/concurrency test. | A2-LEDGER |
| Runner evidence | Exact argv/cwd/exit/digests; unapproved-recipe rejection test; timeout test; recipe-change staleness test. | A2-RUNNER |
| Security evidence | Denied protected edit; agent override rejection; no shell-string path; reviewer write denial; prompt-injection-safe factual output. | A2-INTEGRITY-SECURITY |
| Evaluation evidence | Clean reset artifact; oracle; arm config; raw run outputs; measured metrics; repetition count; failure log. | A2-EVALUATION |
| Release evidence | Install smoke; demo smoke; exported bundle verification; README proof/non-proof sentence; L1-L4 table; collision check; measured-only eval report. | A2-DOCS-RELEASE |

## Evidence quality rule

A prose statement such as "tests pass", "reviewed", or "looks correct" is not implementation-program acceptance evidence. Acceptance must point to a reproducible artifact, command output, fixture, diff, or independent review record.

## Benchmark integrity rule

All thresholds in architecture V/W remain design targets until M6 runs. No build-control or release document may convert them into observed results.
