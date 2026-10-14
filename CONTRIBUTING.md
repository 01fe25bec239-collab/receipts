# Contributing to Receipts

Receipts Free and Pro are open source under Apache 2.0 and remain under active development. Start with [README.md](README.md) for the implemented foundations and planned product behavior.

## Issues and pull requests

Use [GitHub issues](https://github.com/01fe25bec239-collab/receipts/issues) for ordinary bugs and feature requests. Search existing issues first, include reproducible steps and the relevant commit/version, and remove credentials, private prompts, repository contents, and personal data from examples or logs. Report vulnerabilities privately using [SECURITY.md](SECURITY.md).

Propose all changes through a pull request from a branch or fork. Keep the change focused, explain the problem and resulting behavior, and include relevant validation evidence. Discuss architecture or broad behavior changes in an issue before implementing them. Maintainers retain control over whether contributions are accepted, repository access, merging, and releases. Opening a PR does not grant push or merge access.

## Development checks

Use Git, rustup, and a native C compiler for bundled SQLite. `rust-toolchain.toml` pins Rust 1.97.1. From the repository root:

```sh
rustup toolchain install 1.97.1 --component rustfmt --component clippy
cargo build --workspace --locked
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
git diff --check
```

Use `cargo fmt --all` to apply formatting before checking it. Some tests create temporary Git repositories and bind local loopback sockets; the test environment must permit those operations. Report failures and ignored tests accurately. Do not weaken checks to conceal a failure. Preserve the pinned toolchain and lockfile; explain intentional dependency changes.

## Code and review expectations

- Follow the existing subsystem boundaries, contracts, ownership rules, and exact-commit review conventions. Read the relevant architecture and build-control documents before proposing a change. Frozen documents must not be silently rewritten; propose changes through the existing change-control process.
- Add meaningful tests for changed behavior and preserve validation, error handling, security boundaries, and code-state provenance. Documentation-only changes should receive appropriate content and link checks.
- Keep build and agent workflows intact. Distinguish implemented capabilities from planned behavior; passing tests does not establish production readiness.
- Never submit credentials, private keys, provider tokens, or private user data. Use synthetic fixtures and sanitized diagnostics. Security concerns belong in the private disclosure channel.

## Licensing

Read [LICENSE](LICENSE), [LICENSING.md](LICENSING.md), and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Original contributions intentionally submitted for inclusion are licensed under Apache 2.0 under Section 5, unless explicitly stated otherwise. You must have the necessary rights to contribute them.

Identify imported or copied material, its origin, its license, and any required notices in the PR. Preserve upstream attribution. AI assistance does not establish originality or permission to reuse third-party work; review assisted contributions for those concerns. Maintainers may request provenance clarification or decline a contribution whose licensing cannot be established. No separate contributor license agreement is required by this project.
