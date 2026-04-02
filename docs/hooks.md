# Hooks

Hooks run shell commands in response to heddle events — tool calls, prompts, session lifecycle. Use them for guardrails, logging, notifications, or custom integrations.

## Quick Start

Add hooks to your `config.toml` (global `~/.heddle/config.toml` or project `.heddle/config.toml`):

```toml
# Log every tool call (async — doesn't slow anything down)
[[hooks.post_tool]]
command = "echo \"$HEDDLE_HOOK_TOOL_NAME\" >> /tmp/heddle-tools.log"
async = true

# Block writes to production config
[[hooks.pre_tool]]
command = "echo 'Cannot modify production config' >&2 && exit 1"
matchers = { tool = ["write", "edit"], match_path = "**/prod/**" }
```

## Events

| Event | When it fires | Can block? |
|---|---|---|
| `session_start` | Session created | No |
| `session_end` | Session closing | No |
| `pre_prompt` | Before processing user message | Yes |
| `pre_tool` | Before a tool executes | Yes |
| `post_tool` | After a tool executes | No |
| `post_turn` | After an agent loop turn | No |
| `error` | On unrecoverable error | No |

## Hook Definition

```toml
[[hooks.pre_tool]]
command = "my-script"       # Required. Runs via sh -c
timeout = 10000             # Milliseconds (default: 10000)
mode = "both"               # "interactive", "headless", or "both" (default: "both")
async = false               # Fire-and-forget? (default: false)

[hooks.pre_tool.matchers]   # Optional — narrow when this hook fires
tool = "write"              # Tool name (string or array)
match_path = "src/**/*.ts"  # Glob against file_path in tool args
match_args = "*secret*"     # Glob against full tool args JSON
match_input = "*deploy*"    # Glob against user input text
```

All matchers use AND logic — every specified matcher must pass.

## How Hooks Execute

### Sync hooks (default)

Run sequentially. The harness waits for each to finish.

- **Exit 0**: Success. Stdout is captured as feedback (injected into LLM context for post_tool).
- **Exit non-zero**: Blocked. Stderr becomes the reason shown to the LLM. The tool call is skipped.
- **Timeout**: Process killed. Not treated as a block — the tool proceeds with a warning.

### Async hooks (`async = true`)

Fire-and-forget. Cannot block. Non-zero exits are logged as debug warnings. Good for logging, notifications, webhooks.

## Environment Variables

Every hook receives these in its environment:

| Variable | Value |
|---|---|
| `HEDDLE_HOOK_EVENT` | Event name (e.g., `pre_tool`) |
| `HEDDLE_HOOK_SESSION_ID` | Current session UUID |
| `HEDDLE_HOOK_PROJECT` | Project directory (cwd) |
| `HEDDLE_HOOK_MODEL` | Active model name |
| `HEDDLE_HOOK_TOOL_NAME` | Tool name (tool events only) |

Large data (tool args, tool results, user input) is piped via **stdin** as JSON:

```json
{ "tool_args": "{...}", "tool_result": "...", "user_input": "..." }
```

Only fields with values are included.

## Merge Behavior

Hooks from multiple config files are merged **additively**:

1. Global (`~/.heddle/config.toml`) hooks fire first
2. Local (`.heddle/config.toml`) hooks are appended per event

In headless mode, hooks sent via IPC **replace** file-based hooks for the same event.

## Recipes

### Lint check before writes

```toml
[[hooks.pre_tool]]
command = '''
  read stdin
  FILE=$(echo "$stdin" | jq -r '.tool_args | fromjson | .file_path // empty')
  if [[ "$FILE" == *.ts ]]; then
    bunx biome check "$FILE" 2>&1 || exit 1
  fi
'''
matchers = { tool = ["write", "edit"] }
```

### Slack notification on session start

```toml
[[hooks.session_start]]
command = '''
  curl -s -X POST "$SLACK_WEBHOOK" \
    -d "{\"text\": \"Heddle session started: $HEDDLE_HOOK_PROJECT\"}"
'''
async = true
mode = "interactive"
```

### Block dangerous bash commands

```toml
[[hooks.pre_tool]]
command = '''
  read stdin
  ARGS=$(echo "$stdin" | jq -r '.tool_args')
  if echo "$ARGS" | grep -qE '(rm -rf|drop table|truncate)'; then
    echo "Dangerous command blocked" >&2
    exit 1
  fi
'''
matchers = { tool = "bash" }
```

### Log all tool calls to a file

```toml
[[hooks.post_tool]]
command = '''
  echo "$(date -Iseconds) $HEDDLE_HOOK_TOOL_NAME" >> ~/.heddle/tool-audit.log
'''
async = true
```
