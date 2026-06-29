# Heddle Rust Rewrite

Rust LLM API harness: tool execution, streaming, edits, context management, sessions, and IPC.

## Runtime & Tooling

- **Cargo** for Rust build/test/lint/format.
- **just** for common local command shortcuts when available.
- Keep the TypeScript sources and tests under `ts-src/` and `ts-test/` as reference material unless a task explicitly asks to change them.
- Do not use npm, npx, or vitest for Rust rewrite work.

## Common Commands

```bash
cargo build
cargo test
cargo test --test e2e_simple_task
cargo test --test multi_turn_integration -- --nocapture
cargo fmt --check
cargo clippy --all-targets
```

If `just` is installed:

```bash
just test
just test-e2e
just test-multi-turn-live
just test-live
just check
```

Live provider tests require `OPENROUTER_API_KEY`. Provider tests are gated by `HEDDLE_INTEGRATION_TESTS=1`; slow multi-turn tests also require `HEDDLE_SLOW_TESTS=1`.

## Development Workflow

Write tests before implementation. Keep changes focused and prefer small, direct Rust code that follows the existing module patterns.

For tests that touch global env vars, cwd, `HEDDLE_HOME`, or shared filesystem state, use helpers from `tests/common/`, especially `Sandbox::new()`.

## Code Style

- Run `cargo fmt` before finishing.
- Run `cargo clippy --all-targets` for lint-sensitive changes.
- Prefer `Result` and explicit error strings over panics outside tests.
- Tool `execute()` methods should return error strings for recoverable tool failures.
- Use `async fn` / streams consistently with the existing provider and agent loop code.
- Keep JSON wire formats compatible with the schemas and fixtures.

## Project Structure

```
src/
  agent/        # Agent loop and event types
  cli/          # Interactive REPL and one-shot mode
  config/       # Config loading, discovery, AGENTS.md context
  provider/     # OpenRouter provider
  tools/        # Tool implementations and registry
  session/      # JSONL session persistence
  ipc/          # Headless IPC schema and codec
tests/
  common/       # Shared Rust test helpers
  *_integration.rs
ts-src/
ts-test/
```

## IPC Compatibility

- Protocol rules live in `compatibility.md` and `PROTOCOL_VERSION`.
- IPC fixtures are the contract. Update fixtures deliberately when schema behavior changes.
