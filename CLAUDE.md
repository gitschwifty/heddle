# Heddle

TypeScript LLM API harness — tool execution, streaming, edits, context management.

## Runtime & Tooling

- **Bun** for everything: runtime, test runner, package manager. Never use Node, npm, npx, or vitest.
- **TypeBox** for schemas — define once, get both the TS type (`Static<typeof X>`) and JSON Schema.
- **Biome** for lint + format. Run `bunx biome check src/ test/` to verify.
- Type check: `bun run tsc --noEmit`

## Development Workflow

### TDD First

Write failing tests before implementation. This project was built test-first and should stay that way.

**Why:** Tests define the contract. Writing them first forces clear interface design, catches regressions immediately, and makes refactoring safe. The codebase has 155+ tests — keep that ratio healthy.

**In practice:**
1. Write the test file with expected behavior (it will fail — that's the point)
2. Implement the minimum code to make tests pass
3. Run the full suite before committing: `bun test`
4. Negative tests matter — test error paths, invalid inputs, and edge cases alongside happy paths

### Running Tests

```bash
bun test                              # unit tests only (integration skipped)
bun run test:integration              # include provider integration tests
bun run test:all                      # everything including slow multi-turn tests
bun test test/tools/                  # specific directory
bun test test/provider/openrouter.unit.test.ts  # specific file
```

Integration tests hit real APIs and need env vars (auto-loaded from `.env.test`). Gated by `HEDDLE_INTEGRATION_TESTS=1` (set to `0` in `.env.test` by default). Slow multi-turn integration tests additionally require `HEDDLE_SLOW_TESTS=1`.

### Test Concurrency

Tests run concurrently via `concurrentTestGlob` in `bunfig.toml`. Design all tests to be concurrency-safe:

- **No shared mutable state between tests.** Never use `let dir` with `beforeEach`/`afterEach` — concurrent tests race on the shared variable.
- **Use `withTmpDir()` or `beforeAll`/`afterAll`** for temp directories:
  - `withTmpDir(async (dir) => { ... })` — each test gets its own isolated dir (best for tests that use the same filenames).
  - `beforeAll`/`afterAll` with distinct filenames per test — one shared dir, no filename collisions (best when tests use different files).
- **Use distinct filenames** across tests in the same describe block (e.g., `data.txt`, `chain.txt`, `session.jsonl`) so they can share a directory without conflicts.
- **Tests that don't touch the filesystem** (pure mock/in-memory) are inherently safe.

## Platform & Shell

- Prefer cross-platform TypeScript/Bun APIs over shell commands when possible.
- When shell is necessary, use macOS-compatible (BSD) syntax first — this is the primary dev environment.
  - `sed -E` not `sed -r`
  - BSD `tar` argument ordering
  - `trash` instead of `rm` for file deletion (enforced by hook)
- Never use `rm`, `rm -rf`, `shred`, `unlink`, or `find -delete`. Use `trash` instead.

## Code Style

- Tabs for indentation, double quotes for strings (Biome enforces this).
- Follow Biome rules - no non-null assertions (fine in test), use template literal strings, use flat map, etc.
- Keep functions small. Prefer factory functions (`createXTool()`) over classes for tools.
- Use `async function*` generators for streaming patterns (provider streams, agent loop).
- Return error strings from tool `execute()` rather than throwing — the agent loop sends these back to the LLM as tool results. Only throw for truly unrecoverable errors (unknown tool name, invalid JSON args).

## File Format Philosophy

- **Human + agent readable/writable** (agents, skills, HEDDLE.md): **Markdown**
- **Human config** (settings): **TOML**
- **Machine-only** (future: plugins, cache, telemetry): whatever fits (JSON, binary, etc.)
- **Session logs**: **JSONL**

## Config Directory

Two-layer config: global (`~/.heddle/`) and local (`./.heddle/` in project dir). We also follow the AGENTS.md standard, with the closest to cwd the most important, but all from cwd to home, and ~/.heddle/AGENTS.md included.

### Global: `~/.heddle/`

```
~/.heddle/
  config.toml       # User settings (model, api_key, system_prompt)
  agents/           # Agent persona definitions (Markdown)
  skills/           # Reusable instruction sets (Markdown)
  sessions/         # JSONL conversation logs
```

### Local: `.heddle/` in project directory

Project-specific overrides (checked in or gitignored per preference):

```
<project>/.heddle/
  config.toml       # Project-specific settings (overrides global)
  agents/           # Project-specific agent definitions
  skills/           # Project-specific skills
```

**Merge order:** Defaults → Global → Local → Env vars. Last wins.

### Dev/Test Isolation

`HEDDLE_HOME` env var overrides the global config dir. Relative paths resolve from cwd.

```bash
HEDDLE_HOME=.heddle-dev bun run dev    # Dev config, easy to blow away
HEDDLE_HOME=.heddle-test bun test      # Isolated test config
```

## Project Structure

```
src/
  types.ts          # Core message/tool types (TypeBox schemas)
  config/           # Directory resolution + TOML config loading
  provider/         # LLM API clients (OpenRouter)
  agent/            # Agent loop, event types
  tools/            # Tool implementations + registry
  session/          # JSONL session persistence
  cli/              # REPL interface
test/
  mocks/            # Shared mock helpers
  config/           # Config paths + loader + agents-md tests
  provider/         # Provider unit + integration tests
  agent/            # Agent loop tests (streaming, multi-turn, doom loop)
  tools/            # Tool tests (edit, fuzzy-match, read, write, grep, glob)
  session/          # Session logging tests
  e2e/              # End-to-end tests with mock provider + real tools
  integration/      # Real-model integration tests (gated by env vars)
```

## Environment

- `.env` — shared config (TEST_MODEL)
- `.env.local` — secrets for runtime (OPENROUTER_API_KEY), not loaded by `bun test`
- `.env.test` — secrets + config for tests, auto-loaded by `bun test`. Also sets `HEDDLE_INTEGRATION_TESTS=0`.
- All `.env*` files are gitignored.

## Commit Practices

- Commit tests and implementation separately when doing TDD.
- No `.md` files in commits (planning docs live in `private/`, gitignored).
- Keep commits focused — one logical change per commit.

## Key Patterns

- **Provider interface** (`src/provider/types.ts`): `send()` for non-streaming, `stream()` as async generator.
- **HeddleTool interface** (`src/tools/types.ts`): name, description, TypeBox parameters schema, `execute()` function.
- **ToolRegistry**: register tools, generate OpenAI-format tool definitions, execute by name with JSON string args.
- **Agent loop** (`src/agent/loop.ts`): Two variants:
  - `runAgentLoop()` — non-streaming, uses `provider.send()`. Good for tests and batch use.
  - `runAgentLoopStreaming()` — streaming, uses `provider.stream()`, yields `content_delta` events as tokens arrive. CLI uses this.
  - Both support doom loop detection (configurable via `doomLoopThreshold`, default 3).
- **Fuzzy edit matching** (`src/tools/fuzzy-match.ts`): When exact match fails, `cascadingMatch()` tries 4 levels: exact → whitespace-normalized → indent-flexible → line-fuzzy. Edit tool falls back automatically.
- **AGENTS.md** (`src/config/agents-md.ts`): `loadAgentsContext()` walks up from cwd collecting AGENTS.md files (case-insensitive), also checks HEDDLE_HOME. Concatenated farthest-first into system prompt.
- **Mock helpers** (`test/mocks/openrouter.ts`): use these for unit tests — `mockTextResponse()`, `mockToolCallResponse()`, `textChunk()`, `toolCallChunk()`, `finishChunk()`, `mockSSE()`, etc.

## Agent Event Types

The agent loop yields events via `AsyncGenerator<AgentEvent>`:

- `content_delta` — streaming text token (streaming loop only)
- `assistant_message` — complete assembled message
- `tool_start` / `tool_end` — tool execution lifecycle
- `loop_detected` — doom loop warning (N identical iterations)
- `error` — unrecoverable error

## IPC Compatibility

- Protocol rules live in `compatibility.md` and `PROTOCOL_VERSION`.
- Always send `protocol_version` in `Init` when supported; always return it in `InitOk`.
- Golden transcripts are the contract; update fixtures on any schema change.
- IPC fixtures live in `fixtures/ipc/` (canonical in Orboros) and are synced into Heddle via `scripts/sync-ipc.sh`.
- Pre-commit hooks enforce protocol version alignment and IPC sync.
