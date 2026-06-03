# Contributing to codesynapse

## Before you start

- Search [open issues](https://github.com/sohilladhani/codesynapse/issues) before filing a new one.
- For large changes, open an issue first to discuss the approach.

## Development setup

```bash
git clone https://github.com/sohilladhani/codesynapse.git
cd codesynapse
cargo build --workspace
cargo test --workspace
```

Rust stable toolchain required. Install via [rustup](https://rustup.rs).

## Running checks

```bash
cargo test --workspace                             # all tests
cargo clippy --workspace --all-targets -- -D warnings  # lints (must pass)
cargo fmt --all --check                            # formatting (must pass)
```

CI enforces all three on every PR against stable and beta Rust.

## Submitting a PR

1. Fork the repo and create a branch from `main`.
2. Make your changes. Add tests for new behavior.
3. Run all checks above — fix any failures before opening the PR.
4. Keep commits focused. One logical change per commit.
5. Write a clear PR description: what changed and why.

## Adding a language extractor

1. Add a tree-sitter grammar dependency to `codesynapse-core/Cargo.toml`.
2. Create `codesynapse-core/src/ts_extract/<language>.rs` implementing `LanguageExtractor`.
3. Register it in `codesynapse-core/src/ts_extract/mod.rs`.
4. Add the file extension to `codesynapse-core/src/detect.rs`.
5. Add at least one test in `tests/` with a fixture file.

## Reporting bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).

## Requesting features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md).

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
