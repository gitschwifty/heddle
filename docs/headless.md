# Headless Mode

Heddle's headless mode exposes the agent loop over a JSON-over-stdio protocol. This is how external applications (like [Orboros](https://github.com/gitschwifty/orboros)) embed heddle as a worker.

```bash
cargo run --bin heddle-headless
# or build a standalone binary:
cargo build --release --bin heddle-headless
```

## Protocol Overview

Communication is newline-delimited JSON (JSONL) on stdin/stdout. Each line is a complete JSON object.

- **Requests** are sent to heddle on stdin
- **Responses** are written to stdout
- Every request has an `id` field for correlation
- Streaming events during a `send` are emitted as `event` responses

**Protocol version:** `0.5.0` (stored in `PROTOCOL_VERSION` file).

## Lifecycle

```
Client                          Heddle
  │                               │
  │──── init ────────────────────>│
  │<─── init_ok ─────────────────│
  │                               │
  │──── send ────────────────────>│
  │<─── event (content_delta) ───│  (repeated)
  │<─── event (tool_start) ──────│
  │<─── event (tool_end) ────────│
  │<─── event (usage) ───────────│
  │<─── event (heartbeat) ───────│  (periodic)
  │<─── result ──────────────────│
  │                               │
  │──── status ──────────────────>│
  │<─── status_ok ───────────────│
  │                               │
  │──── cancel ──────────────────>│  (during active send)
  │<─── result {cancelled} ──────│
  │                               │
  │──── shutdown ────────────────>│
  │<─── shutdown_ok ─────────────│
```

## Requests

### init

Initialize a session. Must be sent before any other request.

```json
{
  "type": "init",
  "id": "1",
  "protocol_version": "0.5.0",
  "config": {
    "model": "anthropic/claude-sonnet-4",
    "system_prompt": "You are a coding assistant.",
    "tools": ["read_file", "write_file", "edit_file", "glob", "grep", "bash"],
    "max_iterations": 10,
    "task_id": "task-abc",
    "worker_id": "worker-1",
    "app_attribution": {
      "referer": "https://github.com/gitschwifty/orboros",
      "title": "Orboros",
      "categories": "cli-agent"
    },
    "runtime": {
      "mode": "isolated",
      "state_root": "/tmp/orboros/run-42/state",
      "transcript_path": "/tmp/orboros/run-42/transcripts/worker-1.jsonl"
    },
    "routing": {
      "gateway": "openrouter",
      "upstream_provider": "anthropic",
      "grouping_id": "bench-run-42"
    }
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"init"` | yes | |
| `id` | string | yes | Request correlation ID |
| `protocol_version` | string | no | Expected protocol version |
| `config.model` | string | yes | LLM model identifier |
| `config.system_prompt` | string | yes | System prompt |
| `config.tools` | string[] | yes | Tools to enable; an empty list deliberately enables no default tools |
| `config.max_iterations` | number | no | Max agent loop iterations |
| `config.task_id` | string | no | Task ID for correlation (echoed in events/results) |
| `config.worker_id` | string | no | Worker ID for correlation |
| `config.app_attribution.referer` | string | no | OpenRouter app attribution URL; only used when `title` is also set |
| `config.app_attribution.title` | string | no | OpenRouter app attribution display name; only used when `referer` is also set |
| `config.app_attribution.categories` | string | no | Optional OpenRouter app attribution categories |
| `config.runtime.mode` | `"default"` or `"isolated"` | no | Runtime placement policy; omitted preserves current behavior |
| `config.runtime.state_root` | absolute string | required for isolated | Caller-owned absolute root for isolated mutable state |
| `config.runtime.transcript_path` | absolute string | no | Exact absolute JSONL transcript/session path for this session |
| `config.runtime.inherit_ambient_config` | boolean | no | In isolated mode, opt back into normal config/discovery; defaults to `false` |
| `config.routing.gateway` | string | no | Gateway/client identity, e.g. `openrouter` |
| `config.routing.upstream_provider` | string | no | Requested upstream provider behind a gateway |
| `config.routing.direct_provider` | string | no | Direct provider identity for future native clients |
| `config.routing.request_id` | string | no | Caller request correlation ID |
| `config.routing.grouping_id` | string | no | Provider/dashboard grouping identifier |

If `app_attribution` is omitted, or only one of `referer`/`title` is set,
provider requests use Heddle's default attribution headers.

#### Runtime placement

When `config.runtime` is absent, headless keeps its existing session placement,
config/discovery, and `HEDDLE_HOME` behavior. `mode: "isolated"` requires a
caller-supplied `state_root`; it suppresses ambient config/discovery by default
and disables stateful features that still rely on global paths. This avoids a
worker writing into normal user state. `transcript_path` may also be used with
the default mode when only the transcript destination should change.

Runtime placement paths must be absolute. Heddle rejects relative paths at
`init` instead of resolving them after a caller has changed the worker cwd.
For coding workers, send the exact Heddle tool names needed (for example
`read_file`, `write_file`, `edit_file`, `glob`, `grep`, and `bash`). An empty
`tools` array is an explicit deny-all allowlist; an attempted unavailable tool
returns the enabled tool list in its result.

The runtime policy is per init request. Heddle does not mutate process-wide
environment variables such as `HEDDLE_HOME` to implement it.

The eval runner uses fresh temporary task workspaces and writes explicit result
artifacts, but it is not a substitute for this runtime policy. Use isolated
headless mode for benchmark workers that need session/state/transcript isolation.

### send

Send a user message and start the agent loop.

```json
{
  "type": "send",
  "id": "2",
  "message": "Read src/lib.rs and explain it."
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"send"` | yes | |
| `id` | string | yes | Request ID (referenced as `send_id` in events) |
| `message` | string | yes | User message content |

### status

Query the current session state.

```json
{
  "type": "status",
  "id": "3"
}
```

### cancel

Abort an in-progress send. The `target_id` must match the `id` of the send request to cancel. Tools receive an AbortSignal and should stop gracefully.

```json
{
  "type": "cancel",
  "id": "4",
  "target_id": "2"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `target_id` | string | yes | The `id` of the send request to cancel |

### shutdown

Gracefully shut down the session.

```json
{
  "type": "shutdown",
  "id": "5"
}
```

## Responses

### init_ok

Returned after successful initialization.

```json
{
  "type": "init_ok",
  "id": "1",
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "protocol_version": "0.5.0",
  "capabilities": {
    "enabled_tools": ["read_file", "glob", "grep"],
    "explicit_tool_allowlist": true,
    "runtime_modes": ["default", "isolated"],
    "transcript_placement": true,
    "failure_details_version": "v2",
    "routing_request_metadata": true,
    "effective_routing_metadata": true,
    "cache_usage_metrics": true,
    "cancellation": true,
    "turn_state_events": true
  },
  "runtime": {
    "mode": "isolated",
    "state_root": "/tmp/orboros/run-42/state",
    "transcript_path": "/tmp/orboros/run-42/transcripts/worker-1.jsonl"
  },
  "routing": {
    "gateway": "openrouter",
    "upstream_provider": "anthropic",
    "grouping_id": "bench-run-42"
  }
}
```

`capabilities` is an additive feature inventory for this initialized session.
`enabled_tools` is the final, sorted allowlist after session setup; it is not a
catalogue of every tool compiled into heddle. Clients must treat every other
capability flag as optional for forward compatibility.

`profile` is a non-secret reproducibility identity. Its SHA-256 fingerprint
uses only the model, enabled tools, requested runtime mode, max iterations,
and whether permission or hook configuration is present. It never contains
prompt text, credentials, permission patterns, hook commands, or paths.

`routing` is the legacy combined view. New clients should use
`requested_routing` for the caller-supplied routing request and
`effective_routing` for values actually observed from a provider response. The
latter is omitted until heddle has an observed value, and never echoes request
metadata such as `request_id` or `grouping_id`.

If protocol versions are incompatible:

```json
{
  "type": "init_ok",
  "id": "1",
  "session_id": "",
  "protocol_version": "0.5.0",
  "error": {
    "code": "protocol_version_mismatch",
    "message": "Client requested 0.1.0, server is 0.5.0",
    "retryable": false
  }
}
```

### event

Streaming events emitted during an active send. All events include:

| Field | Type | Description |
|-------|------|-------------|
| `type` | `"event"` | |
| `event` | object | The event payload (see below) |
| `event_seq` | number | Monotonic counter, 0-based per send |
| `send_id` | string | The `id` of the originating send request |
| `session_id` | string? | Session ID (if task_id/worker_id were in init) |
| `task_id` | string? | Echoed from init config |
| `worker_id` | string? | Echoed from init config |

#### content_delta

A text token from the LLM response.

```json
{ "event": "content_delta", "text": "Here's what" }
```

#### turn_state

Additive lifecycle observation for a send. A normal send emits `queued`, then
`running`, and always ends with `completed` before its final `result`.
Cancellation additionally emits `cancelling` when observed. Event ordering is
defined by `event_seq`; `result` remains the canonical terminal record.

```json
{ "event": "turn_state", "state": "running" }
```

#### tool_start

A tool invocation has started.

```json
{ "event": "tool_start", "name": "read_file", "args": { "file_path": "src/lib.rs" } }
```

#### tool_end

A tool invocation completed.

```json
{ "event": "tool_end", "name": "read_file", "result_preview": "import { ... } (truncated)" }
```

#### usage

Token usage for the current LLM call.

```json
{
  "event": "usage",
  "prompt_tokens": 1500,
  "completion_tokens": 200,
  "total_tokens": 1700,
  "cost_micros": 123,
  "cost_currency": "USD",
  "cached_tokens": 400,
  "cache_write_tokens": 0,
  "reasoning_tokens": 25,
  "generation_id": "gen-..."
}
```

`cost_micros`, `cost_currency`, token detail fields, and `generation_id` are
optional. `generation_id` is the provider response/chunk `id` and can be used
with provider-side usage/audit endpoints when supported.

#### routed_model

The provider reported the concrete model that served the current response. This
is useful for routed aliases such as `openrouter/free`; `model` in `status_ok`
remains the configured model.

```json
{ "event": "routed_model", "model": "openai/gpt-oss-120b" }
```

#### error

An error occurred during processing.

```json
{
  "event": "error",
  "code": "provider_error",
  "message": "Rate limited",
  "retryable": true,
  "provider": "openrouter",
  "details": null
}
```

#### permission_request

A tool requires approval (when approval mode is set).

```json
{ "event": "permission_request", "name": "bash", "reason": "bash (execute) requires approval in suggest mode" }
```

#### permission_denied

A tool was denied execution.

```json
{ "event": "permission_denied", "name": "bash", "reason": "User denied" }
```

#### plan_complete

Plan mode completed (when `approval_mode` is `plan`).

```json
{ "event": "plan_complete", "plan": "1. Read the file\n2. Identify the bug\n3. Fix it" }
```

#### context_prune

Context was pruned to reduce size.

```json
{
  "event": "context_prune",
  "messages_pruned": 5,
  "tokens_before": 45000,
  "tokens_after": 28000
}
```

#### context_compact

Context was compacted using the weak model. (Schema defined, emission not yet implemented — reserved for future use.)

```json
{ "event": "context_compact" }
```

#### context_handoff

Context handoff marker. (Schema defined, reserved for future use.)

```json
{ "event": "context_handoff" }
```

#### heartbeat

Periodic alive signal during active sends.

```json
{ "event": "heartbeat", "duration_ms": 5200 }
```

Interval is configurable via `HEDDLE_HEARTBEAT_INTERVAL` env var (default: 5000ms). `duration_ms` is cumulative time since the send started.

### result

Returned when a send completes (success, error, or cancellation).

```json
{
  "type": "result",
  "id": "2",
  "status": "ok",
  "response": "The file contains a Rust module that exports...",
  "tool_calls_made": [
    { "name": "read_file", "args": { "file_path": "src/lib.rs" } }
  ],
  "usage": {
    "prompt_tokens": 2000,
    "completion_tokens": 500,
    "total_tokens": 2500,
    "cost_micros": 456,
    "cost_currency": "USD",
    "cached_tokens": 1000,
    "cache_write_tokens": 0,
    "reasoning_tokens": 50,
    "generation_id": "gen-..."
  },
  "iterations": 2,
  "session_id": "550e8400-...",
  "task_id": "task-abc",
  "worker_id": "worker-1",
  "model_latency_ms": 1200,
  "tool_latency_ms": 50,
  "total_latency_ms": 1250,
  "runtime": {
    "mode": "isolated",
    "state_root": "/tmp/orboros/run-42/state",
    "transcript_path": "/tmp/orboros/run-42/transcripts/worker-1.jsonl"
  },
  "routing": {
    "gateway": "openrouter",
    "upstream_provider": "anthropic"
  },
  "requested_routing": {
    "gateway": "openrouter",
    "upstream_provider": "anthropic"
  },
  "effective_routing": {
    "routed_model": "anthropic/claude-sonnet-4",
    "upstream_provider": "anthropic"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Matches the send request `id` |
| `status` | string | `"ok"` or `"error"` |
| `response` | string? | Final text response from the agent |
| `tool_calls_made` | array | List of tools invoked during this send |
| `usage` | object? | Aggregate token usage |
| `iterations` | number | Number of agent loop iterations |
| `error` | ErrorEnvelope? | Present if status is `"error"` |
| `model_latency_ms` | number? | LLM inference time |
| `tool_latency_ms` | number? | Tool execution time |
| `total_latency_ms` | number? | End-to-end time |
| `session_id` | string? | Session ID |
| `task_id` | string? | Echoed from init |
| `worker_id` | string? | Echoed from init |
| `runtime` | object? | Effective mode and actual state/transcript paths |
| `routing` | object? | Legacy combined routing metadata |
| `requested_routing` | object? | Exact routing request supplied at init |
| `effective_routing` | object? | Provider/runtime facts observed during the turn |
| `failure` | object? | Structured termination details when `status` is `"error"` |

`failure` includes a stable code, termination reason, final iteration count,
tool-call count, and final tool ID/name/parsed arguments when applicable. For
loop detection it also carries the observed count and configured threshold.
Provider failures may include provider name, HTTP status/category,
`retry_after_ms`, provider error type, and provider code. Permission denials
carry the denied tool and call ID; malformed tool-call evidence carries the
call ID/name with `args: null`; user-requested cancellation carries
`cancellation_source: "user"`. A field is omitted when its source data is not
available—particularly retry count, which the current provider interface does
not expose. Codes include
`loop_detected`, `max_iterations`, `provider_error`, `tool_error`, and
`cancelled`; clients should use the code rather than parsing the message.

#### Cancelled result

```json
{
  "type": "result",
  "id": "2",
  "status": "error",
  "tool_calls_made": [],
  "iterations": 0,
  "error": {
    "code": "cancelled",
    "message": "cancelled",
    "retryable": false
  }
}
```

### status_ok

```json
{
  "type": "status_ok",
  "id": "3",
  "model": "anthropic/claude-sonnet-4",
  "last_routed_model": "openai/gpt-oss-120b",
  "messages_count": 12,
  "session_id": "550e8400-...",
  "active": false,
  "runtime": {
    "mode": "isolated",
    "state_root": "/tmp/orboros/run-42/state",
    "transcript_path": "/tmp/orboros/run-42/transcripts/worker-1.jsonl"
  }
}
```

`last_routed_model` is omitted until the provider reports a routed model. For
`openrouter/free`, clients can display `model:last_routed_model` to show both the
configured alias and the concrete model from the last response.

When runtime placement or routing metadata was supplied at init, `status_ok`
also reports runtime metadata, the legacy `routing` view, and
`requested_routing`. `effective_routing` is populated only after a provider
reports a routed model or upstream provider; it is never inferred from the
request.

### shutdown_ok

```json
{
  "type": "shutdown_ok",
  "id": "5"
}
```

## Error Envelope

All structured errors use the same envelope:

```json
{
  "code": "provider_error",
  "message": "Rate limited by upstream provider",
  "retryable": true,
  "details": null
}
```

| Code | Retryable | Description |
|------|-----------|-------------|
| `provider_error` | yes | LLM provider returned an error |
| `protocol_error` | no | Malformed request or missing fields |
| `protocol_version_mismatch` | no | Major version incompatibility |
| `tool_error` | no | Tool execution failed |
| `loop_detected` | no | Doom loop — identical tool calls repeated |
| `cancelled` | no | Send was cancelled via cancel request |
| `max_iterations` | no | Agent loop reached its configured iteration cap |

## Protocol Compatibility

Protocol versioning follows semver. Clients and servers are compatible if their MAJOR versions match.

| Change Type | Version Bump |
|-------------|-------------|
| Remove/rename required field | MAJOR |
| Add required field | MAJOR |
| Change field type or meaning | MAJOR |
| Add optional field or new event type | MINOR |
| Bug fixes, no schema changes | PATCH |

Clients must ignore unknown fields. Unknown event types should be treated as no-ops and not cause errors.

See [compatibility.md](../compatibility.md) for the full compatibility policy and changelog.
