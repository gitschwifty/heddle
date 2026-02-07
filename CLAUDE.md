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

**Why:** Tests define the contract. Writing them first forces clear interface design, catches regressions immediately, and makes refactoring safe. The codebase has 83+ tests — keep that ratio healthy.

**In practice:**
1. Write the test file with expected behavior (it will fail — that's the point)
2. Implement the minimum code to make tests pass
3. Run the full suite before committing: `bun test`
4. Negative tests matter — test error paths, invalid inputs, and edge cases alongside happy paths

### Running Tests

```bash
bun test                              # full suite
bun test test/tools/                  # specific directory
bun test test/provider/openrouter.unit.test.ts  # specific file
```

Integration tests hit real APIs and need env vars (auto-loaded from `.env.test`).

## Platform & Shell

- Prefer cross-platform TypeScript/Bun APIs over shell commands when possible.
- When shell is necessary, use macOS-compatible (BSD) syntax first — this is the primary dev environment.
  - `sed -E` not `sed -r`
  - BSD `tar` argument ordering
  - `trash` instead of `rm` for file deletion (enforced by hook)
- Never use `rm`, `rm -rf`, `shred`, `unlink`, or `find -delete`. Use `trash` instead.

## Code Style

- Tabs for indentation, double quotes for strings (Biome enforces this).
- Keep functions small. Prefer factory functions (`createXTool()`) over classes for tools.
- Use `async function*` generators for streaming patterns (provider streams, agent loop).
- Return error strings from tool `execute()` rather than throwing — the agent loop sends these back to the LLM as tool results. Only throw for truly unrecoverable errors (unknown tool name, invalid JSON args).

## Project Structure

```
src/
  types.ts          # Core message/tool types (TypeBox schemas)
  provider/         # LLM API clients (OpenRouter)
  agent/            # Agent loop, event types
  tools/            # Tool implementations + registry
  session/          # JSONL session persistence
  cli/              # REPL interface
test/
  mocks/            # Shared mock helpers
  provider/         # Provider unit + integration tests
  agent/            # Agent loop tests
  tools/            # Tool tests (positive + negative)
  session/          # Session logging tests
  e2e/              # End-to-end tests with mock provider + real tools
```

## Environment

- `.env` — shared config (TEST_MODEL)
- `.env.local` — secrets for runtime (OPENROUTER_API_KEY), not loaded by `bun test`
- `.env.test` — secrets + config for tests, auto-loaded by `bun test`
- All `.env*` files are gitignored.

## Commit Practices

- Commit tests and implementation separately when doing TDD.
- No `.md` files in commits (planning docs live in `private/`, gitignored).
- Keep commits focused — one logical change per commit.

## Key Patterns

- **Provider interface** (`src/provider/types.ts`): `send()` for non-streaming, `stream()` as async generator.
- **HeddleTool interface** (`src/tools/types.ts`): name, description, TypeBox parameters schema, `execute()` function.
- **ToolRegistry**: register tools, generate OpenAI-format tool definitions, execute by name with JSON string args.
- **Agent loop** (`src/agent/loop.ts`): send → tool_calls? → execute each → append results → repeat until text-only response or max iterations.
- **Mock helpers** (`test/mocks/openrouter.ts`): use these for unit tests — `mockTextResponse()`, `mockToolCallResponse()`, `mockSSE()`, etc.
