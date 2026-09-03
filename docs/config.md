# Configuration

Heddle uses a two-layer TOML configuration: global settings in `~/.heddle/config.toml` and project-specific overrides in `.heddle/config.toml`. Values merge with last-write-wins: defaults < global < local < env vars.

## Config File Locations

| Location | Purpose |
|---|---|
| `~/.heddle/config.toml` | Global defaults (model, credential reference, system prompt) |
| `.heddle/config.toml` | Project overrides (checked in or gitignored) |

Override the global config directory with `HEDDLE_HOME`:

```bash
HEDDLE_HOME=.heddle-dev cargo run --bin heddle   # use a dev config
```

## Full Reference

```toml
# ── Identity / API ──────────────────────────────────
# Store the actual value in macOS Keychain first:
# security add-generic-password -U -s heddle -a openrouter -w
# Heddle checks keychain:heddle/openrouter by default. `OPENROUTER_API_KEY`
# overrides Keychain/config (useful for CI and headless).
# `api_key = "sk-or-..."` remains supported as a legacy plaintext fallback.

# Select Straitly instead of the default OpenRouter router when needed.
# Its default endpoint is https://api.straitly.ai/v1.

# ── Router endpoint ─────────────────────────────────
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
openrouter_routing = "balanced" # "balanced" | "nitro" | "exacto"

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

# ── Developer Sandbox ───────────────────────────────
[sandbox]
# Named Bash policy for interactive sessions. The default is "developer".
# Evals always enforce "strict", regardless of this setting. Headless workers
# retain the configured profile (and therefore default to "developer").
profile = "developer"            # "developer" | "strict"
# Extra host paths that the experimental developer Bash sandbox must neither
# read nor write. Entries must be absolute; keep personal paths in global
# ~/.heddle/config.toml rather than committing them to project config.
deny_paths = ["/Users/me/private", "/Volumes/work-secrets"]

# Headless workers can read the same TOML shape from an explicit file passed as
# config.runtime.config_path in their init request. This is particularly useful
# with isolated headless mode, which otherwise intentionally avoids ambient
# global and project config discovery.
# By default, headless ignores every TOML credential reference (including this
# one) and uses only the selected router's inherited environment variable.

# ── Hooks ───────────────────────────────────────────
# See docs/hooks.md for full reference
[[hooks.pre_tool]]
command = "my-guardrail"
matchers = { tool = "bash" }

# ── Router credentials (optional override) ───────────
# Heddle defaults to keychain:heddle/openrouter. Keep this table at the end:
# TOML fields after a table header belong to it.
[routers]
active = "straitly" # "openrouter" | "straitly"

[routers.straitly]
credential = "keychain:heddle/straitly"

# To use OpenRouter (the default) with a non-default Keychain item instead:
# [routers.openrouter]
# credential = "keychain:work/openrouter"
```

## Environment Variable Overrides

| Env Var | Overrides |
|---|---|
| `OPENROUTER_API_KEY` | OpenRouter credential (overrides Keychain/config; useful for CI/headless) |
| `STRAITLY_API_KEY` | Straitly credential when `routers.active = "straitly"` |
| `HEDDLE_MODEL` | `model` |
| `HEDDLE_WEAK_MODEL` | `weak_model` |
| `HEDDLE_BASE_URL` | `base_url` |
| `HEDDLE_MAX_TOKENS` | `max_tokens` |
| `HEDDLE_TEMPERATURE` | `temperature` |
| `HEDDLE_OPENROUTER_ROUTING` | `openrouter_routing` |
| `HEDDLE_APP_REFERER` + `HEDDLE_APP_TITLE` | `app_attribution.referer` + `app_attribution.title` |
| `HEDDLE_APP_CATEGORIES` | `app_attribution.categories` |
| `HEDDLE_APPROVAL_MODE` | `approval_mode` |
| `HEDDLE_TOOLS` | `tools` (comma-separated) |
| `HEDDLE_WEB_FETCH_ALLOW_PRIVATE_ADDRESSES` | `web_fetch_allow_private_addresses` |
| `HEDDLE_HOME` | Global config directory |

For OpenRouter credentials, the order is `OPENROUTER_API_KEY` → a usable
credential reference (default: `keychain:heddle/openrouter`) → legacy
`api_key`. If the default or configured reference cannot be read, Heddle keeps
checking the remaining sources and reports no credential only when none is
available. Env vars always win over file config.

## Headless Router Credentials

Headless workers deliberately do **not** read ambient Keychain references by
default. Their `init.config.credential_source` is optional and defaults to:

```json
{ "source": "environment" }
```

In that mode, Heddle uses only `OPENROUTER_API_KEY` or `STRAITLY_API_KEY` for
the selected router. A supervisor such as Orboros should resolve its own
credential and pass it to the worker through that environment variable; raw
credentials must never be included in the JSONL protocol.

Select the actual request router directly in the same `init.config` object:

```json
{ "router": "straitly" }
```

The only supported values are `"openrouter"` and `"straitly"`. This takes
precedence over any TOML router selection, so an isolated worker does not need
a config file merely to choose its gateway. It is separate from optional
`routing` metadata such as `upstream_provider` or `grouping_id`.

Keychain use is an explicit opt-in per worker:

```json
{
  "source": "keychain",
  "reference": "keychain:heddle/straitly"
}
```

The reference is non-secret metadata. With this choice, Heddle resolves only
that Keychain item for the selected router. An unavailable opted-in reference
causes initialization to fail; it never falls back to ambient TOML credentials.

`balanced` leaves OpenRouter's default provider routing intact. `nitro` prefers
highest-throughput providers. `exacto` requests OpenRouter's quality-first
provider variant, which is useful when tool-call reliability matters more than
price or speed. Heddle already sends tools for agent turns, so OpenRouter's
Auto Exacto may apply under `balanced`; use `exacto` to request it explicitly
for every request, including summaries and compaction.

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
