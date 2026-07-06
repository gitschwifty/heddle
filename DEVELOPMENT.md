# Development

Workflow notes for hacking on heddle. The user-facing reference lives in [README.md](README.md); this file is for contributors.

## Prerequisites

- Stable Rust toolchain (1.75+). Use `rustup` for installs/upgrades.
- An [OpenRouter](https://openrouter.ai) API key for any code path that hits the real provider (most tests don't need this).

## Build

```bash
cargo build                  # debug build
cargo build --release        # release build (both bins under target/release/)
```

Three binaries are produced:

- `heddle` — interactive REPL
- `heddle-headless` — JSON-over-stdio adapter
- `export-schemas` — regenerates `schemas/*.json` from the wire-format Rust types (see [Schema export](#schema-export))

## Run

```bash
cargo run --bin heddle                       # REPL
cargo run --bin heddle -- -p "summarize ./README.md"
cargo run --bin heddle -- --resume <id|name> # resume an existing session
cargo run --bin heddle -- --fork <id|name>   # fork a session into a new one
cargo run --bin heddle -- --help             # full flag list
cargo run --bin heddle-headless              # headless mode (reads JSONL on stdin)
```

The binaries auto-load `.env.local` then `.env` at startup via `dotenvy`. Put `OPENROUTER_API_KEY=...` in `.env.local` (gitignored) for local runs.

## Test

```bash
cargo test                                   # full suite (~834 tests across 84 files)
cargo test --test tools_edit                 # single test binary
cargo test --test tools_edit -- match_       # filter by name inside a binary
cargo test -- --nocapture                    # show stdout/stderr from tests
```

Common commands are also available via `just`:

```bash
just test
just test-e2e
just test-multi-turn-live
just test-live
```

### Integration / live-model tests

`cargo test` does **not** auto-load `.env.test`. Integration tests opt in via `tests/common/env.rs`:

```rust
mod common;

#[tokio::test]
async fn my_integration_test() {
    common::env::init();  // OnceLock-guarded dotenvy load of .env.test
    // ...
}
```

Two gates exist today:

| Gate | Tests | Required env |
|---|---|---|
| **Provider** | `tests/provider_openrouter_integration.rs` (3 tests) | `HEDDLE_INTEGRATION_TESTS=1`, `OPENROUTER_API_KEY` |
| **Slow multi-turn** | `tests/multi_turn_integration.rs` (3 tests) | `HEDDLE_INTEGRATION_TESTS=1`, `HEDDLE_SLOW_TESTS=1`, `OPENROUTER_API_KEY` |

To run them, put the env vars in `.env.test` (gitignored) and:

```bash
HEDDLE_SLOW_TESTS=1 cargo test --test multi_turn_integration --test provider_openrouter_integration
```

Without the env vars, the tests print `skip:` and pass.

### Test conventions

- Each integration test binary lives in `tests/<name>.rs` and is its own crate.
- Shared helpers live in `tests/common/{sandbox,mocks,headless,env}.rs`. They're `#![allow(dead_code)]` since each test binary only uses a subset.
- Use `Sandbox::new()` (from `tests/common/sandbox.rs`) for any test that touches `HEDDLE_HOME` or cwd — it serializes via `GLOBAL_ENV_LOCK` and restores state on drop.
- Use `MockProvider` (in `tests/common/mocks.rs`) for scripted provider responses; `ScriptProvider` patterns in `tests/e2e_simple_task.rs` work for non-streaming flows.
- Use `tests/common/headless.rs` to spawn `heddle-headless` as a subprocess and drive it via JSONL.

## Lint & format

```bash
cargo clippy --all-targets   # must be 0 warnings
cargo fmt                    # rustfmt
cargo fmt --check            # CI-style check
```

Pre-commit checklist:

1. `cargo fmt`
2. `cargo clippy --all-targets`
3. `cargo test`

## Schema export

The TOML config schemas live in `schemas/config.schema.json` and `schemas/hooks.schema.json` and are consumed by Taplo (TOML LSP) for editor autocomplete on `.heddle/config.toml`. They're derived from the wire-format Rust types (`HeddleConfigSchema` in `src/config/types.rs`, `HooksConfig` in `src/hooks/types.rs`) via `schemars`.

To regenerate after editing those types:

```bash
cargo run --bin export-schemas
```

Commit the regenerated JSON files alongside the type changes.

## IPC compatibility

- `PROTOCOL_VERSION` is read at compile time via `include_str!`. Bumps must coordinate with Orboros — see [compatibility.md](compatibility.md).
- IPC fixtures live in `fixtures/ipc/` (canonical in Orboros) and are synced via `scripts/sync-ipc.sh`.
- When a replay/headless test fails across the board, check `PROTOCOL_VERSION` first — see `tests/headless_replay.rs` and the debugging notes in the original `CLAUDE.md`.

## Architecture refresher

| Crate path | What lives there |
|---|---|
| `src/types.rs` | Core message/tool wire types |
| `src/agent/loop_.rs` | Agent loop — `run_agent_loop` and `run_agent_loop_streaming` |
| `src/provider/openrouter.rs` | OpenRouter (OpenAI-compatible) HTTP client |
| `src/tools/` | One file per tool + `registry.rs` |
| `src/session/{jsonl,fork,list,setup}.rs` | Session persistence + resume/fork |
| `src/context/{pruning,compaction}.rs` | Context management |
| `src/headless/` | JSON-over-stdio adapter on top of the agent loop |
| `src/ipc/{types,codec,errors,protocol}.rs` | IPC schema + framing |
| `src/cli/repl.rs` | Interactive REPL entry point |

## Adding a tool

1. Create `src/tools/<name>.rs` exporting `pub fn create_<name>_tool() -> Arc<dyn HeddleTool>`.
2. Register it in `src/tools/registry.rs` (`default_registry` or the relevant set).
3. Tests go in `tests/tools_<name>.rs` — use the existing tool tests as templates.
4. If the tool needs permission gating, update `src/permissions/checker.rs`.

## Releasing

No tagged release process yet. When one is set up, document:

- Version bump in `Cargo.toml`
- `cargo publish` (if/when crate is published)
- Binary artifacts via `cargo build --release`

## Project layout reference

```
src/
  bin/                  heddle, heddle-headless, export-schemas
  agent/                agent loop + event types
  agents/               agent persona loading
  cli/                  REPL, completer, oneshot, shell
  commands/             slash commands
  config/               TOML loader, paths, feature flags, wire types
  context/              pruning + compaction
  cost/                 token + pricing
  debug.rs              debug logging
  file_history/         per-file backup/restore
  headless/             JSON-over-stdio
  history/              cross-session message history
  hooks/                pre/post hooks
  ipc/                  protocol/codec/errors
  memory/               MEMORY.md loader
  permissions/          approval modes + rules
  plans/                plan storage
  provider/             OpenRouter client
  session/              JSONL meta + messages, resume, fork
  tasks/                task storage
  tools/                one file per tool + registry
  types.rs              core wire types
  usage/                usage/cost collector

tests/                  one file per integration test target (84 files)
  common/               shared helpers (sandbox, mocks, headless, env)
fixtures/ipc/           IPC golden transcripts (synced from Orboros)
schemas/                JSON Schemas for Taplo (regenerated by export-schemas)
private/                planning + status docs (gitignored, symlink to vault)
```
