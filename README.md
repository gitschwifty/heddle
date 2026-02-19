# Heddle

TypeScript LLM API harness — tool execution, streaming, edits, context management.

Heddle gives LLMs the ability to read, write, edit, and search files, run shell commands, and maintain persistent conversation sessions. Built on OpenRouter's OpenAI-compatible API.

## Quick Start

```bash
# Install dependencies
bun install

# Set up environment
cp .env.example .env
# Create .env.local with your API key:
echo "OPENROUTER_API_KEY=sk-or-v1-your-key" > .env.local

# Run the CLI
bun run src/cli/index.ts
```

## Requirements

- [Bun](https://bun.sh) (runtime, test runner, package manager)
- An [OpenRouter](https://openrouter.ai) API key

## Environment

Heddle uses three env files (all gitignored via `.env*`):

| File | Loaded by | Purpose |
|------|-----------|---------|
| `.env` | `bun run` + `bun test` | Shared config (`TEST_MODEL`) |
| `.env.local` | `bun run` only | Runtime secrets (`OPENROUTER_API_KEY`) |
| `.env.test` | `bun test` only | Test secrets + config |

See `.env.example` for the template.

## Tools

The agent has access to 6 built-in tools:

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write/overwrite a file (creates parent dirs) |
| `edit_file` | Find-and-replace with unique match enforcement |
| `bash` | Run shell commands |
| `glob` | Find files by glob pattern |
| `grep` | Search file contents by regex |

## Architecture

```
src/
  types.ts          # Core message/tool types (TypeBox schemas)
  provider/         # LLM API clients (OpenRouter)
  agent/            # Agent loop (send → tool_call → execute → repeat)
  tools/            # Tool implementations + registry
  session/          # JSONL session persistence
  cli/              # Interactive REPL
```

**Agent loop:** Send messages to the LLM. If it responds with tool calls, execute them, append results, and send again. Repeat until the LLM responds with text only.

**TypeBox:** Every type is defined once using TypeBox, producing both a TypeScript type and a JSON Schema. Tool parameter schemas double as OpenAI function definitions.

## Development

```bash
# Run tests
bun test                    # full suite (83 tests)
bun test test/tools/        # specific directory
bun test test/provider/openrouter.unit.test.ts  # specific file

# Type check
bun run tsc --noEmit

# Lint + format
bunx biome check src/ test/
```

## Dependencies

- **[@sinclair/typebox](https://github.com/sinclairzx81/typebox)** — TypeScript type + JSON Schema from a single definition
- **[@biomejs/biome](https://biomejs.dev)** — Lint + format (dev)

## License

MIT
