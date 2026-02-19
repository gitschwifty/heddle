# Heddle

TypeScript LLM API harness — tool execution, streaming, edits, context management.

Heddle gives LLMs the ability to read, write, edit, and search files, run shell commands, and maintain persistent conversation sessions. Built on OpenRouter's OpenAI-compatible API.

## Quick Start

```bash
bun install

# Add your API key
echo "OPENROUTER_API_KEY=sk-or-v1-your-key" > .env

# Run the CLI
bun run dev
```

## Requirements

- [Bun](https://bun.sh) (runtime, test runner, package manager)
- An [OpenRouter](https://openrouter.ai) API key

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
  config/           # Directory resolution + TOML config loading
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
bun test                    # full suite
bun test test/tools/        # specific directory
bun run tsc --noEmit        # type check
bun run lint                # lint + format (auto-fixes)
```

## Dependencies

- **[@sinclair/typebox](https://github.com/sinclairzx81/typebox)** — TypeScript type + JSON Schema from a single definition
- **[smol-toml](https://github.com/squirrelchat/smol-toml)** — TOML parser for config files
- **[@biomejs/biome](https://biomejs.dev)** — Lint + format (dev)

## License

MIT
