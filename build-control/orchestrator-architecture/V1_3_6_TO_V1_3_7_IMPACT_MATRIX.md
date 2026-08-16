<!--
MultiAgent Orchestrator Architecture — V1.3.7 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
-->
# V1.3.6 to V1.3.7 impact matrix

| Area | V1.3.7 closure | Evidence | Architectural effect |
|---|---|---|---|
| Reproduction | Fresh V1.3.6 extraction without `jsonschema` exited 0 and falsely reported a pass; isolated validation environment with `jsonschema 4.26.0` reproduced fixture 02 acceptance: `HOST_CAPABILITY_INVALID_STATE_ACCEPTED = 1`, exit 1. | `evidence/VALIDATION_REQUIREMENTS.md`; fixture 02 | None — validation defect only. |
| Dependency gate | Missing `jsonschema` now fails closed with explicit dependency/validation-executed indicators. | `MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED`; `VALIDATION_DEPENDENCY_FAIL_OPEN` | None. |
| Host capability | COMPLETE reports reject installed plugin without support, configured/enabled hooks without support, and required-but-unknown hook trust. PARTIAL reports retain null unknowns. | HostCapabilityReport schema; fixtures 02, 11–13, 08 | None — no state-machine redesign. |
| Final bytes | Build reopens the archive, validates source/package/regression gates from it, compares report gate objects, and writes a sibling final-byte attestation. | `evidence/build_package.py` | None. |

No changes were made to ExecutionGraph, FREE/PRO, ProductEntitlement, provider policy, host authority/freshness, Model Intelligence, runtime roles, SQLite, open-core boundaries, BUILD-A2 topology/DAG, or host architecture.
