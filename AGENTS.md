# Repository Guidelines

## Project Structure & Module Organization
- `Cargo.toml`: Package metadata and dependencies.
- `src/main.rs`: CLI entry; starts processes, streams/collects logs, handles `--follow` and Ctrl+C.
- `src/config.rs`: Loads `proc.toml` or `Procfile` into `ProcessConfig`.
- `target/`: Build artifacts (ignored by Git).
- `README.md`, `plan.md`: Usage and implementation notes.

## Build, Test, and Development Commands
- `cargo build` / `cargo build --release`: Compile (debug/release).
- `cargo run -- [--follow]`: Run locally; pass flags after `--`.
- `cargo fmt --all`: Format code. Use `--check` in CI.
- `cargo clippy --all-targets --all-features -- -D warnings`: Lint and fail on warnings.
- `cargo test`: Run unit/integration tests (use `#[tokio::test]` for async).
- Example: `./target/release/oxproc --follow` to stream logs.

## Coding Style & Naming Conventions
- Rust 2021; rustfmt defaults (4‑space indent, 100 cols).
- Names: `snake_case` for functions/files/modules; `CamelCase` for types/traits; `SCREAMING_SNAKE_CASE` for consts.
- Errors: Prefer `anyhow::Result<T>` in binaries; define typed errors with `thiserror` in modules.
- Logging: For new code, prefer `tracing` over `println!` (user‑facing messages are fine via `println!`).

## Testing Guidelines
- Unit tests live beside code (`mod tests { ... }`).
- Integration tests go in `tests/` (e.g., `tests/config_tests.rs`).
- Name tests by behavior: `loads_proc_toml`, `handles_empty_procfile`.
- Keep tests deterministic; use fixtures under `tests/fixtures/`.
- Target high‑value coverage around config parsing and process lifecycle.

## Commit & Pull Request Guidelines
- Conventional Commits used in history (e.g., `feat:`, `docs:`, `chore:`). Examples: `feat: support cwd`, `fix: handle empty Procfile`.
- PRs: clear description, linked issues, rationale, and sample command/output (e.g., `cargo run -- --follow`). Update docs if behavior changes.
- Run fmt + clippy + tests locally before opening PR.

## Security & Configuration Tips
- This tool executes commands via `sh -c`; never run untrusted `proc.toml`/`Procfile`.
- Prefer explicit `cwd` and absolute paths in configs. Avoid committing logs or secrets.

## Agent‑Specific Instructions
- Scope: applies to the entire repo. Keep changes minimal and focused; avoid renames without need. When adding features, update `README.md` and include a runnable example.
