# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the CLI and core runtime. Key modules include `api.rs` for Anthropic/Ollama requests, `engine.rs` for the agent loop, `repl.rs` for interactive mode, and `tools/` for file tool implementations (`file_read.rs`, `file_edit.rs`, `file_write.rs`). `types.rs` holds shared data structures, and `markdown.rs` handles terminal rendering. `examples/` contains runnable usage samples, `docs/` stores design notes, and `scripts/update-changelog.sh` supports release documentation.

## Build, Test, and Development Commands
Use standard Cargo workflows from the repository root:

- `cargo run` starts the interactive REPL.
- `cargo run -- "your prompt"` runs a single-shot query.
- `cargo build --release` builds the optimized binary.
- `cargo test` runs unit tests across the crate.
- `cargo fmt` formats the codebase with `rustfmt`.
- `cargo fmt -- --check` verifies formatting in CI-style checks.

If you want the local model path, export `USE_OLLAMA=1` before running.

## Coding Style & Naming Conventions
This project follows idiomatic Rust with 4-space indentation and `snake_case` for modules, files, functions, and tests. Use `CamelCase` for structs and enums, and keep modules narrowly scoped by responsibility. Prefer small, composable functions and `anyhow::Result` for fallible CLI flows. Run `cargo fmt` before opening a PR; no separate lint config is checked in, so formatting consistency matters.

## Testing Guidelines
Tests are inline `#[cfg(test)]` modules placed next to the implementation they cover. Name tests descriptively, for example `write_creates_parent_directories` or `clear_command_empties_history`. Run `cargo test` locally before submitting changes. When editing tool behavior, add or update focused unit tests in the relevant file under `src/tools/`.

## Commit & Pull Request Guidelines
Recent history uses short conventional-style subjects such as `feat: file tools`, `docs: update README.md`, and `chore: optimize changelog bash`. Follow that pattern: `<type>: <summary>`, keeping the summary imperative and specific. PRs should include a brief description, note any user-visible CLI changes, link related issues when applicable, and include terminal output or screenshots for REPL/UI behavior changes.

## Configuration Tips
Environment variables drive runtime behavior. `ANTHROPIC_AUTH_TOKEN` enables Anthropic requests; `USE_OLLAMA=1` switches to the local Ollama endpoint, and `OLLAMA_BASE_URL` / `OLLAMA_MODEL` override defaults when needed.
