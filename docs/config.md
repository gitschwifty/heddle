# Configuration

Heddle uses a two-layer TOML configuration: global settings in `~/.heddle/config.toml` and project-specific overrides in `.heddle/config.toml`. Values merge with last-write-wins: defaults < global < local < env vars.

## Config File Locations

| Location | Purpose |
|---|---|
| `~/.heddle/config.toml` | Global defaults (model, API key, system prompt) |
| `.heddle/config.toml` | Project overrides (checked in or gitignored) |

Override the global config directory with `HEDDLE_HOME`:

```bash
HEDDLE_HOME=.heddle-dev cargo run --bin heddle   # use a dev config
```

## Full Reference

```toml
# ── Identity / API ──────────────────────────────────
api_key = "sk-or-..."          # OpenRouter API key (or set OPENROUTER_API_KEY)
base_url = "https://..."       # Custom API base URL (or HEDDLE_BASE_URL)

# ── Model Selection ─────────────────────────────────
model = "openrouter/free"      # Primary model (or HEDDLE_MODEL)
weak_model = "..."             # Cheap model for compaction/summaries (or HEDDLE_WEAK_MODEL)
editor_model = "..."           # Model for edit operations

# Heddle fetches OpenRouter `/models` metadata lazily for cost estimates,
# `/models [query]`, `/model [id]`, and `/context` model-limit reporting.

# ── API Parameters ──────────────────────────────────
max_tokens = 128000            # Max context window (or HEDDLE_MAX_TOKENS)
temperature = 0.7              # Sampling temperature (or HEDDLE_TEMPERATURE)

[app_attribution]              # Optional OpenRouter dashboard attribution
referer = "https://github.com/gitschwifty/heddle"
title = "Heddle"
categories = "cli-agent"

# ── Session Behavior ────────────────────────────────
system_prompt = "You are..."   # Custom system prompt
approval_mode = "suggest"      # "suggest" | "auto-edit" | "full-auto" | "plan" | "yolo"
instructions = ["...", "..."]  # Additional instructions appended to system prompt
tools = ["read", "write", "edit", "glob", "grep", "bash"]  # Enabled tools (or HEDDLE_TOOLS)
web_fetch_allow_private_addresses = false  # Allow web_fetch to reach localhost/private IPs

# ── Context Management ──────────────────────────────
doom_loop_threshold = 3        # Identical tool iterations before stopping
budget_limit = 5.0             # Max cost in dollars before stopping
compact_trigger = 0.8          # Context usage ratio that triggers compaction
prune_protect = 5              # Recent messages protected from pruning
prune_minimum = 3              # Minimum messages to keep after pruning
compact_buffer = 0.3           # Buffer ratio after compaction

# ── Feature Flags ───────────────────────────────────
[features]
history = true                 # Session history logging
usage_data = true              # Token usage tracking
facets = true                  # System facets in prompt
file_history = true            # File backup before edits
paste_cache = true             # Paste buffer
status_line = true             # Status line display
hooks = true                   # Hook execution (see docs/hooks.md)
tasks = true                   # Task tracking

# ── Permissions ─────────────────────────────────────
[permissions]
allow = ["read(*)", "glob(*)"]         # Always allow
deny = ["bash(rm *)", "write(.env*)"]  # Always deny
ask = ["write(*)", "edit(*)"]          # Prompt for approval

# ── Hooks ───────────────────────────────────────────
# See docs/hooks.md for full reference
[[hooks.pre_tool]]
command = "my-guardrail"
matchers = { tool = "bash" }
```

## Environment Variable Overrides

| Env Var | Overrides |
|---|---|
| `OPENROUTER_API_KEY` | `api_key` |
| `HEDDLE_MODEL` | `model` |
| `HEDDLE_WEAK_MODEL` | `weak_model` |
| `HEDDLE_BASE_URL` | `base_url` |
| `HEDDLE_MAX_TOKENS` | `max_tokens` |
| `HEDDLE_TEMPERATURE` | `temperature` |
| `HEDDLE_APP_REFERER` + `HEDDLE_APP_TITLE` | `app_attribution.referer` + `app_attribution.title` |
| `HEDDLE_APP_CATEGORIES` | `app_attribution.categories` |
| `HEDDLE_APPROVAL_MODE` | `approval_mode` |
| `HEDDLE_TOOLS` | `tools` (comma-separated) |
| `HEDDLE_WEB_FETCH_ALLOW_PRIVATE_ADDRESSES` | `web_fetch_allow_private_addresses` |
| `HEDDLE_HOME` | Global config directory |

Env vars always win over file config.

## Model Registry UX

In the interactive CLI, `/models [query]` lists matching OpenRouter model ids
with input/output price per million tokens, context length, max output, and
modality. `/model` with no arguments shows the active model plus known registry
metadata. `/model <id>` looks up the requested id before switching; known models
show price/context details, unknown models warn, and registry fetch failures
warn without blocking the switch.

The registry fetch is lazy and cached for the session. `max_tokens` remains an
explicit override; when it is unset, `/context` reports the OpenRouter registry
context length when available. Routed ids such as `openrouter/free`,
`openrouter/auto`, or fallback `models` arrays may be served by a different
underlying model; when OpenRouter includes that model id in a response, Heddle
prints it in the REPL as `[model: provider/model-id]`. The TUI status line and
`/status` command show this as `configured-model:routed-model`, for example
`openrouter/free:openai/gpt-oss-120b`.

## JSON Schema / Taplo Autocomplete

Generated JSON schemas live in `schemas/`:

- `schemas/config.schema.json` — full config schema
- `schemas/hooks.schema.json` — hooks config schema

The `.taplo.toml` at repo root associates `.heddle/config.toml` files with the config schema, giving you autocomplete and validation in editors that support taplo.

Regenerate schemas after changing config schema definitions:

```bash
cargo run --bin export-schemas
```

## Merge Order

```
defaults → ~/.heddle/config.toml → .heddle/config.toml → env vars
```

For most fields, last value wins. Exceptions:

- **Permissions**: Kept as separate layers for precedence resolution (deny beats allow within each layer)
- **Hooks**: Merged additively (global hooks + local hooks, both fire)
- **Instructions**: Local replaces global (not concatenated)
