# Rust Compiler & Brace Validation Rules
- CRITICAL: You are an autonomous agent with terminal access. NEVER assume your Rust brace placement is correct.
- Whenever you write or modify a `*.rs` file, you MUST immediately execute the bash tool: `cargo fmt && cargo check`.
- If `cargo check` fails due to syntax or mismatched braces, read the diagnostic line number, fix only that broken scope, and re-run `cargo check`.
- Do not mark any code modification task as complete until `cargo check` returns a clean exit status (0).
- Keep all refactored or generated functions under 40 lines to avoid bracket scope misalignment.
- Run `cargo clippy --all-targets -- -D warnings` (not `--lib`) — CI uses `--all-targets` and catches additional lints in tests and examples.

