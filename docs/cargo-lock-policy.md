# Cargo Lockfile and Reproducibility Policy

This repository is a Rust SDK workspace. The root `Cargo.toml` defines library crates, examples, and conformance tooling.

## Policy

- Library crates do not need to commit `Cargo.lock` unless they are published as binaries or QENEX operates a pinned application build from this repository.
- Example binaries, conformance jobs, and release automation should use CI-level dependency pinning so tests are reproducible even when the published libraries stay flexible for downstream users.
- If QENEX begins operating an application from this workspace, commit a workspace `Cargo.lock` and keep it updated through reviewable dependency-update PRs.

## Required checks

Every change should run the Rust-native quality gates that match this repository's strengths:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Review guidance

When adding dependencies, prefer small, well-maintained crates, keep feature flags minimal, and document any dependency that affects networking, file system access, process execution, or production runtime behavior.
