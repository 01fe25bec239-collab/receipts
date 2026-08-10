<!--
Receipts — Repository Installation Map (A2 package V2)
Issued by: A1-BOOTSTRAP
Issued: 2026-08-10
Install path: build-control/a2/REPOSITORY_INSTALLATION_MAP.md
-->

# REPOSITORY_INSTALLATION_MAP

**Purpose.** This package ships as a ZIP but must live in the repository. This file is the authoritative mapping from ZIP path to repository path, so that installation is mechanical and verifiable rather than improvised.

**Installation root:** `build-control/a2/`

## Mapping

| ZIP path | Repository path |
|---|---|
| `A2_INITIALIZATION_INDEX.md` | `build-control/a2/A2_INITIALIZATION_INDEX.md` |
| `A2_INITIALIZATION_ORDER.md` | `build-control/a2/A2_INITIALIZATION_ORDER.md` |
| `A2_FIRST_TASK_ISSUANCE_ORDER.md` | `build-control/a2/A2_FIRST_TASK_ISSUANCE_ORDER.md` |
| `A2_CONSOLIDATION_DECISION.md` | `build-control/a2/A2_CONSOLIDATION_DECISION.md` |
| `A2_OWNERSHIP_REMAP.md` | `build-control/a2/A2_OWNERSHIP_REMAP.md` |
| `A2_BOOTSTRAP_HANDOFF_TEMPLATE.md` | `build-control/a2/A2_BOOTSTRAP_HANDOFF_TEMPLATE.md` |
| `REPOSITORY_INSTALLATION_MAP.md` | `build-control/a2/REPOSITORY_INSTALLATION_MAP.md` |
| `PACKAGE_MANIFEST.sha256` | *not installed* — it describes the ZIP layout only |
| `INSTALL_MANIFEST.sha256` | `build-control/a2/INSTALL_MANIFEST.sha256` |
| `A2_FOUNDATION/A2_FOUNDATION_MANAGER.md` | `build-control/a2/foundation/A2_FOUNDATION_MANAGER.md` |
| `A2_FOUNDATION/CONTEXT_MANIFEST.md` | `build-control/a2/foundation/CONTEXT_MANIFEST.md` |
| `A2_FOUNDATION/OWNERSHIP_MANIFEST.md` | `build-control/a2/foundation/OWNERSHIP_MANIFEST.md` |
| `A2_FOUNDATION/FIRST_MANAGER_TASK.md` | `build-control/a2/foundation/FIRST_MANAGER_TASK.md` |
| `A2_VERIFICATION/*` | `build-control/a2/verification/*` |
| `A2_CLAUDE_INTEGRATION/*` | `build-control/a2/claude-integration/*` |
| `A2_TRUST/*` | `build-control/a2/trust/*` |
| `A2_QUALITY_RELEASE/*` | `build-control/a2/quality-release/*` |

### Folder-name mapping

ZIP folders are uppercase for readability; repository folders use the manager **slug**, matching the `build-control/a2/<slug>/` convention already used for manager status files.

| ZIP folder | Repository folder | Manager | Integration branch |
|---|---|---|---|
| `A2_FOUNDATION/` | `build-control/a2/foundation/` | A2-FOUNDATION | `a2/foundation` |
| `A2_VERIFICATION/` | `build-control/a2/verification/` | A2-VERIFICATION | `a2/verification` |
| `A2_CLAUDE_INTEGRATION/` | `build-control/a2/claude-integration/` | A2-CLAUDE-INTEGRATION | `a2/claude-integration` |
| `A2_TRUST/` | `build-control/a2/trust/` | A2-TRUST | `a2/trust` |
| `A2_QUALITY_RELEASE/` | `build-control/a2/quality-release/` | A2-QUALITY-RELEASE | `a2/quality-release` |

File names inside each folder are unchanged.

## Resulting repository layout

```
receipts/
├── Receipts_Final_Architecture.md          # frozen at CONTRACT_FREEZE_SHA
├── architecture-decisions/
│   └── ARCHITECTURE_DEVIATION_REQUEST_001.md
├── contracts/
│   ├── CONTRACT_INDEX.md
│   └── CONTRACT_*.md                       # 21 frozen contracts, 1.0.0
├── schemas/
│   └── SCHEMA_PLAN.md
├── orchestration/
│   └── 00…15_*.md                          # 16 frozen control files
└── build-control/
    └── a2/
        ├── A2_INITIALIZATION_INDEX.md
        ├── A2_INITIALIZATION_ORDER.md
        ├── A2_FIRST_TASK_ISSUANCE_ORDER.md
        ├── A2_CONSOLIDATION_DECISION.md
        ├── A2_OWNERSHIP_REMAP.md
        ├── A2_BOOTSTRAP_HANDOFF_TEMPLATE.md
        ├── REPOSITORY_INSTALLATION_MAP.md
        ├── INSTALL_MANIFEST.sha256
        ├── foundation/          { A2_FOUNDATION_MANAGER.md, CONTEXT_MANIFEST.md, OWNERSHIP_MANIFEST.md, FIRST_MANAGER_TASK.md }
        ├── verification/        { A2_VERIFICATION_MANAGER.md, … }
        ├── claude-integration/  { A2_CLAUDE_INTEGRATION_MANAGER.md, … }
        ├── trust/               { A2_TRUST_MANAGER.md, … }
        └── quality-release/     { A2_QUALITY_RELEASE_MANAGER.md, … }
```

Each manager later adds its seven status files to its own `build-control/a2/<slug>/` folder — the same folder as its definition files. Definition files are written by the active A1; status files are written by the manager.

## Ownership of installed files

| Path | Written by |
|---|---|
| `build-control/a2/*.md` — the **seven** program-level files: `A2_INITIALIZATION_INDEX.md`, `A2_INITIALIZATION_ORDER.md`, `A2_FIRST_TASK_ISSUANCE_ORDER.md`, `A2_CONSOLIDATION_DECISION.md`, `A2_OWNERSHIP_REMAP.md`, `A2_BOOTSTRAP_HANDOFF_TEMPLATE.md`, `REPOSITORY_INSTALLATION_MAP.md` | **The currently active A1 only.** No A2 manager may modify these. |
| `build-control/a2/INSTALL_MANIFEST.sha256` | The currently active A1 |
| `build-control/a2/<slug>/A2_*_MANAGER.md`, `CONTEXT_MANIFEST.md`, `OWNERSHIP_MANIFEST.md`, `FIRST_MANAGER_TASK.md` | **The currently active A1 only.** A manager reads its own definition files; it does not edit them. |
| `build-control/a2/<slug>/COMPONENT_STATUS.md`, `TASK_LEDGER.md`, `CONTRACT_STATE.md`, `OPEN_ISSUES.md`, `DEPENDENCY_REQUESTS.md`, `EVIDENCE_INDEX.md`, `DECISION_LOG.md` | **That manager only.** |

A manager that wants its own definition changed files a request to the active A1. It does not edit its own charter — a manager that can rewrite its own boundary does not have one.

## Two manifests, two layouts

The ZIP uses uppercase manager folders; the repository uses lowercase slugs. A single manifest cannot describe both, so the package ships **two**, and each is valid in exactly one place.

| Manifest | Describes | Run from | Paths look like |
|---|---|---|---|
| `PACKAGE_MANIFEST.sha256` | The portable ZIP exactly as packaged | The extracted ZIP root | `A2_FOUNDATION/A2_FOUNDATION_MANAGER.md` |
| `INSTALL_MANIFEST.sha256` | The installed repository representation | `build-control/a2/` | `foundation/A2_FOUNDATION_MANAGER.md` |

Both hash the **same underlying file contents** wherever a package file maps to an installed file; only the path prefixes differ. Neither manifest hashes itself, and `PACKAGE_MANIFEST.sha256` does not hash `INSTALL_MANIFEST.sha256` or vice versa.

**Never run `PACKAGE_MANIFEST.sha256` from `build-control/a2/`.** Its uppercase paths will not resolve after the folder rename, and a failure there means nothing about file integrity. `PACKAGE_MANIFEST.sha256` is not installed into the repository.

## Installation procedure

Performed by the human operator or the currently active A1. **Not performed by this package, and not performed now.**

**STEP 1** — Extract the package.

**STEP 2** — From the extracted package root:

```
sha256sum -c PACKAGE_MANIFEST.sha256
```

All entries must pass. Confirm the repository is on `main`, clean, and up to date with `origin/main`.

**STEP 3** — Copy and rename files into `build-control/a2/` exactly per the mapping above: program-level files at the root of `build-control/a2/`, manager folders renamed from `A2_FOUNDATION/` to `foundation/` and so on. Install `INSTALL_MANIFEST.sha256`; do **not** install `PACKAGE_MANIFEST.sha256`.

**STEP 4** — From the repository:

```
cd build-control/a2
sha256sum -c INSTALL_MANIFEST.sha256
```

All entries must pass. Copying and renaming must not alter content; a mismatch here means the copy was lossy or a file was edited in transit.

**STEP 5** — Only after **both** validations pass may the package be committed.

Beyond step 5, and **not authorized by this package**: push to `main` as part of the remaining orchestration work; then, after all remaining orchestration artifacts are committed and pushed, record the final `main` commit as `AGENT_SYSTEM_FREEZE_SHA`; only then may `A1-RUNTIME` be initialized against that SHA and formal authority transfer occur.

## Current installation state

| Item | State |
|---|---|
| `PACKAGE_MANIFEST.sha256` | Present in the ZIP; verified at package-generation time |
| `INSTALL_MANIFEST.sha256` | Present in the ZIP; verified against a simulated `build-control/a2/` layout at package-generation time |
| Package installed into the repository | **NO** |
| Committed | **NO** |
| Pushed | **NO** |
| `AGENT_SYSTEM_FREEZE_SHA` | **NOT YET ASSIGNED** |
| A2 integration branches | **NOT CREATED** |
| A2 integration worktrees | **NOT CREATED** |
| A3 implementation | **BLOCKED** |
| Active A1 | `A1-BOOTSTRAP` |
