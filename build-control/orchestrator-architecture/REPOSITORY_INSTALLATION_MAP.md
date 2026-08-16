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

# REPOSITORY_INSTALLATION_MAP

**Install root:** `build-control/orchestrator-architecture/`

Deliberately **not** `build-control/a2/` — that path holds the historical five-manager Receipts control package at `3c70f4d8…`, which must remain untouched as evidence. Installing here would overwrite historical artifacts and destroy the reconciliation baseline.

## Untouched historical paths

```
Receipts_Final_Architecture.md
architecture-decisions/**
contracts/**
orchestration/**
schemas/SCHEMA_PLAN.md
build-control/a2/**
```

None is modified, moved, or deleted by this installation. Their errata are recorded in `HISTORICAL_BASELINE_ERRATA.md`, not by editing them.

**Name collision note:** this package ships a `schemas/` directory. The repository already contains `schemas/SCHEMA_PLAN.md` at its root. The new schemas install to `build-control/orchestrator-architecture/schemas/`, **not** to the repository root, so there is no collision.

## Mapping

| Package path | Repository path |
|---|---|
| `*.md` (all top-level documents) | `build-control/orchestrator-architecture/*.md` |
| `BUILD_A2_MANAGERS/*.md` | `build-control/orchestrator-architecture/BUILD_A2_MANAGERS/*.md` |
| `schemas/*.schema.json` | `build-control/orchestrator-architecture/schemas/*.schema.json` |
| `evidence/**` (validators, registries, canonical authorities) | `build-control/orchestrator-architecture/evidence/**` |
| `evidence/regression/**` (source-validator regression fixtures) | `build-control/orchestrator-architecture/evidence/regression/**` |
| `fixtures/admission/**` | `build-control/orchestrator-architecture/fixtures/admission/**` |
| `fixtures/graphs/**` | `build-control/orchestrator-architecture/fixtures/graphs/**` |
| `fixtures/graphs-negative/**` | `build-control/orchestrator-architecture/fixtures/graphs-negative/**` |
| `fixtures/host_capability/**` | `build-control/orchestrator-architecture/fixtures/host_capability/**` |
| `fixtures/host_capability-negative/**` | `build-control/orchestrator-architecture/fixtures/host_capability-negative/**` |
| `INSTALL_MANIFEST.sha256` | `build-control/orchestrator-architecture/INSTALL_MANIFEST.sha256` |
| `PACKAGE_MANIFEST.sha256` | **not installed** — describes the ZIP layout only |

Package paths and installed paths are **identical apart from the install-root prefix**. No directory is renamed. Every path present in `INSTALL_MANIFEST.sha256` resolves to exactly one rule above — `INSTALL_MANIFEST_UNMAPPED_PATHS = 0` and `INSTALLATION_MAP_AMBIGUOUS_PATHS = 0` check this against the manifest actually shipped, not against a hand-maintained belief about what it contains.

This is a deliberate correction of the historical package's approach, where ZIP folders were uppercase and installed folders were lowercase slugs — a rename that invalidated the manifest after copying and required two differently-shaped manifests. Here the two manifests differ only by prefix, so a copy cannot silently break verification.

## Resulting layout

```
receipts/
├── Receipts_Final_Architecture.md          ← historical, untouched
├── architecture-decisions/                 ← historical, untouched
├── contracts/                              ← historical, untouched
├── orchestration/                          ← historical, untouched
├── schemas/SCHEMA_PLAN.md                  ← historical, untouched
└── build-control/
    ├── a2/                                 ← historical five-manager package, untouched
    └── orchestrator-architecture/          ← NEW
        ├── *.md
        ├── BUILD_A2_MANAGERS/
        ├── schemas/
        ├── evidence/
        │   └── regression/
        ├── fixtures/
        │   ├── admission/
        │   ├── graphs/
        │   ├── graphs-negative/
        │   ├── host_capability/
        │   └── host_capability-negative/
        └── INSTALL_MANIFEST.sha256
```

## Procedure

**STEP 1** — Extract the package.

**STEP 2** — From the extracted root, verify the package manifest.

| Platform | Command |
|---|---|
| GNU/Linux | `sha256sum -c PACKAGE_MANIFEST.sha256` |
| macOS | `shasum -a 256 -c PACKAGE_MANIFEST.sha256` |

All entries must pass. The manifest format is the standard `<sha256>  <path>` two-space form, which both tools read natively — **the format is unchanged**, only the tool name differs. macOS ships `shasum` by default and does not ship GNU `sha256sum` unless coreutils is installed.

**STEP 3** — Copy into `build-control/orchestrator-architecture/`, preserving relative paths. Do not install `PACKAGE_MANIFEST.sha256`.

**STEP 4** — From the repository root, verify the installed layout.

```
cd build-control/orchestrator-architecture
```

| Platform | Command |
|---|---|
| GNU/Linux | `sha256sum -c INSTALL_MANIFEST.sha256` |
| macOS | `shasum -a 256 -c INSTALL_MANIFEST.sha256` |

All entries must pass. Both manifests are relative-path and must be verified from the directory that contains them.

**Note for the recorded environment.** `git-state.txt` shows the repository root at `/Users/omkar/Documents/receipts`, so verification will be performed on **macOS** — use the `shasum -a 256 -c` form. A `sha256sum: command not found` error there is a missing tool, not a manifest failure.

**STEP 5** — Only after both validations pass may the package be committed.

Beyond step 5, and **not authorized by this package**: commit, push, and only then record the resulting `main` SHA as `NEW_ARCHITECTURE_FREEZE_SHA`. The freeze SHA is obtained from the repository after the commit exists; it is never written into the package that produces it.

## Ownership of installed files

| Path | Writer |
|---|---|
| `build-control/orchestrator-architecture/**` | **BUILD-A1 only** |

No BUILD-A2 writes this tree. Managers read their own definitions; they do not edit their own charters.

## Current state

| Item | State |
|---|---|
| Installed | **NO** |
| Committed | **NO** |
| Pushed | **NO** |
| `NEW_ARCHITECTURE_FREEZE_SHA` | **NOT ASSIGNED** |
