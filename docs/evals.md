# Eval timing

Each eval case artifact records four monotonic latency fields in milliseconds:

- `wall_latency_ms`: elapsed time for the complete attempt.
- `model_latency_ms`: time spent in model-provider calls. Provider-managed
  retries and backoff are included. If a task timeout cancels an in-flight
  request, the observed partial request duration is retained.
- `tool_latency_ms`: elapsed time from each emitted `ToolStart` through its
  matching `ToolEnd`. A tool interrupted by cancellation is counted through
  attempt teardown.
- `harness_latency_ms`: `wall_latency_ms - model_latency_ms - tool_latency_ms`,
  clamped at zero. It covers workspace setup, event scheduling, scoring,
  serialization, and other harness work.

All values use Rust's monotonic `Instant`; host sleep therefore contributes to
elapsed wall/model/tool time when it occurs during an active span. Heddle does
not claim to know provider queue or inference time separately unless a provider
reports it.

The main `summary.md` table includes wall time. A final case record aggregates
all attempts and any inter-attempt retry delay; each transient attempt keeps
its own breakdown in `retry_attempts` and its separate retry artifact. Compact
trace and error artifacts repeat the breakdown so a provider failure and a
local tool failure can be diagnosed without opening a full transcript. The
legacy `duration_ms` field remains as an alias of `wall_latency_ms` for
existing aggregate readers.
