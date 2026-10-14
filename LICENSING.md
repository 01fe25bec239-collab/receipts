# Licensing scope

Receipts is released under the **Apache License, Version 2.0**, SPDX identifier **Apache-2.0**. The complete, unchanged license text is in [LICENSE](LICENSE).

This license covers all original Receipts materials in this release: source code, documentation, architecture and contract documents, configuration, build and agent workflows, tests, and plugin/integration materials. It includes both Receipts Free and Receipts Pro implementations and their shared foundations wherever Receipts holds the necessary licensing rights. Third-party materials retain their own licenses as described below.

Free and Pro are execution modes within the product architecture. They do not define different copyright-license terms. Apache 2.0 permits use, modification, forking, and redistribution, including commercial use, subject to its terms. There is no additional noncommercial restriction, anti-fork restriction, or requirement to obtain a separate Pro source license. Existing activation or entitlement logic describes application behavior and does not add restrictions to the Apache license.

Both editions remain under development. Licensing existing foundations does not imply that the complete workflows, plugins, or planned capabilities are finished or production-ready. See [README.md](README.md).

## Current policy and preserved architecture

This policy supersedes earlier source-distribution statements that describe Receipts as open core or Pro as proprietary. Those statements occur in the preserved `build-control/orchestrator-architecture/` package, including:

- `OPEN_CORE_COMMERCIAL_BOUNDARY.md` and ADR-OPENCORE-001 in `ARCHITECTURE_DECISION_RECORDS_V1_3.md`;
- `PLUGIN_DISTRIBUTION_ARCHITECTURE.md` and `REPOSITORY_LAYOUT_PROPOSAL.md`;
- associated assumptions, traceability, and evidence records.

Those documents remain preserved for architecture and historical context. Their earlier source-license recommendations are not the current licensing policy. This change does not rewrite the architecture, runtime entitlement behavior, ownership rules, or build and agent workflows.

## Third-party materials and history

External libraries, provider software, and separately licensed materials do not become Apache-licensed Receipts code simply because they are dependencies or appear in Git history. Preserve their applicable copyright, license, and attribution notices. [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) identifies the locked dependencies and supplied upstream notices.

Retained Git history includes generated and compiled dependency artifacts formerly under `target/`. Those third-party portions remain subject to their upstream licenses. The root Apache license does not replace their licenses or claim ownership of them. Historical checkpoint refs are not part of this release branch.

External AI provider subscriptions, API access, usage charges, and service terms remain governed by the providers. The Apache license does not grant access to a provider account or service.

## Contributions

Contributions intentionally submitted for inclusion are under Apache 2.0 as provided by Section 5 of the license, unless explicitly stated otherwise. Contributors must have the right to submit their work and must identify any third-party material and its applicable terms. Maintainers decide whether a contribution can be accepted. See [CONTRIBUTING.md](CONTRIBUTING.md).
