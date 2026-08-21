//! Heddle eval harness runner.
//!
//! Loads task + prompt fixtures from an eval directory (default
//! `evals/` in the repository, or `--evals <path>`), runs each
//! (task, prompt) pair against the agent loop, and scores outcome +
//! efficiency + cost.
//!
//! See `evals/README.md` for the prompt/task format.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::{Provider, ProviderConfig, ProviderTelemetry, RetryConfig};
use heddle::tools::bash::create_workspace_bash_tool;
use heddle::tools::edit::create_workspace_edit_tool;
use heddle::tools::glob::create_workspace_glob_tool;
use heddle::tools::grep::create_workspace_grep_tool;
use heddle::tools::read::create_workspace_read_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::HeddleTool;
use heddle::tools::web_fetch::create_web_fetch_tool;
use heddle::tools::workspace::WorkspaceBoundary;
use heddle::tools::write::create_workspace_write_tool;
#[cfg(test)]
use heddle::tools::{
    create_bash_tool, create_edit_tool, create_glob_tool, create_grep_tool, create_read_tool,
    create_write_tool,
};
use heddle::types::{Message, SystemMessage, UserMessage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use walkdir::WalkDir;

#[path = "../eval_aggregation.rs"]
mod eval_aggregation;
use eval_aggregation::cmd_aggregate;

// ─── CLI ─────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "eval", about = "Heddle eval harness")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// List tasks and prompts in the eval directory.
    List {
        #[arg(long, default_value = "evals")]
        evals: PathBuf,
    },
    /// Run one or more (task, prompt) pairs.
    Run {
        #[arg(long, default_value = "evals")]
        evals: PathBuf,
        /// Comma-separated prompt ids, or "all".
        #[arg(long, default_value = "all")]
        prompts: String,
        /// Comma-separated task ids, or "all".
        #[arg(long, default_value = "all")]
        tasks: String,
        /// Comma-separated task tags. Applied after --tasks selection; use
        /// "all" to leave the selected task set unchanged.
        #[arg(long, default_value = "all")]
        tags: String,
        /// Model id (defaults to manifest.default_model).
        #[arg(long)]
        model: Option<String>,
        /// Hard cap on tokens per task (default 10000).
        #[arg(long, default_value_t = 10_000)]
        max_tokens_per_task: u64,
        /// Hard cap on a single model response, sent as `max_tokens` to
        /// the provider. This is the load-bearing cost guard — the session
        /// budget only fires *after* a response arrives.
        #[arg(long, default_value_t = 1500)]
        max_tokens_per_response: u32,
        /// Fallback cap for tasks that do not declare max_turns (default 8).
        #[arg(long, default_value_t = 8)]
        max_turns: u32,
        /// Wall-clock timeout per task, in seconds (default 150).
        #[arg(long, default_value_t = 150)]
        task_timeout_secs: u64,
        /// Abort the sweep if cumulative cost crosses this USD value.
        #[arg(long)]
        budget_stop_usd: Option<f64>,
        /// Write results under this directory. Overrides the generated result name.
        #[arg(long)]
        results_dir: Option<PathBuf>,
        /// Optional label appended to the generated result directory name.
        #[arg(long, conflicts_with = "results_dir")]
        tag: Option<String>,
        /// Human-readable suite family label. Required to create a new
        /// non-default suite root when no matching fingerprint exists.
        #[arg(long, conflicts_with = "results_dir")]
        suite_label: Option<String>,
        /// Number of times to run each (task, prompt) pair. When >1, the
        /// summary aggregates with mean ± stddev per pair. Useful for
        /// averaging out LLM stochastic variance.
        #[arg(long, default_value_t = 1)]
        runs: u32,
        /// Include assistant text for passing runs too. Failed runs always
        /// include assistant text for diagnosis.
        #[arg(long, default_value_t = false)]
        record_all_text: bool,
        /// Prewarm each selected prompt's stable instruction prefix, then use
        /// cache-friendly routing for the sweep. Requires a paid pinned model.
        #[arg(long, default_value_t = false)]
        cache_prewarm: bool,
        /// Request Anthropic's one-hour cache TTL. Requires --cache-prewarm
        /// and an anthropic/* model; provider-default TTL is otherwise used.
        #[arg(long, default_value_t = false)]
        cache_ttl_1h: bool,
        /// Run only prompts without cwd, date, git, or file-tree context.
        /// Explicitly selected dynamic prompts fail instead of being skipped.
        #[arg(long, default_value_t = false)]
        static_context_only: bool,
        /// TOML declaration of the single harness-change hypothesis being
        /// evaluated. Its resolved identity is retained in run metadata and
        /// aggregate comparison profiles.
        #[arg(long)]
        condition: Option<PathBuf>,
    },
    /// Rebuild cross-run reports from completed eval artifacts.
    Aggregate {
        /// Promoted run root to scan when --run is omitted.
        #[arg(long, default_value = "evals/results")]
        results_root: PathBuf,
        /// Completed run directory to include. Repeat to select runs
        /// explicitly; otherwise all completed runs beneath --results-root
        /// are considered.
        #[arg(long = "run")]
        runs: Vec<PathBuf>,
        /// Human-readable suite label used in the aggregate directory.
        #[arg(long, default_value = "suite")]
        suite_label: String,
        /// Human-readable comparison-profile label used in the aggregate directory.
        #[arg(long, default_value = "default")]
        profile_label: String,
        /// Root directory for managed aggregate reports.
        #[arg(long, default_value = "evals/results/aggregates")]
        aggregate_root: PathBuf,
        /// Override the generated aggregate output directory.
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Compare a harness change or single-prompt condition with identical
    /// non-prompt controls.
    Compare {
        /// Completed baseline run directory.
        #[arg(long)]
        baseline: PathBuf,
        /// Completed variant run directory.
        #[arg(long)]
        variant: PathBuf,
        /// Optional comparison name; defaults to the variant condition or
        /// prompt-pair ID.
        #[arg(long)]
        name: Option<String>,
        /// Optional suffix for another comparison with the same name.
        #[arg(long)]
        tag: Option<String>,
        /// Print the complete machine-readable report instead of the review summary.
        #[arg(long)]
        json: bool,
    },
}

// ─── Manifest / prompt / task schemas ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default = "default_version")]
    version: String,
    #[serde(default)]
    default_model: Option<String>,
    /// Family label for the full matrix and named smoke profile. The suite
    /// fingerprint still distinguishes behaviorally different selections.
    #[serde(default)]
    suite_label: Option<String>,
    #[serde(default)]
    defaults: ManifestDefaults,
}

fn default_version() -> String {
    "0.0.0".into()
}

#[derive(Debug, Deserialize, Default)]
struct ManifestDefaults {
    #[allow(dead_code)]
    max_turns: Option<u32>,
    #[allow(dead_code)]
    max_tokens_per_task: Option<u64>,
    budget_stop_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct EvalCondition {
    id: String,
    hypothesis: String,
    baseline: String,
    variant: String,
    changed_factor: String,
    expected_signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ResolvedEvalCondition {
    #[serde(flatten)]
    declaration: EvalCondition,
    fingerprint: String,
}

fn load_eval_condition(path: &Path) -> Result<ResolvedEvalCondition> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let declaration: EvalCondition =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    for (name, value) in [
        ("id", &declaration.id),
        ("hypothesis", &declaration.hypothesis),
        ("baseline", &declaration.baseline),
        ("variant", &declaration.variant),
        ("changed_factor", &declaration.changed_factor),
        ("expected_signal", &declaration.expected_signal),
    ] {
        if value.trim().is_empty() {
            bail!("condition {} must not be empty in {}", name, path.display());
        }
    }
    if declaration.baseline == declaration.variant {
        bail!(
            "condition baseline and variant must differ in {}",
            path.display()
        );
    }
    let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&declaration)?));
    Ok(ResolvedEvalCondition {
        declaration,
        fingerprint,
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PromptFrontMatter {
    id: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// Stable comparison role retained with run and aggregate provenance.
    #[serde(default)]
    role: Option<String>,
    /// Falsifiable behavior claim for this independently selectable prompt.
    #[serde(default)]
    hypothesis: Option<String>,
    #[serde(default)]
    context: ContextConfig,
    /// When true, skip this prompt when running `--prompts all`. The prompt
    /// is still selectable by explicit name. Use for retired prompts kept
    /// for reference, or known-failing baselines.
    #[serde(default)]
    matrix_exclude: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct ContextConfig {
    #[serde(default)]
    cwd: bool,
    #[serde(default)]
    date: bool,
    #[serde(default)]
    git: Option<GitConfig>,
    #[serde(default)]
    file_tree: Option<FileTreeConfig>,
}

impl ContextConfig {
    fn dynamic_features(&self) -> Vec<&'static str> {
        let mut features = Vec::new();
        if self.cwd {
            features.push("cwd");
        }
        if self.date {
            features.push("date");
        }
        if self.git.is_some() {
            features.push("git");
        }
        if self.file_tree.is_some() {
            features.push("file_tree");
        }
        features
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GitConfig {
    #[serde(default)]
    branch: bool,
    #[serde(default)]
    status: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct FileTreeConfig {
    #[serde(default = "default_tree_depth")]
    max_depth: usize,
    #[serde(default = "default_tree_entries")]
    max_entries: usize,
}

fn default_tree_depth() -> usize {
    2
}
fn default_tree_entries() -> usize {
    50
}

#[derive(Debug, Clone)]
struct Prompt {
    id: String,
    front: PromptFrontMatter,
    body: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskSpec {
    id: String,
    prompt: String,
    /// Capability/category labels used by --tags selection and reports.
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    max_turns: Option<u32>,
    /// Per-task wall-clock timeout, in seconds. When omitted, the CLI
    /// `--task-timeout-secs` default applies.
    #[serde(default)]
    task_timeout_secs: Option<u64>,
    /// Per-task override for `--max-tokens-per-task`. When set, this wins
    /// over the CLI default for this task — different tasks need different
    /// budgets (a 1-line write is 2k; a 3-file refactor on a slow model
    /// needs 30k+). Use the CLI flag to bump everything globally.
    #[serde(default)]
    budget_tokens: Option<u64>,
    /// Smoke tasks are harness validators, not prompt discriminators. They
    /// only run against the `default` prompt in matrix mode regardless of
    /// `--prompts`, so they verify the runner works without polluting the
    /// matrix table. Use `--include-smoke-matrix` to bypass.
    #[serde(default)]
    smoke: bool,
    score: TaskScoreSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TaskScoreSpec {
    outcome: OutcomeSpec,
    /// Optional deterministic acceptance check for behavior/API fixtures.
    /// It is only considered after a completed run does not exact-match the
    /// expected tree (or when no expected tree is configured).
    #[serde(default)]
    semantic_verification: Option<SemanticVerificationSpec>,
    #[serde(default)]
    efficiency: Option<EfficiencySpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OutcomeSpec {
    /// Exact source-tree oracle. Omit for an intentionally open-ended
    /// API/behavior fixture that is scored entirely by semantic verification.
    #[serde(default)]
    expected_dir: Option<String>,
    #[serde(default)]
    ignore_globs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SemanticVerificationSpec {
    /// Executable followed by arguments. Shell parsing is intentionally not
    /// supported so fixture commands remain inspectable and deterministic.
    command: Vec<String>,
    #[serde(default = "default_verification_timeout_secs")]
    timeout_secs: u64,
    #[serde(default = "default_verification_max_output_bytes")]
    max_output_bytes: usize,
    /// Agent-owned paths which must remain byte-for-byte unchanged before the
    /// hidden verifier is staged into the workspace.
    #[serde(default)]
    protected_globs: Vec<String>,
}

fn default_verification_timeout_secs() -> u64 {
    30
}

fn default_verification_max_output_bytes() -> usize {
    4_096
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EfficiencySpec {
    #[serde(default)]
    min_tool_calls: Option<u32>,
    #[serde(default)]
    max_tool_calls: Option<u32>,
}

#[derive(Debug, Clone)]
struct Task {
    dir: PathBuf,
    spec: TaskSpec,
}

// ─── Result schema ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct TaskResult {
    task_id: String,
    prompt_id: String,
    /// Task capability labels, retained in result artifacts so aggregate
    /// reports can show which classes of work produced failures.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    /// Requested model. Run metadata owns this for persisted results, so do
    /// not repeat it in every per-case or aggregate result record.
    #[serde(default, skip_serializing)]
    model: String,
    heddle_commit: String,
    evals_version: String,
    timestamp: String,
    duration_ms: u128,
    scores: Scores,
    rendered_system_prompt_chars: usize,
    /// Fingerprints of the exact system-prompt messages and selected tool
    /// schemas supplied for this case. These are safe to retain in normal
    /// artifacts without copying prompt or schema bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_contract: Option<EvalInputContract>,
    /// Provider-reported model for each assistant response. This preserves
    /// switches behind routed aliases such as `openrouter/free`. Pinned runs
    /// omit this when every observed model equals the requested model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    routed_models: Vec<RoutedModelObservation>,
    /// Upstream provider observed on each assistant response. Unlike a
    /// requested routing policy this only records provider facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    upstream_providers: Vec<UpstreamProviderObservation>,
    /// Provider generation IDs observed for successful model responses in this
    /// case. These make eval artifacts joinable to provider-side logs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    generation_ids: Vec<String>,
    /// Correlation metadata retained for provider failures, including calls
    /// that failed before they emitted usage or an assistant response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    provider_telemetry: Vec<ProviderTelemetry>,
    /// Sanitized diagnostics retained only in this run's separate debug log.
    #[serde(skip, default)]
    debug_errors: Vec<ProviderDebugError>,
    /// Supplementary per-call evidence, written only to call-telemetry.jsonl.
    #[serde(skip, default)]
    call_telemetry: Vec<CallTelemetry>,
    /// 1-indexed run number when --runs N. 0 if single-run.
    #[serde(default)]
    run_index: u32,
    /// Transient provider/transport failures retried from a fresh workspace
    /// and conversation. The final result remains one matrix observation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retry_attempts: Vec<RetryAttempt>,
    /// Order of tool calls (names only). Useful for diagnosing why a task
    /// failed without re-reading the result JSON.
    tool_sequence: Vec<String>,
    /// Provider finish reasons for each assistant message. Kept for all runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    finish_reasons: Vec<String>,
    /// Assistant text is stored only for failures by default. Use
    /// `--record-all-text` to include it for passing runs too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    assistant_messages: Vec<AssistantTrace>,
    /// Bounded durable execution evidence. Unlike the full transcript this is
    /// retained in the per-case artifact after transcript cleanup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trace: Option<CompactTrace>,
    /// Complete model-facing conversation retained only in the per-run
    /// transcript artifact, not duplicated in the result JSON.
    #[serde(skip, default)]
    transcript: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EvalInputContract {
    rendered_system_prompt_sha256: String,
    tool_schema_sha256: String,
    tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetryAttempt {
    attempt: u32,
    cause: FailureCause,
    error: String,
    duration_ms: u128,
    cost: CostScore,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    provider_telemetry: Vec<ProviderTelemetry>,
    #[serde(skip, default)]
    debug_errors: Vec<ProviderDebugError>,
    #[serde(skip, default)]
    call_telemetry: Vec<CallTelemetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_delay_ms: Option<u64>,
}

impl RetryAttempt {
    fn from_result(result: &TaskResult, attempt: u32) -> Option<Self> {
        Some(Self {
            attempt,
            cause: failure_cause(result)?,
            error: result.scores.error.clone()?,
            duration_ms: result.duration_ms,
            cost: result.scores.cost.clone(),
            provider_telemetry: result.provider_telemetry.clone(),
            debug_errors: result.debug_errors.clone(),
            call_telemetry: result.call_telemetry.clone(),
            retry_reason: None,
            retry_delay_ms: None,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AssistantTrace {
    turn: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    text: String,
}

const TRACE_MAX_TOOL_EVENTS: usize = 32;
const TRACE_MAX_FAILURES: usize = 8;
const TRACE_MAX_FAILURE_BYTES: usize = 2_048;

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Serialize, Deserialize)]
struct CompactTrace {
    assistant_turns: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_sequence: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tool_counts: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_failures: Vec<ToolFailureTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    finish_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_cause: Option<FailureCause>,
    #[serde(skip_serializing_if = "Option::is_none")]
    routed_model_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_provider_change: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolFailureTrace {
    name: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutedModelObservation {
    assistant_turn: u32,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpstreamProviderObservation {
    assistant_turn: u32,
    provider: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderDebugError {
    timestamp: String,
    detail: String,
    telemetry: ProviderTelemetry,
}

/// One safe, decoded provider-call observation. This intentionally lives only
/// in the invocation-level JSONL log so normal case artifacts stay compact.
#[derive(Debug, Clone, Serialize)]
struct CallTelemetry {
    task_id: String,
    prompt_id: String,
    run_index: u32,
    attempt: u32,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_turn: Option<u32>,
    requested_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    routed_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_telemetry: Option<ProviderTelemetry>,
}

impl CallTelemetry {
    fn new(task: &Task, prompt: &Prompt, model: &str) -> Self {
        Self {
            task_id: task.spec.id.clone(),
            prompt_id: prompt.id.clone(),
            run_index: 0,
            attempt: 1,
            timestamp: Utc::now().to_rfc3339(),
            assistant_turn: None,
            requested_model: model.to_string(),
            routed_model: None,
            provider: None,
            generation_id: None,
            finish_reason: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            usd: None,
            provider_telemetry: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Scores {
    outcome: OutcomeScore,
    efficiency: EfficiencyScore,
    cost: CostScore,
    /// Configured or runtime guard that stopped an incomplete task. Older
    /// artifacts derive max-turn limits from their legacy error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<LimitReason>,
    /// Stable explanation for every non-passing result written by current
    /// harnesses. `error` remains the original diagnostic detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_cause: Option<FailureCause>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum FailureCause {
    NoOp,
    WrongChangedFile,
    MissingExpectedChange,
    UnexpectedExtraFile,
    WrongDiff,
    ProtectedFileChanged,
    SemanticVerificationFailed,
    MaxTurns,
    TokenBudget,
    DoomLoop,
    Timeout,
    ProviderApi,
    Transport,
    Tool,
    Permission,
    HarnessInternal,
}

impl FailureCause {
    fn label(self) -> &'static str {
        match self {
            Self::NoOp => "no_op",
            Self::WrongChangedFile => "wrong_changed_file",
            Self::MissingExpectedChange => "missing_expected_change",
            Self::UnexpectedExtraFile => "unexpected_extra_file",
            Self::WrongDiff => "wrong_diff",
            Self::ProtectedFileChanged => "protected_file_changed",
            Self::SemanticVerificationFailed => "semantic_verification_failed",
            Self::MaxTurns => "max_turns",
            Self::TokenBudget => "token_budget",
            Self::DoomLoop => "doom_loop",
            Self::Timeout => "timeout",
            Self::ProviderApi => "provider_api",
            Self::Transport => "transport",
            Self::Tool => "tool",
            Self::Permission => "permission",
            Self::HarnessInternal => "harness_internal",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LimitReason {
    MaxTurns,
    TokenBudget,
    DoomLoop,
}

impl LimitReason {
    fn label(self) -> &'static str {
        match self {
            Self::MaxTurns => "max_turns",
            Self::TokenBudget => "token_budget",
            Self::DoomLoop => "doom_loop",
        }
    }
}

impl From<LimitReason> for FailureCause {
    fn from(value: LimitReason) -> Self {
        match value {
            LimitReason::MaxTurns => Self::MaxTurns,
            LimitReason::TokenBudget => Self::TokenBudget,
            LimitReason::DoomLoop => Self::DoomLoop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultStatus {
    Pass,
    Fail,
    Limit,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
struct OutcomeScore {
    passed: bool,
    /// Whether the expected source tree matched before semantic verification.
    #[serde(default)]
    exact_passed: bool,
    diff_files: Vec<DirDiffEntry>,
    /// Protected source paths changed before a semantic verifier could run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    protected_diffs: Vec<DirDiffEntry>,
    /// Present only for fixtures configured with semantic verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    semantic_verification: Option<SemanticVerificationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SemanticVerificationResult {
    command: Vec<String>,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "is_false")]
    timed_out: bool,
    output: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DirDiffEntry {
    path: String,
    kind: String, // "missing" | "unexpected" | "differs"
}

#[derive(Debug, Serialize, Deserialize)]
struct EfficiencyScore {
    tool_calls: u32,
    turns: u32,
    /// Tool-call count fell within task.toml [min, max] range.
    tool_calls_in_range: bool,
    /// Optional task-level maximum retained so summaries can show when a
    /// correct result exceeded its tool-call guidance.
    #[serde(default)]
    max_tool_calls: Option<u32>,
    /// Whether the recorded tool-call count exceeded the task-level maximum.
    /// Absent in artifacts written before this field was introduced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exceeded_max_tool_calls: Option<bool>,
    /// Total tokens stayed under the per-task budget (CLI default or task
    /// override). When false, the task was force-aborted but still scored
    /// on whatever workspace state existed at the time.
    tokens_in_budget: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CostScore {
    tokens_in: u64,
    tokens_out: u64,
    /// Prompt tokens served from a provider cache. Zero when unavailable.
    #[serde(default)]
    cached_tokens: u64,
    /// Prompt tokens written to a provider cache. Zero when unavailable.
    #[serde(default)]
    cache_write_tokens: u64,
    // USD lookup is best-effort; 0.0 if pricing isn't loaded.
    usd: f64,
}

// ─── Loaders ─────────────────────────────────────────────────────────────

fn load_manifest(evals: &Path) -> Result<Manifest> {
    let path = evals.join("manifest.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let m: Manifest = toml::from_str(&text)?;
    Ok(m)
}

fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let s = text.trim_start_matches('\u{FEFF}');
    if let Some(rest) = s.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n');
            return (Some(front), body);
        }
    }
    (None, s)
}

fn load_prompt(path: &Path) -> Result<Prompt> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (front_yaml, body) = split_front_matter(&raw);
    let mut front: PromptFrontMatter = match front_yaml {
        Some(y) => serde_yaml::from_str(y)
            .with_context(|| format!("parsing front matter in {}", path.display()))?,
        None => PromptFrontMatter::default(),
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".into());
    if front.id.is_none() {
        front.id = Some(stem.clone());
    }
    Ok(Prompt {
        id: front.id.clone().unwrap_or(stem),
        front,
        body: body.to_string(),
    })
}

fn load_prompts(evals: &Path) -> Result<Vec<Prompt>> {
    let dir = evals.join("prompts");
    let mut out = Vec::new();
    for entry in WalkDir::new(&dir).min_depth(1).into_iter() {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(load_prompt(path)?);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    for pair in out.windows(2) {
        if pair[0].id == pair[1].id {
            bail!("duplicate prompt id {:?} in {}", pair[0].id, dir.display());
        }
    }
    Ok(out)
}

fn load_task(dir: &Path) -> Result<Task> {
    let toml_path = dir.join("task.toml");
    let text = fs::read_to_string(&toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;
    let spec: TaskSpec =
        toml::from_str(&text).with_context(|| format!("parsing {}", toml_path.display()))?;
    if spec.score.outcome.expected_dir.is_none() && spec.score.semantic_verification.is_none() {
        bail!(
            "{} must configure score.outcome.expected_dir or score.semantic_verification",
            toml_path.display()
        );
    }
    if let Some(verification) = &spec.score.semantic_verification {
        if verification.command.is_empty() {
            bail!(
                "{} semantic verification command must not be empty",
                toml_path.display()
            );
        }
        if !dir.join("verify").is_dir() {
            bail!(
                "{} configures semantic verification but has no verify/ directory",
                toml_path.display()
            );
        }
        for pattern in &verification.protected_globs {
            globset::Glob::new(pattern).with_context(|| {
                format!(
                    "invalid protected_glob {pattern:?} in {}",
                    toml_path.display()
                )
            })?;
        }
    }
    Ok(Task {
        dir: dir.to_path_buf(),
        spec,
    })
}

fn load_tasks(evals: &Path) -> Result<Vec<Task>> {
    let dir = evals.join("tasks");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() && path.join("task.toml").exists() {
            out.push(load_task(&path)?);
        }
    }
    out.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));
    Ok(out)
}

// ─── Context block renderer ──────────────────────────────────────────────

fn render_context(ctx: &ContextConfig, workspace: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    if ctx.cwd {
        parts.push(format!(
            "## Current Working Directory\n\n`{}`",
            workspace.display()
        ));
    }
    if ctx.date {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        parts.push(format!("## Today's Date\n\n{date}"));
    }
    if let Some(git) = &ctx.git {
        if let Some(block) = render_git(workspace, git) {
            parts.push(block);
        }
    }
    if let Some(ft) = &ctx.file_tree {
        parts.push(render_file_tree(workspace, ft));
    }
    parts.join("\n\n")
}

fn render_git(workspace: &Path, cfg: &GitConfig) -> Option<String> {
    if !workspace.join(".git").exists() {
        return None;
    }
    let mut lines: Vec<String> = vec!["## Git".into()];
    if cfg.branch {
        if let Ok(out) = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(workspace)
            .output()
        {
            if out.status.success() {
                let b = String::from_utf8_lossy(&out.stdout).trim().to_string();
                lines.push(format!("Branch: {b}"));
            }
        }
    }
    if cfg.status {
        if let Ok(out) = std::process::Command::new("git")
            .args(["status", "--short"])
            .current_dir(workspace)
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let s = if s.is_empty() { "(clean)".into() } else { s };
                lines.push(format!("Status:\n```\n{s}\n```"));
            }
        }
    }
    Some(lines.join("\n"))
}

fn render_file_tree(workspace: &Path, cfg: &FileTreeConfig) -> String {
    let mut entries: Vec<String> = Vec::new();
    for entry in WalkDir::new(workspace)
        .max_depth(cfg.max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if p == workspace {
            continue;
        }
        let rel = p.strip_prefix(workspace).unwrap_or(p).display().to_string();
        let suffix = if entry.file_type().is_dir() { "/" } else { "" };
        entries.push(format!("{rel}{suffix}"));
        if entries.len() >= cfg.max_entries {
            entries.push("...".into());
            break;
        }
    }
    format!("## File Tree\n\n```\n{}\n```", entries.join("\n"))
}

fn compose_system_prompt(prompt: &Prompt, workspace: &Path) -> String {
    let ctx_block = render_context(&prompt.front.context, workspace);
    let mut parts: Vec<String> = Vec::new();
    if !ctx_block.is_empty() {
        parts.push(ctx_block);
    }
    let body = prompt.body.trim();
    if !body.is_empty() {
        parts.push(body.to_string());
    }
    parts.join("\n\n")
}

fn compose_messages(prompt: &Prompt, workspace: &Path, cache_mode: bool) -> Vec<Message> {
    if !cache_mode {
        let composed = compose_system_prompt(prompt, workspace);
        return if composed.is_empty() {
            Vec::new()
        } else {
            vec![Message::System(SystemMessage { content: composed })]
        };
    }

    // Put the stable prompt body first so it remains a reusable provider-cache
    // prefix while task-specific workspace context can still vary per run.
    let mut messages = Vec::new();
    let body = prompt.body.trim();
    if !body.is_empty() {
        messages.push(Message::System(SystemMessage {
            content: body.to_string(),
        }));
    }
    let context = render_context(&prompt.front.context, workspace);
    if !context.is_empty() {
        messages.push(Message::System(SystemMessage { content: context }));
    }
    messages
}

// ─── Sandbox helpers ─────────────────────────────────────────────────────

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(from) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dst = to.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

/// Normalize file contents for diffing.
///
/// Most LLMs are inconsistent about line endings and trailing newlines —
/// `0.3.0` vs `0.3.0\n` vs `0.3.0\r\n` is noise we don't want to score on.
/// We:
///   - decode as UTF-8 (binary files: byte-compare as-is)
///   - convert CRLF -> LF
///   - strip trailing whitespace from each line
///   - strip trailing newlines from the whole file
fn normalize_for_diff(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            let normalized: String = s
                .replace("\r\n", "\n")
                .lines()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            normalized.trim_end_matches('\n').as_bytes().to_vec()
        }
        Err(_) => bytes.to_vec(),
    }
}

fn collect_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for e in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel = e
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if rel == ".keep" {
            continue;
        }
        if let Ok(b) = fs::read(e.path()) {
            out.insert(rel, b);
        }
    }
    out
}

fn diff_dirs(actual: &Path, expected: &Path) -> Vec<DirDiffEntry> {
    let mut entries = Vec::new();
    let expected_files = collect_files(expected);
    let actual_files = collect_files(actual);
    for (path, want) in &expected_files {
        match actual_files.get(path) {
            None => entries.push(DirDiffEntry {
                path: path.clone(),
                kind: "missing".into(),
            }),
            Some(got) if normalize_for_diff(got) != normalize_for_diff(want) => {
                entries.push(DirDiffEntry {
                    path: path.clone(),
                    kind: "differs".into(),
                })
            }
            _ => {}
        }
    }
    for path in actual_files.keys() {
        if !expected_files.contains_key(path) {
            entries.push(DirDiffEntry {
                path: path.clone(),
                kind: "unexpected".into(),
            });
        }
    }
    entries
}

fn protected_path_diffs(
    before: &Path,
    workspace: &Path,
    patterns: &[String],
) -> Result<Vec<DirDiffEntry>> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .with_context(|| format!("invalid protected_glob {pattern:?}"))?,
        );
    }
    let matcher = builder.build()?;
    Ok(diff_dirs(workspace, before)
        .into_iter()
        .filter(|entry| matcher.is_match(Path::new(&entry.path)))
        .collect())
}

fn truncate_verification_output(bytes: &[u8], max_bytes: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= max_bytes {
        return text.into_owned();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

async fn run_semantic_verification(
    task: &Task,
    workspace: &Path,
    spec: &SemanticVerificationSpec,
) -> Result<SemanticVerificationResult> {
    let fixture_verifier = task.dir.join("verify");
    if !fixture_verifier.is_dir() {
        bail!(
            "semantic verification for {} requires a verify/ directory",
            task.spec.id
        );
    }
    let staged_verifier = workspace.join(".heddle-verification");
    copy_dir_recursive(&fixture_verifier, &staged_verifier)
        .with_context(|| format!("staging semantic verification for {}", task.spec.id))?;
    let (program, args) = spec
        .command
        .split_first()
        .ok_or_else(|| anyhow!("semantic verification command must not be empty"))?;
    let output = match timeout(
        Duration::from_secs(spec.timeout_secs.max(1)),
        tokio::process::Command::new(program)
            .args(args)
            .current_dir(workspace)
            .output(),
    )
    .await
    {
        Ok(output) => output.with_context(|| {
            format!(
                "running semantic verification command {:?} for {}",
                spec.command, task.spec.id
            )
        })?,
        Err(_) => {
            return Ok(SemanticVerificationResult {
                command: spec.command.clone(),
                passed: false,
                exit_code: None,
                timed_out: true,
                output: format!("verification timed out after {}s", spec.timeout_secs.max(1)),
            });
        }
    };
    let mut combined = output.stdout;
    if !combined.is_empty() && !output.stderr.is_empty() {
        combined.push(b'\n');
    }
    combined.extend_from_slice(&output.stderr);
    Ok(SemanticVerificationResult {
        command: spec.command.clone(),
        passed: output.status.success(),
        exit_code: output.status.code(),
        timed_out: false,
        output: truncate_verification_output(&combined, spec.max_output_bytes),
    })
}

// ─── Tool selection ──────────────────────────────────────────────────────

#[cfg(test)]
fn tool_by_name(name: &str) -> Option<Arc<dyn HeddleTool>> {
    match name {
        "read_file" => Some(create_read_tool()),
        "write_file" => Some(create_write_tool()),
        "edit_file" => Some(create_edit_tool()),
        "glob" => Some(create_glob_tool()),
        "grep" => Some(create_grep_tool()),
        "bash" => Some(create_bash_tool()),
        "web_fetch" => Some(create_web_fetch_tool()),
        _ => None,
    }
}

#[cfg(test)]
fn build_registry(names: &[String]) -> Result<ToolRegistry> {
    let mut r = ToolRegistry::new();
    for n in names {
        let tool = tool_by_name(n).ok_or_else(|| anyhow!("unknown tool: {n}"))?;
        r.register(tool)?;
    }
    Ok(r)
}

fn build_workspace_registry(names: &[String], root: &Path) -> Result<ToolRegistry> {
    let boundary = WorkspaceBoundary::new(root).map_err(|error| anyhow!(error.to_string()))?;
    let mut registry = ToolRegistry::new();
    for name in names {
        let tool: Arc<dyn HeddleTool> = match name.as_str() {
            "read_file" => create_workspace_read_tool(boundary.clone()),
            "write_file" => create_workspace_write_tool(boundary.clone()),
            "edit_file" => create_workspace_edit_tool(boundary.clone()),
            "glob" => create_workspace_glob_tool(boundary.clone()),
            "grep" => create_workspace_grep_tool(boundary.clone()),
            "bash" => create_workspace_bash_tool(boundary.clone()),
            "web_fetch" => create_web_fetch_tool(),
            _ => return Err(anyhow!("unknown tool: {name}")),
        };
        registry.register(tool)?;
    }
    Ok(registry)
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn eval_input_contract(messages: &[Message], registry: &ToolRegistry) -> Result<EvalInputContract> {
    let system_messages: Vec<&str> = messages
        .iter()
        .filter_map(|message| match message {
            Message::System(system) => Some(system.content.as_str()),
            _ => None,
        })
        .collect();
    let mut definitions = registry.definitions();
    definitions.sort_by(|left, right| left.function.name.cmp(&right.function.name));
    let mut tools: Vec<String> = definitions
        .iter()
        .map(|definition| definition.function.name.clone())
        .collect();
    tools.sort();
    Ok(EvalInputContract {
        rendered_system_prompt_sha256: sha256_json(&system_messages)?,
        tool_schema_sha256: sha256_json(&definitions)?,
        tools,
    })
}

// ─── Runner ──────────────────────────────────────────────────────────────

const FREE_FALLBACK: &[&str] = &[
    "liquid/lfm-2.5-1.2b-instruct:free",
    "arcee-ai/trinity-large-preview:free",
    "arcee-ai/trinity-mini:free",
    "openrouter/free",
];

const FREE_MODEL_REQUEST_INTERVAL: Duration = Duration::from_millis(3_200);

fn is_free_model(model: &str) -> bool {
    model == "openrouter/free" || model.ends_with(":free")
}

struct PacedProvider {
    inner: Arc<dyn Provider>,
    interval: Duration,
    next_request: Arc<Mutex<Instant>>,
}

impl PacedProvider {
    async fn wait_for_turn(&self) {
        let mut next = self.next_request.lock().await;
        let now = Instant::now();
        let wait = next.saturating_duration_since(now);
        *next = now.max(*next) + self.interval;
        drop(next);
        if !wait.is_zero() {
            sleep(wait).await;
        }
    }
}

#[async_trait::async_trait]
impl Provider for PacedProvider {
    async fn send(
        &self,
        messages: &[Message],
        tools: Option<&[heddle::types::ToolDefinition]>,
        overrides: &serde_json::Value,
    ) -> Result<heddle::types::ChatCompletionResponse> {
        self.wait_for_turn().await;
        self.inner.send(messages, tools, overrides).await
    }

    fn stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<heddle::types::ToolDefinition>>,
        overrides: serde_json::Value,
    ) -> heddle::provider::types::ChunkStream {
        let inner = self.inner.clone();
        let interval = self.interval;
        let next_request = self.next_request.clone();
        Box::pin(
            futures::stream::once(async move {
                let pacer = PacedProvider {
                    inner: inner.clone(),
                    interval,
                    next_request,
                };
                pacer.wait_for_turn().await;
                inner.stream(messages, tools, overrides)
            })
            .flatten(),
        )
    }

    fn with(&self, overrides: serde_json::Value) -> Arc<dyn Provider> {
        Arc::new(Self {
            inner: self.inner.with(overrides),
            interval: self.interval,
            next_request: self.next_request.clone(),
        })
    }
}

#[derive(Debug, Clone)]
struct CachePrewarmConfig {
    session_id: String,
    ttl_1h: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PrewarmResult {
    prompt_id: String,
    duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    routed_model: Option<String>,
    tokens_in: u64,
    tokens_out: u64,
    cached_tokens: u64,
    cache_write_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CachePrewarmRun {
    session_id: String,
    ttl: String,
    prewarms: Vec<PrewarmResult>,
}

fn validate_cache_model(model: &str, _ttl_1h: bool) -> Result<()> {
    if model == "openrouter/auto" || model == "openrouter/free" || model.ends_with(":free") {
        bail!("--cache-prewarm requires a pinned paid model; {model:?} is routed or free");
    }
    if !model.starts_with("anthropic/") {
        bail!(
            "--cache-prewarm currently supports pinned anthropic/* models; {model:?} uses a different OpenRouter caching path"
        );
    }
    Ok(())
}

fn make_provider(
    model: &str,
    api_key: String,
    max_tokens_per_response: u32,
    cache: Option<&CachePrewarmConfig>,
) -> Arc<dyn Provider> {
    // Per-response cap is the load-bearing cost guard. The session-level
    // budget check only fires after a `Usage` event arrives — by that point
    // the model has already produced (and we've paid for) the response.
    // `max_tokens` in the request prevents runaway single responses.
    let mut params = serde_json::Map::new();
    params.insert(
        "max_tokens".into(),
        serde_json::Value::Number(max_tokens_per_response.into()),
    );
    if model == "openrouter/free" {
        let fallback: Vec<&str> = FREE_FALLBACK.iter().skip(1).copied().collect();
        params.insert("models".into(), json!(fallback));
        params.insert("route".into(), json!("fallback"));
    }
    if let Some(cache) = cache {
        params.insert("session_id".into(), json!(cache.session_id));
        let mut cache_control = serde_json::Map::new();
        cache_control.insert("type".into(), json!("ephemeral"));
        if cache.ttl_1h {
            cache_control.insert("ttl".into(), json!("1h"));
        }
        params.insert(
            "cache_control".into(),
            serde_json::Value::Object(cache_control),
        );
        // A cache key is not useful if OpenRouter silently falls back to a
        // different upstream provider mid-sweep.
        params.insert("provider".into(), json!({ "allow_fallbacks": false }));
    }
    let provider = create_openrouter_provider(ProviderConfig {
        api_key,
        model: model.to_string(),
        base_url: None,
        request_params: Some(serde_json::Value::Object(params)),
        app_attribution: None,
        retry: Some(RetryConfig {
            max_retries: 3,
            base_delay_ms: 1_000,
            // Batch runs can wait through a free-router minute window; normal
            // REPL/headless clients retain the shorter provider default.
            max_delay_ms: 90_000,
        }),
    });
    if is_free_model(model) {
        Arc::new(PacedProvider {
            inner: provider,
            interval: FREE_MODEL_REQUEST_INTERVAL,
            next_request: Arc::new(Mutex::new(Instant::now())),
        })
    } else {
        provider
    }
}

async fn prewarm_prompt(
    prompt: &Prompt,
    model: &str,
    api_key: &str,
    cache: &CachePrewarmConfig,
) -> Result<PrewarmResult> {
    let stable_prefix = prompt.body.trim();
    if stable_prefix.is_empty() {
        bail!(
            "--cache-prewarm requires a non-empty prompt body ({})",
            prompt.id
        );
    }

    let provider = make_provider(model, api_key.to_string(), 1, Some(cache));
    let start = Instant::now();
    let response = provider
        .send(
            &[
                Message::System(SystemMessage {
                    content: stable_prefix.to_string(),
                }),
                Message::User(UserMessage {
                    content: "Acknowledge these instructions briefly.".to_string(),
                }),
            ],
            None,
            &json!({}),
        )
        .await
        .with_context(|| format!("prewarming prompt {}", prompt.id))?;
    let usage = response.usage.unwrap_or_default();
    let details = usage.prompt_tokens_details.as_ref();
    Ok(PrewarmResult {
        prompt_id: prompt.id.clone(),
        duration_ms: start.elapsed().as_millis(),
        routed_model: response.model,
        tokens_in: usage.prompt_tokens,
        tokens_out: usage.completion_tokens,
        cached_tokens: details.and_then(|d| d.cached_tokens).unwrap_or(0),
        cache_write_tokens: details.and_then(|d| d.cache_write_tokens).unwrap_or(0),
    })
}

#[derive(Debug, Clone)]
struct RunOneOptions {
    max_turns: u32,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    task_timeout_secs: u64,
    record_all_text: bool,
    cache: Option<CachePrewarmConfig>,
}

async fn run_one(
    task: &Task,
    prompt: &Prompt,
    model: &str,
    api_key: &str,
    options: &RunOneOptions,
) -> TaskResult {
    let start = Instant::now();
    let tempdir = tempfile::tempdir().expect("tempdir");
    let workspace = tempdir.path();
    if let Err(e) = copy_dir_recursive(&task.dir.join("before"), workspace) {
        return error_result(task, prompt, model, format!("copy before/: {e}"), start);
    }

    let cache_mode = options.cache.is_some();
    let mut messages = compose_messages(prompt, workspace, cache_mode);
    let rendered_system_prompt_chars = messages
        .iter()
        .filter_map(|message| match message {
            Message::System(system) => Some(system.content.chars().count()),
            _ => None,
        })
        .sum();
    messages.push(Message::User(UserMessage {
        content: task.spec.prompt.clone(),
    }));

    let tool_names = task.spec.tools.clone().unwrap_or_else(|| {
        vec![
            "read_file".into(),
            "write_file".into(),
            "edit_file".into(),
            "glob".into(),
            "grep".into(),
        ]
    });
    let registry = match build_workspace_registry(&tool_names, workspace) {
        Ok(r) => r,
        Err(e) => return error_result(task, prompt, model, e.to_string(), start),
    };
    let input_contract = match eval_input_contract(&messages, &registry) {
        Ok(contract) => contract,
        Err(e) => return error_result(task, prompt, model, e.to_string(), start),
    };

    let provider = make_provider(
        model,
        api_key.to_string(),
        options.max_tokens_per_response,
        options.cache.as_ref(),
    );
    let effective_max_turns = task_max_turns(task.spec.max_turns, options.max_turns);
    // task.toml budget wins when set; else CLI default.
    let effective_max_tokens = task
        .spec
        .budget_tokens
        .unwrap_or(options.max_tokens_per_task);
    let effective_timeout_secs = task
        .spec
        .task_timeout_secs
        .unwrap_or(options.task_timeout_secs)
        .max(1);

    let mut tool_calls = 0u32;
    let mut turns = 0u32;
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut cached_tokens = 0u64;
    let mut cache_write_tokens = 0u64;
    let mut usd = 0.0f64;
    let mut routed_models: Vec<RoutedModelObservation> = Vec::new();
    let mut upstream_providers: Vec<UpstreamProviderObservation> = Vec::new();
    let mut generation_ids: Vec<String> = Vec::new();
    let mut provider_telemetry: Vec<ProviderTelemetry> = Vec::new();
    let mut debug_errors: Vec<ProviderDebugError> = Vec::new();
    let mut call_telemetry: Vec<CallTelemetry> = Vec::new();
    let mut current_call: Option<CallTelemetry> = None;
    let mut pending_routed_model: Option<String> = None;
    let mut pending_upstream_provider: Option<String> = None;
    let mut error: Option<String> = None;
    let mut error_cause: Option<FailureCause> = None;
    let mut limit: Option<LimitReason> = None;
    let mut tool_sequence: Vec<String> = Vec::new();
    let mut tool_failures: Vec<ToolFailureTrace> = Vec::new();
    let mut tool_trace_truncated = false;
    let mut finish_reasons: Vec<String> = Vec::new();
    let mut assistant_messages: Vec<AssistantTrace> = Vec::new();
    let mut budget_exceeded = false;

    let prev_cwd = std::env::current_dir().ok();
    if std::env::set_current_dir(workspace).is_err() {
        return error_result(task, prompt, model, "set_current_dir failed".into(), start);
    }
    // Commands such as `cargo test` are legitimate agent behavior but their
    // build output is not part of the requested workspace edit. Keep it in a
    // separate temporary directory so exact-directory scoring remains about
    // source changes rather than tool byproducts.
    let cargo_target_dir = tempfile::tempdir().expect("cargo target tempdir");
    let previous_cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR");
    std::env::set_var("CARGO_TARGET_DIR", cargo_target_dir.path());

    let opts = AgentLoopOptions {
        max_iterations: Some(effective_max_turns),
        ..AgentLoopOptions::default()
    };
    {
        let stream = run_agent_loop(provider, registry, &mut messages, opts);
        futures::pin_mut!(stream);
        loop {
            let remaining =
                Duration::from_secs(effective_timeout_secs).saturating_sub(start.elapsed());
            let event = match timeout(remaining, stream.next()).await {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    error = Some(format!("Task timed out after {effective_timeout_secs}s"));
                    error_cause = Some(FailureCause::Timeout);
                    break;
                }
            };

            match event {
                AgentEvent::ToolStart { name, .. } => {
                    tool_calls += 1;
                    println!("      -> {name}");
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    tool_sequence.push(name);
                }
                AgentEvent::ToolEnd { name, result, .. } => {
                    if is_tool_failure(&result) {
                        let detail = truncate_trace_detail(&result);
                        let used_bytes: usize = tool_failures
                            .iter()
                            .map(|failure| failure.detail.len())
                            .sum();
                        if tool_failures.len() < TRACE_MAX_FAILURES
                            && used_bytes + detail.len() <= TRACE_MAX_FAILURE_BYTES
                        {
                            tool_failures.push(ToolFailureTrace { name, detail });
                        } else {
                            tool_trace_truncated = true;
                        }
                    }
                }
                AgentEvent::AssistantMessage {
                    message,
                    finish_reason,
                } => {
                    let call =
                        current_call.get_or_insert_with(|| CallTelemetry::new(task, prompt, model));
                    turns += 1;
                    call.assistant_turn = Some(turns);
                    call.finish_reason = finish_reason.clone();
                    if let Some(model) = pending_routed_model.take() {
                        routed_models.push(RoutedModelObservation {
                            assistant_turn: turns,
                            model,
                        });
                    }
                    if let Some(provider) = pending_upstream_provider.take() {
                        upstream_providers.push(UpstreamProviderObservation {
                            assistant_turn: turns,
                            provider,
                        });
                    }
                    if let Some(reason) = &finish_reason {
                        finish_reasons.push(reason.clone());
                    }
                    if let Some(text) = message.content {
                        if !text.is_empty() {
                            assistant_messages.push(AssistantTrace {
                                turn: turns,
                                finish_reason,
                                text,
                            });
                        }
                    }
                }
                AgentEvent::Usage {
                    usage,
                    generation_id,
                } => {
                    let call =
                        current_call.get_or_insert_with(|| CallTelemetry::new(task, prompt, model));
                    if let Some(generation_id) = generation_id {
                        generation_ids.push(generation_id.clone());
                        call.generation_id = Some(generation_id);
                    }
                    call.prompt_tokens = Some(usage.prompt_tokens);
                    call.completion_tokens = Some(usage.completion_tokens);
                    call.total_tokens = Some(usage.total_tokens);
                    call.cached_tokens = usage
                        .prompt_tokens_details
                        .as_ref()
                        .and_then(|d| d.cached_tokens);
                    call.cache_write_tokens = usage
                        .prompt_tokens_details
                        .as_ref()
                        .and_then(|d| d.cache_write_tokens);
                    call.reasoning_tokens = usage
                        .completion_tokens_details
                        .as_ref()
                        .and_then(|d| d.reasoning_tokens);
                    call.usd = usage.cost;
                    tokens_in += usage.prompt_tokens;
                    tokens_out += usage.completion_tokens;
                    cached_tokens += usage
                        .prompt_tokens_details
                        .as_ref()
                        .and_then(|details| details.cached_tokens)
                        .unwrap_or(0);
                    cache_write_tokens += usage
                        .prompt_tokens_details
                        .as_ref()
                        .and_then(|details| details.cache_write_tokens)
                        .unwrap_or(0);
                    usd += usage.cost.unwrap_or(0.0);
                    if tokens_in + tokens_out > effective_max_tokens {
                        // Cost-control kill switch, NOT a correctness failure.
                        // Break so we don't burn more tokens, but still attempt
                        // the dir diff below — the agent may have done the work
                        // and just emitted verbose tail-text after.
                        budget_exceeded = true;
                        limit = Some(LimitReason::TokenBudget);
                        break;
                    }
                }
                AgentEvent::RoutedModel { model } => {
                    if let Some(call) = current_call.take() {
                        call_telemetry.push(call);
                    }
                    let call = current_call
                        .get_or_insert_with(|| CallTelemetry::new(task, prompt, &model));
                    call.routed_model = Some(model.clone());
                    pending_routed_model = Some(model);
                }
                AgentEvent::UpstreamProvider { provider } => {
                    if upstream_providers
                        .last()
                        .is_some_and(|previous| previous.provider != provider)
                    {
                        println!(
                            "      upstream provider switch: {} -> {provider}",
                            upstream_providers.last().expect("checked above").provider
                        );
                    }
                    let call =
                        current_call.get_or_insert_with(|| CallTelemetry::new(task, prompt, model));
                    call.provider = Some(provider.clone());
                    pending_upstream_provider = Some(provider);
                }
                AgentEvent::Error { message } => {
                    if let Some(call) = current_call.take() {
                        call_telemetry.push(call);
                    }
                    if message.contains("; retrying once") {
                        continue;
                    }
                    if message.starts_with("Max iterations (") {
                        limit = Some(LimitReason::MaxTurns);
                    } else {
                        error_cause = Some(classify_runtime_error(&message));
                        error = Some(message);
                    }
                    break;
                }
                AgentEvent::ProviderError {
                    message,
                    telemetry,
                    debug_detail,
                } => {
                    if let Some(call) = current_call.take() {
                        call_telemetry.push(call);
                    }
                    let mut failed_call = CallTelemetry::new(task, prompt, model);
                    failed_call.provider = telemetry.provider.clone();
                    failed_call.generation_id = telemetry.generation_id.clone();
                    failed_call.provider_telemetry = Some(telemetry.clone());
                    call_telemetry.push(failed_call);
                    if let Some(detail) = debug_detail {
                        debug_errors.push(ProviderDebugError {
                            timestamp: Utc::now().to_rfc3339(),
                            detail,
                            telemetry: telemetry.clone(),
                        });
                    }
                    provider_telemetry.push(telemetry);
                    error_cause = Some(FailureCause::ProviderApi);
                    error = Some(message);
                    break;
                }
                AgentEvent::PermissionDenied { reason, .. } => {
                    error = Some(reason);
                    error_cause = Some(FailureCause::Permission);
                    break;
                }
                AgentEvent::LoopDetected { .. } => {
                    limit = Some(LimitReason::DoomLoop);
                    break;
                }
                _ => {}
            }
        }
    }
    if let Some(call) = current_call.take() {
        call_telemetry.push(call);
    }
    if let Some(prev) = prev_cwd {
        let _ = std::env::set_current_dir(prev);
    }
    match previous_cargo_target_dir {
        Some(path) => std::env::set_var("CARGO_TARGET_DIR", path),
        None => std::env::remove_var("CARGO_TARGET_DIR"),
    }

    let diff = task
        .spec
        .score
        .outcome
        .expected_dir
        .as_deref()
        .map(|expected_dir| diff_dirs(workspace, &task.dir.join(expected_dir)))
        .unwrap_or_default();
    let exact_passed = task.spec.score.outcome.expected_dir.is_some() && diff.is_empty();
    let mut semantic_verification = None;
    let mut protected_diffs = Vec::new();
    if error.is_none() && limit.is_none() && !exact_passed {
        if let Some(spec) = &task.spec.score.semantic_verification {
            match protected_path_diffs(&task.dir.join("before"), workspace, &spec.protected_globs) {
                Ok(diffs) => protected_diffs = diffs,
                Err(verification_error) => {
                    error = Some(verification_error.to_string());
                    error_cause = Some(FailureCause::HarnessInternal);
                }
            }
            if error.is_none() && protected_diffs.is_empty() {
                match run_semantic_verification(task, workspace, spec).await {
                    Ok(result) => semantic_verification = Some(result),
                    Err(verification_error) => {
                        error = Some(verification_error.to_string());
                        error_cause = Some(FailureCause::HarnessInternal);
                    }
                }
            }
        }
    }
    let semantic_passed = semantic_verification
        .as_ref()
        .is_some_and(|result| result.passed);
    let passed = error.is_none() && (exact_passed || semantic_passed);
    let failure_cause = if passed {
        None
    } else if let Some(limit) = limit {
        Some(limit.into())
    } else if let Some(cause) = error_cause {
        Some(cause)
    } else if !protected_diffs.is_empty() {
        Some(FailureCause::ProtectedFileChanged)
    } else if semantic_verification.is_some() {
        Some(FailureCause::SemanticVerificationFailed)
    } else {
        Some(classify_workspace_failure(
            &diff_dirs(workspace, &task.dir.join("before")),
            &diff,
        ))
    };
    if passed && !options.record_all_text {
        assistant_messages.clear();
    }

    let (eff_min, eff_max) = match &task.spec.score.efficiency {
        Some(e) => (e.min_tool_calls, e.max_tool_calls),
        None => (None, None),
    };
    let tool_calls_in_range = eff_min.map(|m| tool_calls >= m).unwrap_or(true)
        && eff_max.map(|m| tool_calls <= m).unwrap_or(true);
    if routed_models
        .iter()
        .all(|observation| observation.model == model)
    {
        routed_models.clear();
    }
    let trace = Some(build_compact_trace(CompactTraceInput {
        assistant_turns: turns,
        tool_sequence: &tool_sequence,
        tool_failures: &tool_failures,
        finish_reasons: &finish_reasons,
        terminal_cause: failure_cause,
        routed_models: &routed_models,
        upstream_providers: &upstream_providers,
        tool_trace_truncated,
    }));

    TaskResult {
        task_id: task.spec.id.clone(),
        prompt_id: prompt.id.clone(),
        tags: task.spec.tags.clone(),
        model: model.to_string(),
        heddle_commit: heddle_git_info().commit,
        evals_version: "0.1.0".into(),
        timestamp: Utc::now().to_rfc3339(),
        duration_ms: start.elapsed().as_millis(),
        rendered_system_prompt_chars,
        input_contract: Some(input_contract),
        routed_models,
        upstream_providers,
        generation_ids,
        provider_telemetry,
        debug_errors,
        call_telemetry,
        run_index: 0,
        retry_attempts: Vec::new(),
        tool_sequence,
        finish_reasons,
        assistant_messages,
        trace,
        transcript: messages,
        scores: Scores {
            outcome: OutcomeScore {
                passed,
                exact_passed,
                diff_files: diff,
                protected_diffs,
                semantic_verification,
            },
            efficiency: EfficiencyScore {
                tool_calls,
                turns,
                tool_calls_in_range,
                max_tool_calls: eff_max,
                exceeded_max_tool_calls: eff_max.map(|max| tool_calls > max),
                tokens_in_budget: !budget_exceeded,
            },
            cost: CostScore {
                tokens_in,
                tokens_out,
                cached_tokens,
                cache_write_tokens,
                usd,
            },
            limit,
            failure_cause,
            error,
        },
    }
}

fn task_max_turns(task_limit: Option<u32>, fallback: u32) -> u32 {
    task_limit.unwrap_or(fallback)
}

fn error_result(
    task: &Task,
    prompt: &Prompt,
    model: &str,
    err: String,
    start: Instant,
) -> TaskResult {
    TaskResult {
        task_id: task.spec.id.clone(),
        prompt_id: prompt.id.clone(),
        tags: task.spec.tags.clone(),
        model: model.to_string(),
        heddle_commit: heddle_git_info().commit,
        evals_version: "0.1.0".into(),
        timestamp: Utc::now().to_rfc3339(),
        duration_ms: start.elapsed().as_millis(),
        rendered_system_prompt_chars: 0,
        input_contract: None,
        routed_models: Vec::new(),
        upstream_providers: Vec::new(),
        generation_ids: Vec::new(),
        provider_telemetry: Vec::new(),
        debug_errors: Vec::new(),
        call_telemetry: Vec::new(),
        run_index: 0,
        retry_attempts: Vec::new(),
        tool_sequence: Vec::new(),
        finish_reasons: Vec::new(),
        assistant_messages: Vec::new(),
        trace: Some(CompactTrace {
            assistant_turns: 0,
            tool_sequence: Vec::new(),
            tool_counts: BTreeMap::new(),
            tool_failures: Vec::new(),
            finish_reasons: Vec::new(),
            terminal_cause: Some(FailureCause::HarnessInternal),
            routed_model_change: None,
            upstream_provider_change: None,
            truncated: false,
        }),
        transcript: Vec::new(),
        scores: Scores {
            outcome: OutcomeScore {
                passed: false,
                exact_passed: false,
                diff_files: Vec::new(),
                protected_diffs: Vec::new(),
                semantic_verification: None,
            },
            efficiency: EfficiencyScore {
                tool_calls: 0,
                turns: 0,
                tool_calls_in_range: false,
                max_tool_calls: None,
                exceeded_max_tool_calls: None,
                tokens_in_budget: true,
            },
            cost: CostScore {
                tokens_in: 0,
                tokens_out: 0,
                cached_tokens: 0,
                cache_write_tokens: 0,
                usd: 0.0,
            },
            limit: None,
            failure_cause: Some(FailureCause::HarnessInternal),
            error: Some(err),
        },
    }
}

async fn run_with_provider_retry(
    task: &Task,
    prompt: &Prompt,
    model: &str,
    api_key: &str,
    options: &RunOneOptions,
) -> (TaskResult, Option<TaskResult>) {
    let first = run_one(task, prompt, model, api_key, options).await;
    let Some(policy) = retry_policy(&first) else {
        return (first, None);
    };

    let cause = failure_cause(&first)
        .expect("retryable_provider_error requires a structured failure cause");
    println!(
        "      retrying from a fresh workspace and conversation after {} ({})",
        cause.label(),
        policy.reason
    );
    if !policy.delay.is_zero() {
        sleep(policy.delay).await;
    }
    let mut retry = run_one(task, prompt, model, api_key, options).await;
    if let Some(attempt) = RetryAttempt::from_result(&first, 1) {
        retry.retry_attempts.push(RetryAttempt {
            retry_reason: Some(policy.reason),
            retry_delay_ms: (!policy.delay.is_zero()).then_some(policy.delay.as_millis() as u64),
            ..attempt
        });
    }
    (retry, Some(first))
}

fn classify_runtime_error(message: &str) -> FailureCause {
    let message = message.to_ascii_lowercase();
    if message.contains("api error") || message.contains("provider") {
        FailureCause::ProviderApi
    } else if message.contains("connection")
        || message.contains("network")
        || message.contains("dns")
        || message.contains("socket")
        || message.contains("transport")
    {
        FailureCause::Transport
    } else if message.contains("permission") || message.contains("denied") {
        FailureCause::Permission
    } else if message.contains("tool") {
        FailureCause::Tool
    } else {
        FailureCause::HarnessInternal
    }
}

fn classify_workspace_failure(
    changes_from_before: &[DirDiffEntry],
    expected_diff: &[DirDiffEntry],
) -> FailureCause {
    if changes_from_before.is_empty() {
        return FailureCause::NoOp;
    }
    let kinds: std::collections::BTreeSet<&str> = expected_diff
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect();
    match kinds.len() {
        1 if kinds.contains("differs") => FailureCause::WrongChangedFile,
        1 if kinds.contains("missing") => FailureCause::MissingExpectedChange,
        1 if kinds.contains("unexpected") => FailureCause::UnexpectedExtraFile,
        _ => FailureCause::WrongDiff,
    }
}

fn is_tool_failure(result: &str) -> bool {
    result.trim_start().starts_with("Error:")
}

fn truncate_trace_detail(detail: &str) -> String {
    if detail.len() <= TRACE_MAX_FAILURE_BYTES {
        return detail.to_string();
    }
    let mut end = TRACE_MAX_FAILURE_BYTES;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &detail[..end])
}

struct CompactTraceInput<'a> {
    assistant_turns: u32,
    tool_sequence: &'a [String],
    tool_failures: &'a [ToolFailureTrace],
    finish_reasons: &'a [String],
    terminal_cause: Option<FailureCause>,
    routed_models: &'a [RoutedModelObservation],
    upstream_providers: &'a [UpstreamProviderObservation],
    tool_trace_truncated: bool,
}

fn build_compact_trace(input: CompactTraceInput<'_>) -> CompactTrace {
    let CompactTraceInput {
        assistant_turns,
        tool_sequence,
        tool_failures,
        finish_reasons,
        terminal_cause,
        routed_models,
        upstream_providers,
        tool_trace_truncated,
    } = input;
    let mut tool_counts = BTreeMap::new();
    for name in tool_sequence {
        *tool_counts.entry(name.clone()).or_default() += 1;
    }
    let truncated = tool_trace_truncated || tool_sequence.len() > TRACE_MAX_TOOL_EVENTS;
    let routed_model_change = (routed_models.len() > 1).then(|| {
        routed_models
            .iter()
            .map(|observation| observation.model.as_str())
            .collect::<Vec<_>>()
            .join(" -> ")
    });
    let distinct_providers: Vec<&str> = upstream_providers
        .iter()
        .map(|observation| observation.provider.as_str())
        .fold(Vec::new(), |mut providers, provider| {
            if providers.last().copied() != Some(provider) {
                providers.push(provider);
            }
            providers
        });
    let upstream_provider_change =
        (distinct_providers.len() > 1).then(|| distinct_providers.join(" -> "));
    CompactTrace {
        assistant_turns,
        tool_sequence: tool_sequence
            .iter()
            .take(TRACE_MAX_TOOL_EVENTS)
            .cloned()
            .collect(),
        tool_counts,
        tool_failures: tool_failures.to_vec(),
        finish_reasons: finish_reasons.to_vec(),
        terminal_cause,
        routed_model_change,
        upstream_provider_change,
        truncated,
    }
}

#[derive(Debug, Clone)]
struct GitInfo {
    commit: String,
    dirty: Option<bool>,
}

fn git_info(dir: &Path) -> GitInfo {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--short", "HEAD"])
        .output();
    let commit = match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown".into(),
    };
    let dirty = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| !output.stdout.is_empty());
    GitInfo { commit, dirty }
}

fn heddle_git_info() -> GitInfo {
    git_info(Path::new(env!("CARGO_MANIFEST_DIR")))
}

// ─── Output ──────────────────────────────────────────────────────────────

fn legacy_limit_reason(result: &TaskResult) -> Option<LimitReason> {
    result
        .scores
        .error
        .as_deref()
        .filter(|message| message.starts_with("Max iterations ("))
        .map(|_| LimitReason::MaxTurns)
}

fn limit_reason(result: &TaskResult) -> Option<LimitReason> {
    result.scores.limit.or_else(|| legacy_limit_reason(result))
}

fn result_status(result: &TaskResult) -> ResultStatus {
    if result.scores.outcome.passed {
        ResultStatus::Pass
    } else if limit_reason(result).is_some() {
        ResultStatus::Limit
    } else if result.scores.error.is_some() {
        ResultStatus::Error
    } else {
        ResultStatus::Fail
    }
}

fn failure_cause(result: &TaskResult) -> Option<FailureCause> {
    result.scores.failure_cause
}

fn failure_cause_summary<'a>(results: impl IntoIterator<Item = &'a TaskResult>) -> Option<String> {
    let mut counts: BTreeMap<FailureCause, usize> = BTreeMap::new();
    for result in results {
        if let Some(cause) = failure_cause(result) {
            *counts.entry(cause).or_default() += 1;
        }
        for retry in &result.retry_attempts {
            *counts.entry(retry.cause).or_default() += 1;
        }
    }
    (!counts.is_empty()).then(|| {
        format!(
            "failure causes: {}\n",
            counts
                .into_iter()
                .map(|(cause, count)| format!("{}={count}", cause.label()))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn outcome_label(result: &TaskResult) -> String {
    let outcome = match result_status(result) {
        ResultStatus::Pass if !result.scores.outcome.exact_passed => "SEMANTIC PASS",
        ResultStatus::Pass if result.scores.efficiency.tokens_in_budget => "PASS",
        ResultStatus::Pass => "PASS*",
        ResultStatus::Fail => "FAIL",
        ResultStatus::Limit => "LIMIT",
        ResultStatus::Error => "ERROR",
    };
    if result.retry_attempts.is_empty() {
        outcome.into()
    } else {
        format!("{outcome} ({} RETRY)", result.retry_attempts.len())
    }
}

fn retry_count(result: &TaskResult) -> usize {
    result.retry_attempts.len()
}

fn retry_error_attempt_count(result: &TaskResult) -> usize {
    result.retry_attempts.len()
        + usize::from(result_status(result) == ResultStatus::Error && retry_count(result) > 0)
}

struct RetryPolicy {
    reason: String,
    delay: Duration,
}

fn retry_policy(result: &TaskResult) -> Option<RetryPolicy> {
    result.scores.error.as_ref()?;
    if failure_cause(result) == Some(FailureCause::Transport) {
        return Some(RetryPolicy {
            reason: "transport_failure".into(),
            delay: Duration::ZERO,
        });
    }
    if failure_cause(result) != Some(FailureCause::ProviderApi) {
        return None;
    }
    let telemetry = result.provider_telemetry.last()?;
    let error_type = telemetry
        .error_type
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let retryable_type = [
        "rate_limit",
        "overload",
        "temporarily_unavailable",
        "server_error",
    ]
    .iter()
    .any(|kind| error_type.contains(kind));
    let retryable_status = matches!(telemetry.status, Some(408 | 429 | 500 | 502 | 503 | 504));
    if !retryable_type && !retryable_status {
        return None;
    }
    let delay = Duration::from_millis(telemetry.retry_after_ms.unwrap_or(0).min(10_000));
    Some(RetryPolicy {
        reason: if retryable_type {
            format!("provider_{error_type}")
        } else {
            format!("http_{}", telemetry.status.unwrap_or_default())
        },
        delay,
    })
}

fn attempt_cost(result: &TaskResult) -> CostScore {
    let mut cost = result.scores.cost.clone();
    for retry in &result.retry_attempts {
        cost.tokens_in += retry.cost.tokens_in;
        cost.tokens_out += retry.cost.tokens_out;
        cost.cached_tokens += retry.cost.cached_tokens;
        cost.cache_write_tokens += retry.cost.cache_write_tokens;
        cost.usd += retry.cost.usd;
    }
    cost
}

fn exceeded_max_tool_call_guidance(result: &TaskResult) -> Option<bool> {
    result
        .scores
        .efficiency
        .exceeded_max_tool_calls
        .or_else(|| {
            result
                .scores
                .efficiency
                .max_tool_calls
                .map(|max| result.scores.efficiency.tool_calls > max)
        })
}

fn requested_model_line<'a>(results: impl IntoIterator<Item = &'a TaskResult>) -> Option<String> {
    let mut models: Vec<&str> = results
        .into_iter()
        .map(|result| result.model.as_str())
        .filter(|model| !model.is_empty())
        .collect();
    models.sort_unstable();
    models.dedup();
    match models.as_slice() {
        [] => None,
        [model] => Some(format!("model: {model}\n")),
        models => Some(format!("models: {}\n", models.join(", "))),
    }
}

fn format_summary(results: &[TaskResult]) -> String {
    let mut out = String::new();
    if results.is_empty() {
        return out;
    }
    let has_routed_models = results.iter().any(|r| !r.routed_models.is_empty());
    let has_upstream_providers = results.iter().any(|r| !r.upstream_providers.is_empty());
    let mut header = vec!["task", "prompt"];
    if has_routed_models {
        header.push("routed");
    }
    if has_upstream_providers {
        header.push("provider");
    }
    header.extend([
        "outcome",
        "tools",
        "turns",
        "tokens",
        "cache r/w",
        "usd",
        "err",
    ]);
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(results.len() + 1);
    rows.push(header.into_iter().map(String::from).collect());
    for r in results {
        let err = r.scores.error.as_deref().unwrap_or("");
        let err: String = err.chars().take(50).collect();
        let tool_calls = r
            .scores
            .efficiency
            .max_tool_calls
            .filter(|_| exceeded_max_tool_call_guidance(r) == Some(true))
            .map(|max| format!("{} (max {max})", r.scores.efficiency.tool_calls))
            .unwrap_or_else(|| r.scores.efficiency.tool_calls.to_string());
        let mut row = vec![r.task_id.clone(), r.prompt_id.clone()];
        if has_routed_models {
            row.push(routed_model_summary(r));
        }
        if has_upstream_providers {
            row.push(upstream_provider_summary(r));
        }
        row.extend([
            outcome_label(r),
            tool_calls,
            r.scores.efficiency.turns.to_string(),
            format!("{}/{}", r.scores.cost.tokens_in, r.scores.cost.tokens_out),
            format!(
                "{}/{}",
                r.scores.cost.cached_tokens, r.scores.cost.cache_write_tokens
            ),
            format!("{:.6}", r.scores.cost.usd),
            err,
        ]);
        rows.push(row);
    }
    let mut widths = vec![0usize; rows[0].len()];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |row: &[String]| -> String {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<width$}", width = widths[i]))
            .collect();
        format!("| {} |", cells.join(" | "))
    };
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    out.push('\n');
    out.push_str(&render(&rows[0]));
    out.push('\n');
    out.push_str(&format!("|-{}-|", sep.join("-|-")));
    out.push('\n');
    for row in &rows[1..] {
        out.push_str(&render(row));
        out.push('\n');
    }
    out.push('\n');
    if let Some(model_line) = requested_model_line(results) {
        out.push_str(&model_line);
    }
    let pass = results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Pass)
        .count();
    let over_budget = results
        .iter()
        .filter(|r| r.scores.outcome.passed && !r.scores.efficiency.tokens_in_budget)
        .count();
    let limited = results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Limit)
        .count();
    let errors = results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Error)
        .count();
    let fail = results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Fail)
        .count();
    let retries: usize = results.iter().map(retry_count).sum();
    let retry_errors: usize = results.iter().map(retry_error_attempt_count).sum();
    out.push_str(&format!(
        "{pass} passed ({over_budget} over budget), {fail} failed, {limited} limited, {errors} errored of {} total{}\n",
        results.len(),
        if retries > 0 {
            format!("; {retries} retr{} ({retry_errors} errored attempt{})",
                if retries == 1 { "y" } else { "ies" },
                if retry_errors == 1 { "" } else { "s" })
        } else {
            String::new()
        },
    ));
    if over_budget > 0 {
        out.push_str(
            "(`pass*` = correct outcome but token budget exceeded mid-run; not a failure)\n",
        );
    }
    let over_tool_call_guidance = results
        .iter()
        .filter(|r| exceeded_max_tool_call_guidance(r) == Some(true))
        .count();
    if over_tool_call_guidance > 0 {
        out.push_str(&format!(
            "{over_tool_call_guidance} exceeded maximum tool-call guidance (not a failure)\n"
        ));
    }
    if let Some(causes) = failure_cause_summary(results.iter()) {
        out.push_str(&causes);
    }
    let cached_total = results
        .iter()
        .map(|r| attempt_cost(r).cached_tokens)
        .sum::<u64>();
    let cache_write_total = results
        .iter()
        .map(|r| attempt_cost(r).cache_write_tokens)
        .sum::<u64>();
    out.push_str(&format!(
        "cache tokens: {cached_total} read, {cache_write_total} written\n"
    ));
    let totals = run_totals(results);
    out.push_str(&format!(
        "totals: {} prompt + {} completion = {} tokens, ${:.6}\n",
        totals.prompt_tokens, totals.completion_tokens, totals.total_tokens, totals.usd
    ));
    out.push('\n');
    out
}

fn run_totals(results: &[TaskResult]) -> RunTotals {
    run_totals_from_refs(results.iter())
}

fn run_totals_from_refs<'a>(results: impl IntoIterator<Item = &'a TaskResult>) -> RunTotals {
    let costs: Vec<CostScore> = results.into_iter().map(attempt_cost).collect();
    let prompt_tokens = costs.iter().map(|cost| cost.tokens_in).sum();
    let completion_tokens = costs.iter().map(|cost| cost.tokens_out).sum();
    RunTotals {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        usd: costs.iter().map(|cost| cost.usd).sum(),
    }
}

fn reported_results(results: &[TaskResult]) -> Vec<&TaskResult> {
    if results.iter().any(|result| result.run_index > 0) {
        results
            .iter()
            .filter(|result| result.run_index > 0)
            .collect()
    } else {
        results.iter().collect()
    }
}

fn routed_model_summary(r: &TaskResult) -> String {
    let mut models: Vec<&str> = Vec::new();
    for observation in &r.routed_models {
        if models.last().copied() != Some(observation.model.as_str()) {
            models.push(&observation.model);
        }
    }
    match models.as_slice() {
        [] => "-".into(),
        [model] => (*model).to_string(),
        models => format!(
            "mixed ({} models; final {})",
            models.len(),
            models.last().expect("non-empty routed model list")
        ),
    }
}

fn upstream_provider_summary(r: &TaskResult) -> String {
    let providers = r
        .upstream_providers
        .iter()
        .fold(Vec::new(), |mut values, observation| {
            if values.last().copied() != Some(observation.provider.as_str()) {
                values.push(observation.provider.as_str());
            }
            values
        });
    match providers.as_slice() {
        [] => "-".into(),
        [provider] => (*provider).to_string(),
        providers => format!(
            "switched ({}; final {})",
            providers.join(" -> "),
            providers.last().unwrap()
        ),
    }
}

/// Aggregate per (task, prompt) over multiple runs. Reports pass rate
/// (X/N), mean tokens (in/out), mean tool_calls and turns, and a stddev
/// flag when tokens vary by >25% from mean (indicates noise).
fn format_aggregated_summary(results: &[TaskResult], runs: u32) -> String {
    use std::collections::BTreeMap;
    if results.is_empty() {
        return String::new();
    }
    // In repeated runs, smoke cases are one-time preflight checks (run_index
    // zero), not observations in the quality matrix. Keep them visible only
    // when the matrix never started, such as after a smoke failure.
    let reported_results = reported_results(results);
    // Group by (task_id, prompt_id).
    let mut groups: BTreeMap<(String, String), Vec<&TaskResult>> = BTreeMap::new();
    for r in &reported_results {
        groups
            .entry((r.task_id.clone(), r.prompt_id.clone()))
            .or_default()
            .push(*r);
    }

    let header = [
        "task",
        "prompt",
        "outcome",
        "tools (avg)",
        "turns (avg)",
        "tokens in (avg±std)",
        "tokens out (avg)",
        "cache read (avg)",
        "cache write (avg)",
        "usd (avg)",
    ];
    let mut rows: Vec<[String; 10]> = Vec::with_capacity(groups.len() + 1);
    rows.push(header.map(String::from));

    for ((task_id, prompt_id), runs_of) in &groups {
        let n = runs_of.len() as f64;
        let passed = runs_of.iter().filter(|r| r.scores.outcome.passed).count();
        let limited = runs_of
            .iter()
            .filter(|r| result_status(r) == ResultStatus::Limit)
            .count();
        let errors = runs_of
            .iter()
            .filter(|r| result_status(r) == ResultStatus::Error)
            .count();
        let failed = runs_of
            .iter()
            .filter(|r| result_status(r) == ResultStatus::Fail)
            .count();
        let retries: usize = runs_of.iter().map(|r| retry_count(r)).sum();
        let mut pass_rate = format!("{passed}/{} pass", runs_of.len());
        if retries > 0 {
            pass_rate.push_str(&format!("; {retries} RETRY"));
        }
        if failed > 0 {
            pass_rate.push_str(&format!("; {failed} FAIL"));
        }
        if limited > 0 {
            pass_rate.push_str(&format!("; {limited} LIMIT"));
        }
        if errors > 0 {
            pass_rate.push_str(&format!("; {errors} ERROR"));
        }
        let mean_tools = runs_of
            .iter()
            .map(|r| r.scores.efficiency.tool_calls as f64)
            .sum::<f64>()
            / n;
        let mean_turns = runs_of
            .iter()
            .map(|r| r.scores.efficiency.turns as f64)
            .sum::<f64>()
            / n;
        let toks_in: Vec<f64> = runs_of
            .iter()
            .map(|r| r.scores.cost.tokens_in as f64)
            .collect();
        let mean_in = toks_in.iter().sum::<f64>() / n;
        let var_in = toks_in.iter().map(|t| (t - mean_in).powi(2)).sum::<f64>() / n;
        let std_in = var_in.sqrt();
        let mean_out = runs_of
            .iter()
            .map(|r| r.scores.cost.tokens_out as f64)
            .sum::<f64>()
            / n;
        let mean_usd = runs_of.iter().map(|r| r.scores.cost.usd).sum::<f64>() / n;
        let mean_cached = runs_of
            .iter()
            .map(|r| r.scores.cost.cached_tokens as f64)
            .sum::<f64>()
            / n;
        let mean_cache_write = runs_of
            .iter()
            .map(|r| r.scores.cost.cache_write_tokens as f64)
            .sum::<f64>()
            / n;
        let pct = if mean_in > 0.0 {
            std_in / mean_in * 100.0
        } else {
            0.0
        };
        rows.push([
            task_id.clone(),
            prompt_id.clone(),
            pass_rate,
            format!("{mean_tools:.1}"),
            format!("{mean_turns:.1}"),
            format!("{mean_in:.0}±{std_in:.0} ({pct:.0}%)"),
            format!("{mean_out:.0}"),
            format!("{mean_cached:.0}"),
            format!("{mean_cache_write:.0}"),
            format!("{mean_usd:.6}"),
        ]);
    }

    let mut widths = [0usize; 10];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |row: &[String; 10]| -> String {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<width$}", width = widths[i]))
            .collect();
        format!("| {} |", cells.join(" | "))
    };
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("Aggregated across {runs} runs per pair\n\n"));
    out.push_str(&render(&rows[0]));
    out.push('\n');
    out.push_str(&format!("|-{}-|\n", sep.join("-|-")));
    for row in &rows[1..] {
        out.push_str(&render(row));
        out.push('\n');
    }
    out.push('\n');
    if let Some(model_line) = requested_model_line(reported_results.iter().copied()) {
        out.push_str(&model_line);
    }
    let pass = reported_results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Pass)
        .count();
    let over_budget = reported_results
        .iter()
        .filter(|r| r.scores.outcome.passed && !r.scores.efficiency.tokens_in_budget)
        .count();
    let limited = reported_results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Limit)
        .count();
    let errors = reported_results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Error)
        .count();
    let fail = reported_results
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Fail)
        .count();
    let retries: usize = reported_results.iter().map(|r| retry_count(r)).sum();
    let retry_errors: usize = reported_results
        .iter()
        .map(|r| retry_error_attempt_count(r))
        .sum();
    out.push_str(&format!(
        "{pass} passed ({over_budget} over budget), {fail} failed, {limited} limited, {errors} errored of {} total{}\n",
        reported_results.len(),
        if retries > 0 {
            format!("; {retries} retr{} ({retry_errors} errored attempt{})",
                if retries == 1 { "y" } else { "ies" },
                if retry_errors == 1 { "" } else { "s" })
        } else {
            String::new()
        }
    ));
    if over_budget > 0 {
        out.push_str(
            "(`pass*` = correct outcome but token budget exceeded mid-run; not a failure)\n",
        );
    }
    let over_tool_call_guidance = reported_results
        .iter()
        .filter(|r| exceeded_max_tool_call_guidance(r) == Some(true))
        .count();
    if over_tool_call_guidance > 0 {
        out.push_str(&format!(
            "{over_tool_call_guidance} exceeded maximum tool-call guidance (not a failure)\n"
        ));
    }
    if let Some(causes) = failure_cause_summary(reported_results.iter().copied()) {
        out.push_str(&causes);
    }
    let cached_total = reported_results
        .iter()
        .map(|r| attempt_cost(r).cached_tokens)
        .sum::<u64>();
    let cache_write_total = reported_results
        .iter()
        .map(|r| attempt_cost(r).cache_write_tokens)
        .sum::<u64>();
    out.push_str(&format!(
        "cache tokens: {cached_total} read, {cache_write_total} written\n"
    ));
    let totals = RunTotals {
        prompt_tokens: reported_results
            .iter()
            .map(|r| attempt_cost(r).tokens_in)
            .sum(),
        completion_tokens: reported_results
            .iter()
            .map(|r| attempt_cost(r).tokens_out)
            .sum(),
        total_tokens: reported_results
            .iter()
            .map(|r| {
                let cost = attempt_cost(r);
                cost.tokens_in + cost.tokens_out
            })
            .sum(),
        usd: reported_results.iter().map(|r| attempt_cost(r).usd).sum(),
    };
    out.push_str(&format!(
        "totals: {} prompt + {} completion = {} tokens, ${:.6}\n",
        totals.prompt_tokens, totals.completion_tokens, totals.total_tokens, totals.usd
    ));
    out.push('\n');
    out
}

fn format_failure_details(results: &[TaskResult]) -> String {
    let mut out = String::new();
    let fails: Vec<&TaskResult> = results
        .iter()
        .filter(|r| !r.scores.outcome.passed)
        .collect();
    let retries: Vec<(&TaskResult, &RetryAttempt)> = results
        .iter()
        .flat_map(|result| {
            result
                .retry_attempts
                .iter()
                .map(move |retry| (result, retry))
        })
        .collect();
    if fails.is_empty() && retries.is_empty() {
        return out;
    }
    if !fails.is_empty() {
        out.push_str(&format!("failures ({}):\n", fails.len()));
        for r in fails {
            out.push_str(&format!("  {} | {}\n", r.task_id, r.prompt_id));
            if let Some(limit) = limit_reason(r) {
                out.push_str(&format!("    limit: {}\n", limit.label()));
            } else if let Some(e) = &r.scores.error {
                out.push_str(&format!("    error: {e}\n"));
            }
            if let Some(cause) = failure_cause(r) {
                out.push_str(&format!("    cause: {}\n", cause.label()));
            }
            if !r.scores.outcome.diff_files.is_empty() {
                for d in &r.scores.outcome.diff_files {
                    out.push_str(&format!("    diff: {} ({})\n", d.path, d.kind));
                }
            }
            if !r.tool_sequence.is_empty() {
                out.push_str(&format!("    tools: {}\n", r.tool_sequence.join(" -> ")));
            }
            if let Some(trace) = &r.trace {
                if !trace.tool_failures.is_empty() {
                    let failures = trace
                        .tool_failures
                        .iter()
                        .map(|failure| format!("{}: {}", failure.name, failure.detail))
                        .collect::<Vec<_>>()
                        .join("; ");
                    out.push_str(&format!("    tool failures: {failures}\n"));
                }
                if let Some(change) = &trace.routed_model_change {
                    out.push_str(&format!("    routed-model change: {change}\n"));
                }
                if let Some(change) = &trace.upstream_provider_change {
                    out.push_str(&format!("    upstream-provider switch: {change}\n"));
                }
                if trace.truncated {
                    out.push_str("    trace: truncated\n");
                }
            }
        }
    }
    if !retries.is_empty() {
        out.push_str(&format!("retries ({}):\n", retries.len()));
        for (result, retry) in retries {
            out.push_str(&format!(
                "  {} | {} | attempt {} | {}: {}\n",
                result.task_id,
                result.prompt_id,
                retry.attempt,
                retry.cause.label(),
                retry.error,
            ));
        }
    }
    out.push('\n');
    out
}

fn write_result(results_dir: &Path, r: &TaskResult, attempt: Option<u32>) -> Result<()> {
    fs::create_dir_all(results_dir)?;
    let name = format!("{}.json", result_artifact_stem(r, attempt));
    let path = results_dir.join(name);
    fs::write(&path, serde_json::to_string_pretty(r)?)?;
    Ok(())
}

fn result_artifact_stem(r: &TaskResult, attempt: Option<u32>) -> String {
    let base = if r.run_index > 0 {
        format!("{}__{}__run{}", r.task_id, r.prompt_id, r.run_index)
    } else {
        format!("{}__{}", r.task_id, r.prompt_id)
    };
    attempt
        .map(|number| format!("{base}__attempt{number}"))
        .unwrap_or(base)
}

fn compact_results_location(results_dir: &Path) -> String {
    let mut components: Vec<String> = results_dir
        .components()
        .rev()
        .take(3)
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    components.reverse();
    components.join("/")
}

fn write_transcript(results_dir: &Path, r: &TaskResult, attempt: Option<u32>) -> Result<()> {
    let transcript_dir = results_dir.join("transcripts");
    fs::create_dir_all(&transcript_dir)?;

    let header = json!({
        "type": "eval_transcript",
        "task_id": r.task_id,
        "prompt_id": r.prompt_id,
        "run_index": r.run_index,
        "model": r.model,
        "routed_models": r.routed_models,
        "upstream_providers": r.upstream_providers,
        "generation_ids": r.generation_ids,
        "provider_telemetry": r.provider_telemetry,
        "timestamp": r.timestamp,
        "heddle_commit": r.heddle_commit,
        "evals_version": r.evals_version,
    });
    let mut lines = vec![serde_json::to_string(&header)?];
    lines.extend(
        r.transcript
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?,
    );
    fs::write(
        transcript_dir.join(format!("{}.jsonl", result_artifact_stem(r, attempt))),
        format!("{}\n", lines.join("\n")),
    )?;
    Ok(())
}

fn write_error_artifact(results_dir: &Path, r: &TaskResult, attempt: Option<u32>) -> Result<()> {
    if result_status(r) != ResultStatus::Error {
        return Ok(());
    }
    let Some(error) = &r.scores.error else {
        return Ok(());
    };

    let error_dir = results_dir.join("errors");
    fs::create_dir_all(&error_dir)?;
    let stem = result_artifact_stem(r, attempt);
    let correlation = serde_json::to_string(&r.provider_telemetry)?;
    let contents = format!(
        "task: {}\nprompt: {}\nrun_index: {}\nmodel: {}\nrouted_models: {}\nupstream_providers: {}\ngeneration_ids: {}\nprovider_correlation: {}\ncause: {}\nturns: {}\ntool_calls: {}\ntokens: {}/{}\nusd: {:.6}\ntools: {}\nresult: ../{}.json\ntranscript: ../transcripts/{}.jsonl\n\nerror:\n{}\n",
        r.task_id,
        r.prompt_id,
        r.run_index,
        r.model,
        routed_model_summary(r),
        upstream_provider_summary(r),
        r.generation_ids.join(","),
        correlation,
        failure_cause(r).map(FailureCause::label).unwrap_or("unknown"),
        r.scores.efficiency.turns,
        r.scores.efficiency.tool_calls,
        r.scores.cost.tokens_in,
        r.scores.cost.tokens_out,
        r.scores.cost.usd,
        r.tool_sequence.join(" -> "),
        stem,
        stem,
        error,
    );
    fs::write(error_dir.join(format!("{stem}.log")), contents)?;
    Ok(())
}

fn write_result_artifacts(results_dir: &Path, r: &TaskResult) -> Result<()> {
    write_result(results_dir, r, None)?;
    write_transcript(results_dir, r, None)?;
    write_error_artifact(results_dir, r, None)
}

fn write_retry_attempt_artifacts(results_dir: &Path, r: &TaskResult, attempt: u32) -> Result<()> {
    write_result(results_dir, r, Some(attempt))?;
    write_transcript(results_dir, r, Some(attempt))?;
    write_error_artifact(results_dir, r, Some(attempt))
}

// ─── Main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::from_filename(".env.test");
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List { evals } => cmd_list(&evals),
        Cmd::Aggregate {
            results_root,
            runs,
            suite_label,
            profile_label,
            aggregate_root,
            output_dir,
        } => cmd_aggregate(
            &results_root,
            &runs,
            &suite_label,
            &profile_label,
            &aggregate_root,
            output_dir.as_deref(),
        ),
        Cmd::Compare {
            baseline,
            variant,
            name,
            tag,
            json,
        } => cmd_compare(&baseline, &variant, name.as_deref(), tag.as_deref(), json),
        Cmd::Run {
            evals,
            prompts,
            tasks,
            tags,
            model,
            max_tokens_per_task,
            max_tokens_per_response,
            max_turns,
            task_timeout_secs,
            budget_stop_usd,
            results_dir,
            tag,
            suite_label,
            runs,
            record_all_text,
            cache_prewarm,
            cache_ttl_1h,
            static_context_only,
            condition,
        } => {
            cmd_run(
                &evals,
                &prompts,
                &tasks,
                &tags,
                model.as_deref(),
                max_tokens_per_task,
                max_tokens_per_response,
                max_turns,
                task_timeout_secs,
                budget_stop_usd,
                results_dir,
                tag.as_deref(),
                suite_label.as_deref(),
                runs.max(1),
                record_all_text,
                cache_prewarm,
                cache_ttl_1h,
                static_context_only,
                condition.as_deref(),
            )
            .await
        }
    }
}

#[derive(Debug, Deserialize)]
struct StoredComparisonMeta {
    model: String,
    #[serde(default = "default_runs_per_case")]
    runs_per_case: u32,
    comparison: ComparisonConfig,
    #[serde(default)]
    comparison_identity: Option<ComparisonIdentity>,
    suite: SuiteIdentity,
    #[serde(default)]
    condition: Option<ResolvedEvalCondition>,
}

fn default_runs_per_case() -> u32 {
    1
}

struct ComparisonRun {
    dir: PathBuf,
    meta: StoredComparisonMeta,
    results: Vec<TaskResult>,
}

#[derive(Debug, Serialize)]
struct ComparisonDelta {
    prompt_tokens: i64,
    completion_tokens: i64,
    tool_calls: i64,
    turns: i64,
    duration_ms: i128,
    usd: f64,
}

#[derive(Debug, Default, Serialize)]
struct OutcomeCounts {
    pass: usize,
    fail: usize,
    limit: usize,
    error: usize,
    total: usize,
}

impl OutcomeCounts {
    fn record(&mut self, result: &TaskResult) {
        self.total += 1;
        match result_status(result) {
            ResultStatus::Pass => self.pass += 1,
            ResultStatus::Fail => self.fail += 1,
            ResultStatus::Limit => self.limit += 1,
            ResultStatus::Error => self.error += 1,
        }
    }

    fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.pass as f64 / self.total as f64
        }
    }
}

#[derive(Debug, Serialize)]
struct PairedComparisonReport {
    baseline: String,
    variant: String,
    condition: ResolvedEvalCondition,
    pairs: usize,
    baseline_outcomes: OutcomeCounts,
    variant_outcomes: OutcomeCounts,
    outcome_transitions: BTreeMap<String, usize>,
    task_outcome_transitions: BTreeMap<String, BTreeMap<String, usize>>,
    variant_minus_baseline: ComparisonDelta,
}

fn load_comparison_run(dir: &Path) -> Result<ComparisonRun> {
    let meta_path = dir.join("run_meta.json");
    let results_path = dir.join("summary.json");
    let meta: StoredComparisonMeta = serde_json::from_str(
        &fs::read_to_string(&meta_path)
            .with_context(|| format!("reading {}", meta_path.display()))?,
    )
    .with_context(|| format!("parsing {}", meta_path.display()))?;
    let results: Vec<TaskResult> = serde_json::from_str(
        &fs::read_to_string(&results_path)
            .with_context(|| format!("reading {}", results_path.display()))?,
    )
    .with_context(|| format!("parsing {}", results_path.display()))?;
    Ok(ComparisonRun {
        dir: dir.to_path_buf(),
        meta,
        results,
    })
}

fn controls_match(left: &ComparisonConfig, right: &ComparisonConfig) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.condition = None;
    right.condition = None;
    left == right
}

fn controls_match_except_prompt(left: &ComparisonConfig, right: &ComparisonConfig) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.condition = None;
    right.condition = None;
    left.prompts.clear();
    right.prompts.clear();
    left.prompt_conditions.clear();
    right.prompt_conditions.clear();
    left == right
}

fn single_prompt_condition(meta: &StoredComparisonMeta, side: &str) -> Result<PromptCondition> {
    if meta.comparison.prompts.len() != 1 || meta.comparison.prompt_conditions.len() != 1 {
        bail!(
            "{side} prompt comparison requires exactly one selected prompt with retained role metadata; rerun each side with one prompt using the current eval runner"
        );
    }
    let prompt = meta.comparison.prompt_conditions[0].clone();
    if meta.comparison.prompts[0] != prompt.id {
        bail!("{side} prompt comparison metadata does not match its selected prompt");
    }
    Ok(prompt)
}

fn status_label(result: &TaskResult) -> &'static str {
    match result_status(result) {
        ResultStatus::Pass => "pass",
        ResultStatus::Fail => "fail",
        ResultStatus::Limit => "limit",
        ResultStatus::Error => "error",
    }
}

fn build_paired_comparison(
    baseline: &ComparisonRun,
    variant: &ComparisonRun,
) -> Result<PairedComparisonReport> {
    if baseline.meta.model != variant.meta.model {
        bail!(
            "requested model mismatch: baseline {:?}, variant {:?}",
            baseline.meta.model,
            variant.meta.model
        );
    }
    if baseline.meta.runs_per_case != variant.meta.runs_per_case {
        bail!(
            "runs-per-case mismatch: baseline {}, variant {}",
            baseline.meta.runs_per_case,
            variant.meta.runs_per_case
        );
    }
    let (condition, prompt_condition_mode) = if baseline.meta.suite.fingerprint
        == variant.meta.suite.fingerprint
    {
        if !controls_match(&baseline.meta.comparison, &variant.meta.comparison) {
            bail!("comparison controls differ outside the declared condition");
        }
        let condition = variant
            .meta
            .condition
            .clone()
            .or_else(|| variant.meta.comparison.condition.clone())
            .ok_or_else(|| anyhow!("variant run has no declared eval condition"))?;
        if let Some(baseline_condition) = baseline.meta.condition.as_ref().or(baseline
            .meta
            .comparison
            .condition
            .as_ref())
        {
            if baseline_condition.declaration.id != condition.declaration.baseline {
                bail!(
                    "baseline condition {:?} does not match variant declaration baseline {:?}",
                    baseline_condition.declaration.id,
                    condition.declaration.baseline
                );
            }
        }
        (condition, false)
    } else {
        if !controls_match_except_prompt(&baseline.meta.comparison, &variant.meta.comparison) {
            bail!("prompt-condition comparison controls differ outside the selected prompt");
        }
        let baseline_identity = baseline.meta.comparison_identity.as_ref().ok_or_else(|| {
            anyhow!("baseline lacks a comparison fingerprint; rerun with the current eval runner")
        })?;
        let variant_identity = variant.meta.comparison_identity.as_ref().ok_or_else(|| {
            anyhow!("variant lacks a comparison fingerprint; rerun with the current eval runner")
        })?;
        if baseline_identity.fingerprint != variant_identity.fingerprint {
            bail!("prompt-condition comparison fingerprint mismatch; task inputs or non-prompt controls changed");
        }
        let baseline_prompt = single_prompt_condition(&baseline.meta, "baseline")?;
        let variant_prompt = single_prompt_condition(&variant.meta, "variant")?;
        let condition = variant
            .meta
            .condition
            .clone()
            .or_else(|| variant.meta.comparison.condition.clone())
            .ok_or_else(|| anyhow!("variant prompt comparison requires --condition <toml>"))?;
        if baseline.meta.condition.is_some() || baseline.meta.comparison.condition.is_some() {
            bail!("prompt-condition comparison baseline must not declare a harness condition");
        }
        if condition.declaration.baseline != baseline_prompt.id
            || condition.declaration.variant != variant_prompt.id
        {
            bail!(
                "prompt comparison condition expects {} -> {}, but runs select {} -> {}",
                condition.declaration.baseline,
                condition.declaration.variant,
                baseline_prompt.id,
                variant_prompt.id
            );
        }
        (condition, true)
    };

    let key = |result: &TaskResult| {
        (
            result.task_id.clone(),
            if prompt_condition_mode {
                "<prompt-condition>".to_string()
            } else {
                result.prompt_id.clone()
            },
            result.run_index,
        )
    };
    let baseline_by_key: BTreeMap<_, _> = baseline
        .results
        .iter()
        .map(|result| (key(result), result))
        .collect();
    let variant_by_key: BTreeMap<_, _> = variant
        .results
        .iter()
        .map(|result| (key(result), result))
        .collect();
    let baseline_keys: BTreeSet<_> = baseline_by_key.keys().cloned().collect();
    let variant_keys: BTreeSet<_> = variant_by_key.keys().cloned().collect();
    if baseline_keys != variant_keys {
        bail!("baseline and variant result pairs differ; rerun with the same task and repetition selection");
    }

    let mut baseline_outcomes = OutcomeCounts::default();
    let mut variant_outcomes = OutcomeCounts::default();
    let mut transitions = BTreeMap::new();
    let mut task_transitions: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut delta = ComparisonDelta {
        prompt_tokens: 0,
        completion_tokens: 0,
        tool_calls: 0,
        turns: 0,
        duration_ms: 0,
        usd: 0.0,
    };
    for key in baseline_keys {
        let base = baseline_by_key[&key];
        let changed = variant_by_key[&key];
        baseline_outcomes.record(base);
        variant_outcomes.record(changed);
        let transition = format!("{}->{}", status_label(base), status_label(changed));
        *transitions.entry(transition.clone()).or_default() += 1;
        *task_transitions
            .entry(base.task_id.clone())
            .or_default()
            .entry(transition)
            .or_default() += 1;
        delta.prompt_tokens +=
            changed.scores.cost.tokens_in as i64 - base.scores.cost.tokens_in as i64;
        delta.completion_tokens +=
            changed.scores.cost.tokens_out as i64 - base.scores.cost.tokens_out as i64;
        delta.tool_calls +=
            changed.scores.efficiency.tool_calls as i64 - base.scores.efficiency.tool_calls as i64;
        delta.turns += changed.scores.efficiency.turns as i64 - base.scores.efficiency.turns as i64;
        delta.duration_ms += changed.duration_ms as i128 - base.duration_ms as i128;
        delta.usd += changed.scores.cost.usd - base.scores.cost.usd;
    }
    Ok(PairedComparisonReport {
        baseline: baseline.dir.display().to_string(),
        variant: variant.dir.display().to_string(),
        condition,
        pairs: baseline_by_key.len(),
        baseline_outcomes,
        variant_outcomes,
        outcome_transitions: transitions,
        task_outcome_transitions: task_transitions,
        variant_minus_baseline: delta,
    })
}

fn comparison_output_path(
    report: &PairedComparisonReport,
    name: Option<&str>,
    tag: Option<&str>,
) -> PathBuf {
    let mut stem = result_name_component(
        name.unwrap_or(&report.condition.declaration.id),
        "comparison",
    );
    if let Some(tag) = tag {
        stem.push_str("__");
        stem.push_str(&result_name_component(tag, "tag"));
    }
    PathBuf::from("evals")
        .join("comparisons")
        .join(format!("{stem}.json"))
}

fn write_comparison_report(output: &Path, serialized: &str) -> Result<()> {
    if output.exists() {
        bail!(
            "comparison report {} already exists; use --tag to retain another comparison",
            output.display()
        );
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("creating comparison output directory {}", parent.display())
        })?;
    }
    fs::write(output, serialized)
        .with_context(|| format!("writing comparison report {}", output.display()))
}

fn existing_report_matches(output: &Path, report: &PairedComparisonReport) -> Result<bool> {
    let existing: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output)
            .with_context(|| format!("reading existing comparison report {}", output.display()))?,
    )
    .with_context(|| format!("parsing existing comparison report {}", output.display()))?;
    Ok(existing.get("baseline") == Some(&json!(report.baseline))
        && existing.get("variant") == Some(&json!(report.variant)))
}

fn format_comparison_summary(report: &PairedComparisonReport, output: &Path) -> String {
    let mut lines = vec![
        format!("Comparison: {}", report.condition.declaration.id),
        format!("Hypothesis: {}", report.condition.declaration.hypothesis),
        format!(
            "Changed factor: {}",
            report.condition.declaration.changed_factor
        ),
        format!("Pairs: {}", report.pairs),
        format!(
            "Outcomes: baseline {}/{} pass ({:.0}%) -> variant {}/{} pass ({:.0}%)",
            report.baseline_outcomes.pass,
            report.baseline_outcomes.total,
            report.baseline_outcomes.pass_rate() * 100.0,
            report.variant_outcomes.pass,
            report.variant_outcomes.total,
            report.variant_outcomes.pass_rate() * 100.0,
        ),
        "Transitions:".into(),
    ];
    for (transition, count) in &report.outcome_transitions {
        lines.push(format!("  {transition}: {count}"));
    }
    lines.push("Per task:".into());
    for (task, transitions) in &report.task_outcome_transitions {
        let details = transitions
            .iter()
            .map(|(transition, count)| format!("{transition}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("  {task}: {details}"));
    }
    let delta = &report.variant_minus_baseline;
    lines.push("Variant minus baseline:".into());
    lines.push(format!(
        "  prompt tokens: {:+}; completion tokens: {:+}; tool calls: {:+}; turns: {:+}",
        delta.prompt_tokens, delta.completion_tokens, delta.tool_calls, delta.turns
    ));
    lines.push(format!(
        "  duration: {:+}ms; cost: {:+.6} USD",
        delta.duration_ms, delta.usd
    ));
    lines.push(format!("Baseline: {}", report.baseline));
    lines.push(format!("Variant: {}", report.variant));
    lines.push(format!("JSON report: {}", output.display()));
    lines.join("\n")
}

fn cmd_compare(
    baseline: &Path,
    variant: &Path,
    name: Option<&str>,
    tag: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let report = build_paired_comparison(
        &load_comparison_run(baseline)?,
        &load_comparison_run(variant)?,
    )?;
    let serialized = serde_json::to_string_pretty(&report)?;
    let output = comparison_output_path(&report, name, tag);
    if output.exists() {
        if !existing_report_matches(&output, &report)? {
            bail!(
                "comparison report {} already exists for different inputs; use --tag to retain another comparison",
                output.display()
            );
        }
    } else {
        write_comparison_report(&output, &serialized)?;
    }
    if json_output {
        println!("{serialized}");
    } else {
        println!("{}", format_comparison_summary(&report, &output));
    }
    Ok(())
}

fn cmd_list(evals: &Path) -> Result<()> {
    let manifest = load_manifest(evals)?;
    let prompts = load_prompts(evals)?;
    let tasks = load_tasks(evals)?;
    println!("manifest: version={}", manifest.version);
    println!();
    println!("prompts ({}):", prompts.len());
    for p in &prompts {
        let chars = p.body.chars().count();
        let cwd = p.front.context.cwd;
        let date = p.front.context.date;
        let git = p.front.context.git.is_some();
        let tree = p.front.context.file_tree.is_some();
        let matrix = if p.front.matrix_exclude {
            "excluded"
        } else {
            "active"
        };
        let role = p.front.role.as_deref().unwrap_or("unclassified");
        println!(
            "  {:<28} matrix={:<8} role={:<28} body={:>5}c  cwd={} date={} git={} tree={}",
            p.id, matrix, role, chars, cwd, date, git, tree
        );
    }
    println!();
    println!("tasks ({}):", tasks.len());
    for t in &tasks {
        println!(
            "  {:<28} tags={:<36} max_turns={}  timeout={}s  tools={:?}",
            t.spec.id,
            if t.spec.tags.is_empty() {
                "-".to_string()
            } else {
                t.spec.tags.join(",")
            },
            t.spec.max_turns.unwrap_or(8),
            t.spec.task_timeout_secs.unwrap_or(150),
            t.spec.tools.as_ref().map(|v| v.len()).unwrap_or(7),
        );
    }
    Ok(())
}

fn select<'a, T, F>(all: &'a [T], wanted: &str, id_of: F) -> Result<Vec<&'a T>>
where
    F: Fn(&T) -> &str,
{
    if wanted == "all" {
        return Ok(all.iter().collect());
    }
    let names: Vec<&str> = wanted.split(',').map(|s| s.trim()).collect();
    let mut out = Vec::new();
    for name in names {
        let m = all
            .iter()
            .find(|x| id_of(x) == name)
            .ok_or_else(|| anyhow!("unknown id: {name}"))?;
        out.push(m);
    }
    Ok(out)
}

fn select_task_tags<'a>(tasks: Vec<&'a Task>, wanted: &str) -> Result<Vec<&'a Task>> {
    if wanted == "all" {
        return Ok(tasks);
    }
    let tags: Vec<&str> = wanted
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .collect();
    if tags.is_empty() {
        bail!("--tags must name at least one tag or use 'all'");
    }
    let selected: Vec<&Task> = tasks
        .into_iter()
        .filter(|task| {
            tags.iter()
                .any(|tag| task.spec.tags.iter().any(|task_tag| task_tag == tag))
        })
        .collect();
    if selected.is_empty() {
        bail!("no selected tasks match tags: {}", tags.join(", "));
    }
    Ok(selected)
}

fn result_name_component(value: &str, fallback: &str) -> String {
    let component: String = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '-',
        })
        .collect();
    let component = component.trim_matches(['.', '-', '_']);
    if component.is_empty() || component == "." || component == ".." {
        fallback.to_string()
    } else {
        component.to_string()
    }
}

fn model_result_dir_name(model: &str) -> String {
    if model == "openrouter/free" {
        return "openrouter-free".to_string();
    }
    result_name_component(model.rsplit('/').next().unwrap_or(model), "model")
}

fn default_result_dir_name(timestamp: &str, _model: &str, tag: Option<&str>) -> String {
    let mut name = timestamp.to_string();
    if let Some(tag) = tag {
        name.push('_');
        name.push_str(&result_name_component(tag, "tag"));
    }
    name
}

#[derive(Debug, Clone, Serialize)]
struct StaticContextExclusion {
    prompt_id: String,
    dynamic_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StaticContextSelection {
    enabled: bool,
    excluded_prompts: Vec<StaticContextExclusion>,
}

fn apply_static_context_selection(
    prompts: &mut Vec<&Prompt>,
    prompts_arg: &str,
    enabled: bool,
) -> Result<StaticContextSelection> {
    if !enabled {
        return Ok(StaticContextSelection {
            enabled: false,
            excluded_prompts: Vec::new(),
        });
    }

    let dynamic: Vec<StaticContextExclusion> = prompts
        .iter()
        .filter_map(|prompt| {
            let dynamic_features = prompt.front.context.dynamic_features();
            (!dynamic_features.is_empty()).then(|| StaticContextExclusion {
                prompt_id: prompt.id.clone(),
                dynamic_features: dynamic_features.into_iter().map(str::to_string).collect(),
            })
        })
        .collect();

    if prompts_arg != "all" && !dynamic.is_empty() {
        let details = dynamic
            .iter()
            .map(|excluded| {
                format!(
                    "{} ({})",
                    excluded.prompt_id,
                    excluded.dynamic_features.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!("--static-context-only excludes explicitly selected dynamic prompt(s): {details}");
    }

    prompts.retain(|prompt| prompt.front.context.dynamic_features().is_empty());
    Ok(StaticContextSelection {
        enabled: true,
        excluded_prompts: dynamic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_prompts_discovers_nested_conditions_and_rejects_duplicate_ids() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = dir.path().join("prompts");
        fs::create_dir_all(prompts.join("conditions")).unwrap();
        fs::write(prompts.join("baseline.md"), "---\nid: baseline\n---\nBase").unwrap();
        fs::write(
            prompts.join("conditions/verification.md"),
            "---\nid: verification\nrole: post-edit-verification\nhypothesis: Focused checks improve correctness.\n---\nVerify",
        )
        .unwrap();

        let loaded = load_prompts(dir.path()).unwrap();
        assert_eq!(
            loaded
                .iter()
                .map(|prompt| prompt.id.as_str())
                .collect::<Vec<_>>(),
            vec!["baseline", "verification"]
        );
        assert_eq!(
            loaded[1].front.role.as_deref(),
            Some("post-edit-verification")
        );
        assert_eq!(
            loaded[1].front.hypothesis.as_deref(),
            Some("Focused checks improve correctness.")
        );

        fs::write(
            prompts.join("conditions/duplicate.md"),
            "---\nid: baseline\n---\nDuplicate",
        )
        .unwrap();
        assert!(load_prompts(dir.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate prompt id"));
    }

    #[test]
    fn eval_condition_is_validated_and_fingerprinted() {
        let dir = tempfile::tempdir().unwrap();
        let condition_path = dir.path().join("range-read.toml");
        fs::write(
            &condition_path,
            r#"
id = "range-read-v1"
hypothesis = "Bounded reads reduce retrieval cost."
baseline = "full-read-v1"
variant = "range-read-v1"
changed_factor = "read_file_contract"
expected_signal = "fewer input tokens"
"#,
        )
        .unwrap();

        let first = load_eval_condition(&condition_path).unwrap();
        let second = load_eval_condition(&condition_path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.declaration.id, "range-read-v1");
        assert_eq!(first.fingerprint.len(), 64);

        fs::write(
            &condition_path,
            r#"
id = "invalid"
hypothesis = "x"
baseline = "same"
variant = "same"
changed_factor = "x"
expected_signal = "x"
"#,
        )
        .unwrap();
        assert!(load_eval_condition(&condition_path)
            .unwrap_err()
            .to_string()
            .contains("baseline and variant must differ"));
    }

    #[test]
    fn input_contract_fingerprints_prompt_and_tool_schema_deterministically() {
        let messages = vec![Message::System(SystemMessage {
            content: "Inspect only the requested files.".into(),
        })];
        let forward = build_registry(&["read_file".into(), "grep".into()]).unwrap();
        let reverse = build_registry(&["grep".into(), "read_file".into()]).unwrap();
        let first = eval_input_contract(&messages, &forward).unwrap();
        let reordered = eval_input_contract(&messages, &reverse).unwrap();

        assert_eq!(first, reordered);
        assert_eq!(first.tools, vec!["grep", "read_file"]);
        assert_eq!(first.rendered_system_prompt_sha256.len(), 64);
        assert_eq!(first.tool_schema_sha256.len(), 64);

        let changed_messages = vec![Message::System(SystemMessage {
            content: "Inspect files and run the focused test.".into(),
        })];
        let changed_prompt = eval_input_contract(&changed_messages, &forward).unwrap();
        let changed_tools =
            eval_input_contract(&messages, &build_registry(&["read_file".into()]).unwrap())
                .unwrap();
        assert_ne!(
            first.rendered_system_prompt_sha256,
            changed_prompt.rendered_system_prompt_sha256
        );
        assert_ne!(first.tool_schema_sha256, changed_tools.tool_schema_sha256);
    }

    #[test]
    fn generated_result_name_includes_optional_tag() {
        assert_eq!(
            default_result_dir_name("20260728T010203", "z-ai/glm-4.7-flash", Some("cache trial"),),
            "20260728T010203_cache-trial"
        );
        assert_eq!(
            default_result_dir_name("20260728T010203", "openrouter/free", None),
            "20260728T010203"
        );
    }

    #[test]
    fn compact_results_location_uses_suite_model_and_run() {
        assert_eq!(
            compact_results_location(Path::new(
                "/tmp/heddle-eval-results/base-evals-v1.1__s-7f1a908e/deepseek-v4-flash/20260806T040420"
            )),
            "base-evals-v1.1__s-7f1a908e/deepseek-v4-flash/20260806T040420"
        );
    }

    #[cfg(unix)]
    #[test]
    fn suite_root_creation_writes_metadata_and_is_reused_by_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let evals_dir = dir.path().join("evals");
        let results_repo = dir.path().join("heddle-eval-results");
        std::fs::create_dir_all(&evals_dir).unwrap();
        std::fs::create_dir_all(results_repo.join(".git")).unwrap();
        std::os::unix::fs::symlink(&results_repo, evals_dir.join("results")).unwrap();
        let suite = SuiteIdentity {
            fingerprint: "1234567890abcdef".into(),
            source: "test".into(),
        };

        let created =
            resolve_suite_root(&evals_dir, &suite, Some("focused repair"), false).unwrap();
        assert_eq!(created.file_name().unwrap(), "focused-repair__s-12345678");
        assert_eq!(
            suite_from_root(&created).unwrap().unwrap().fingerprint,
            suite.fingerprint
        );

        let reused = resolve_suite_root(&evals_dir, &suite, None, false).unwrap();
        assert_eq!(reused, created);

        let changed_suite = SuiteIdentity {
            fingerprint: "abcdef1234567890".into(),
            source: "test".into(),
        };
        let error = resolve_suite_root(&evals_dir, &changed_suite, Some("focused repair"), true)
            .unwrap_err();
        assert!(error.to_string().contains("bump suite_label"));
        assert!(
            resolve_suite_root(&evals_dir, &changed_suite, Some("focused repair v2"), true,)
                .is_ok()
        );
    }

    #[test]
    fn openrouter_free_uses_a_distinct_result_directory() {
        assert_eq!(model_result_dir_name("openrouter/free"), "openrouter-free");
        assert_eq!(
            model_result_dir_name("inclusionai/ling-3.0-flash:free"),
            "ling-3.0-flash-free"
        );
    }

    fn result(
        cached_tokens: u64,
        cache_write_tokens: u64,
        routed_model: Option<&str>,
    ) -> TaskResult {
        TaskResult {
            task_id: "task-1".into(),
            prompt_id: "prompt-1".into(),
            tags: Vec::new(),
            model: "openrouter/auto".into(),
            routed_models: routed_model
                .map(|model| {
                    vec![RoutedModelObservation {
                        assistant_turn: 1,
                        model: model.into(),
                    }]
                })
                .unwrap_or_default(),
            upstream_providers: Vec::new(),
            generation_ids: Vec::new(),
            provider_telemetry: Vec::new(),
            debug_errors: Vec::new(),
            call_telemetry: Vec::new(),
            heddle_commit: "abc123".into(),
            evals_version: "0.1.0".into(),
            timestamp: "2026-07-22T00:00:00Z".into(),
            duration_ms: 1,
            scores: Scores {
                outcome: OutcomeScore {
                    passed: true,
                    exact_passed: true,
                    diff_files: Vec::new(),
                    protected_diffs: Vec::new(),
                    semantic_verification: None,
                },
                efficiency: EfficiencyScore {
                    tool_calls: 1,
                    turns: 1,
                    tool_calls_in_range: true,
                    max_tool_calls: None,
                    exceeded_max_tool_calls: None,
                    tokens_in_budget: true,
                },
                cost: CostScore {
                    tokens_in: 100,
                    tokens_out: 20,
                    cached_tokens,
                    cache_write_tokens,
                    usd: 0.01,
                },
                limit: None,
                failure_cause: None,
                error: None,
            },
            rendered_system_prompt_chars: 10,
            input_contract: None,
            run_index: 0,
            retry_attempts: Vec::new(),
            tool_sequence: Vec::new(),
            finish_reasons: Vec::new(),
            assistant_messages: Vec::new(),
            trace: None,
            transcript: Vec::new(),
        }
    }

    fn test_condition() -> ResolvedEvalCondition {
        ResolvedEvalCondition {
            declaration: EvalCondition {
                id: "range-read-v1".into(),
                hypothesis: "Bounded reads reduce retrieval cost.".into(),
                baseline: "full-read-v1".into(),
                variant: "range-read-v1".into(),
                changed_factor: "read_file_contract".into(),
                expected_signal: "fewer input tokens".into(),
            },
            fingerprint: "condition-fingerprint".into(),
        }
    }

    fn test_comparison(condition: Option<ResolvedEvalCondition>) -> ComparisonConfig {
        ComparisonConfig {
            prompts: vec!["prompt-1".into()],
            prompt_conditions: Vec::new(),
            tasks: vec!["task-1".into()],
            max_tokens_per_task: 1_000,
            max_tokens_per_response: 500,
            max_turns: 4,
            task_timeout_secs: 60,
            static_context_only: false,
            excluded_dynamic_prompts: Vec::new(),
            cache_prewarm: false,
            cache_ttl: None,
            openrouter_routing: "balanced".into(),
            condition,
        }
    }

    fn test_prompt_comparison(id: &str, role: &str, hypothesis: &str) -> ComparisonConfig {
        let mut comparison = test_comparison(None);
        comparison.prompts = vec![id.into()];
        comparison.prompt_conditions = vec![PromptCondition {
            id: id.into(),
            role: role.into(),
            hypothesis: hypothesis.into(),
        }];
        comparison
    }

    fn test_prompt_condition() -> ResolvedEvalCondition {
        ResolvedEvalCondition {
            declaration: EvalCondition {
                id: "verify-after-edit-vs-default-v1".into(),
                hypothesis: "Focused checks improve correctness.".into(),
                baseline: "default".into(),
                variant: "verify-after-edit".into(),
                changed_factor: "system_prompt_condition".into(),
                expected_signal: "Fewer multi-file failures.".into(),
            },
            fingerprint: "prompt-condition-fingerprint".into(),
        }
    }

    fn test_comparison_identity(fingerprint: &str) -> ComparisonIdentity {
        ComparisonIdentity {
            fingerprint: fingerprint.into(),
            source: "test".into(),
        }
    }

    #[test]
    fn paired_comparison_requires_matching_controls_and_reports_deltas() {
        let baseline = ComparisonRun {
            dir: PathBuf::from("baseline"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 1,
                comparison: test_comparison(None),
                comparison_identity: None,
                suite: SuiteIdentity {
                    fingerprint: "suite".into(),
                    source: "test".into(),
                },
                condition: None,
            },
            results: vec![result(0, 0, None)],
        };
        let mut changed = result(0, 0, None);
        changed.scores.cost.tokens_in = 80;
        changed.scores.cost.tokens_out = 15;
        changed.scores.efficiency.tool_calls = 2;
        changed.scores.efficiency.turns = 2;
        changed.duration_ms = 5;
        changed.scores.cost.usd = 0.02;
        let variant = ComparisonRun {
            dir: PathBuf::from("variant"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 1,
                comparison: test_comparison(Some(test_condition())),
                comparison_identity: None,
                suite: SuiteIdentity {
                    fingerprint: "suite".into(),
                    source: "test".into(),
                },
                condition: Some(test_condition()),
            },
            results: vec![changed],
        };

        let report = build_paired_comparison(&baseline, &variant).unwrap();
        assert_eq!(report.pairs, 1);
        assert_eq!(report.baseline_outcomes.pass, 1);
        assert_eq!(report.variant_outcomes.pass, 1);
        assert_eq!(report.outcome_transitions["pass->pass"], 1);
        assert_eq!(report.task_outcome_transitions["task-1"]["pass->pass"], 1);
        assert_eq!(report.variant_minus_baseline.prompt_tokens, -20);
        assert_eq!(report.variant_minus_baseline.completion_tokens, -5);
        assert_eq!(report.variant_minus_baseline.tool_calls, 1);
        assert_eq!(report.variant_minus_baseline.turns, 1);
        assert_eq!(report.variant_minus_baseline.duration_ms, 4);
        assert!((report.variant_minus_baseline.usd - 0.01).abs() < f64::EPSILON);
        let output = comparison_output_path(&report, None, Some("rerun-2"));
        assert_eq!(
            output,
            PathBuf::from("evals/comparisons/range-read-v1__rerun-2.json")
        );
        let summary = format_comparison_summary(&report, &output);
        assert!(summary.contains("baseline 1/1 pass (100%) -> variant 1/1 pass (100%)"));
        assert!(summary.contains("task-1: pass->pass=1"));
        assert!(summary.contains("JSON report: evals/comparisons/range-read-v1__rerun-2.json"));

        let report_dir = tempfile::tempdir().unwrap();
        let report_path = report_dir.path().join("comparison.json");
        write_comparison_report(&report_path, "{}").unwrap();
        assert!(write_comparison_report(&report_path, "{}")
            .unwrap_err()
            .to_string()
            .contains("already exists"));
        let matching_path = report_dir.path().join("matching.json");
        std::fs::write(
            &matching_path,
            json!({ "baseline": report.baseline, "variant": report.variant }).to_string(),
        )
        .unwrap();
        assert!(existing_report_matches(&matching_path, &report).unwrap());

        let mut mismatched = ComparisonRun {
            dir: variant.dir.clone(),
            meta: StoredComparisonMeta {
                model: variant.meta.model.clone(),
                runs_per_case: variant.meta.runs_per_case,
                comparison: variant.meta.comparison.clone(),
                comparison_identity: variant.meta.comparison_identity.clone(),
                suite: variant.meta.suite.clone(),
                condition: variant.meta.condition.clone(),
            },
            results: vec![result(0, 0, None)],
        };
        mismatched.meta.comparison.max_turns = 5;
        assert!(build_paired_comparison(&baseline, &mismatched)
            .unwrap_err()
            .to_string()
            .contains("controls differ"));
    }

    #[test]
    fn paired_comparison_allows_one_prompt_condition_with_matching_task_inputs() {
        let mut baseline_result = result(0, 0, None);
        baseline_result.prompt_id = "default".into();
        let mut variant_result = result(0, 0, None);
        variant_result.prompt_id = "verify-after-edit".into();
        variant_result.scores.efficiency.tool_calls = 2;

        let baseline = ComparisonRun {
            dir: PathBuf::from("baseline"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 3,
                comparison: test_prompt_comparison(
                    "default",
                    "production-baseline",
                    "Production reference.",
                ),
                comparison_identity: Some(test_comparison_identity("same-controls")),
                suite: SuiteIdentity {
                    fingerprint: "default-suite".into(),
                    source: "test".into(),
                },
                condition: None,
            },
            results: vec![baseline_result],
        };
        let variant = ComparisonRun {
            dir: PathBuf::from("variant"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 3,
                comparison: test_prompt_comparison(
                    "verify-after-edit",
                    "post-edit-verification",
                    "Focused checks improve correctness.",
                ),
                comparison_identity: Some(test_comparison_identity("same-controls")),
                suite: SuiteIdentity {
                    fingerprint: "verification-suite".into(),
                    source: "test".into(),
                },
                condition: Some(test_prompt_condition()),
            },
            results: vec![variant_result],
        };

        let report = build_paired_comparison(&baseline, &variant).unwrap();
        assert_eq!(
            report.condition.declaration.id,
            "verify-after-edit-vs-default-v1"
        );
        assert_eq!(
            report.condition.declaration.changed_factor,
            "system_prompt_condition"
        );
        assert_eq!(report.pairs, 1);
        assert_eq!(report.variant_minus_baseline.tool_calls, 1);
    }

    #[test]
    fn prompt_comparison_rejects_changed_controls() {
        let baseline = ComparisonRun {
            dir: PathBuf::from("baseline"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 3,
                comparison: test_prompt_comparison("default", "production-baseline", "Reference."),
                comparison_identity: Some(test_comparison_identity("same-controls")),
                suite: SuiteIdentity {
                    fingerprint: "default-suite".into(),
                    source: "test".into(),
                },
                condition: None,
            },
            results: vec![result(0, 0, None)],
        };
        let mut variant_comparison = test_prompt_comparison(
            "verify-after-edit",
            "post-edit-verification",
            "Focused checks improve correctness.",
        );
        variant_comparison.max_turns = 5;
        let variant = ComparisonRun {
            dir: PathBuf::from("variant"),
            meta: StoredComparisonMeta {
                model: "model/a".into(),
                runs_per_case: 3,
                comparison: variant_comparison,
                comparison_identity: Some(test_comparison_identity("different-controls")),
                suite: SuiteIdentity {
                    fingerprint: "verification-suite".into(),
                    source: "test".into(),
                },
                condition: None,
            },
            results: vec![result(0, 0, None)],
        };

        assert!(build_paired_comparison(&baseline, &variant)
            .unwrap_err()
            .to_string()
            .contains("controls differ outside the selected prompt"));
    }

    #[test]
    fn result_artifact_omits_requested_model_and_serializes_routing_only_when_needed() {
        let value =
            serde_json::to_value(result(75, 25, Some("anthropic/claude-sonnet-4"))).unwrap();
        assert!(value.get("model").is_none());
        assert!(value.get("routed_model").is_none());
        assert_eq!(
            value["routed_models"][0]["model"],
            "anthropic/claude-sonnet-4"
        );
        assert_eq!(value["scores"]["cost"]["cached_tokens"], 75);
        assert_eq!(value["scores"]["cost"]["cache_write_tokens"], 25);
    }

    #[test]
    fn older_result_artifacts_default_missing_optional_metrics() {
        let mut value = serde_json::to_value(result(75, 25, None)).unwrap();
        value["scores"]["cost"]
            .as_object_mut()
            .unwrap()
            .remove("cached_tokens");
        value["scores"]["cost"]
            .as_object_mut()
            .unwrap()
            .remove("cache_write_tokens");
        value["scores"]["efficiency"]
            .as_object_mut()
            .unwrap()
            .remove("max_tool_calls");
        value["scores"]["efficiency"]
            .as_object_mut()
            .unwrap()
            .remove("exceeded_max_tool_calls");

        let parsed: TaskResult = serde_json::from_value(value).unwrap();

        assert_eq!(parsed.scores.cost.cached_tokens, 0);
        assert_eq!(parsed.scores.cost.cache_write_tokens, 0);
        assert_eq!(parsed.scores.efficiency.max_tool_calls, None);
        assert_eq!(parsed.scores.efficiency.exceeded_max_tool_calls, None);
        assert_eq!(parsed.scores.failure_cause, None);
        assert_eq!(parsed.input_contract, None);
    }

    #[test]
    fn failure_taxonomy_classifies_workspace_execution_and_limit_causes() {
        let changed = vec![DirDiffEntry {
            path: "src/lib.rs".into(),
            kind: "differs".into(),
        }];
        assert_eq!(
            classify_workspace_failure(&[], &changed),
            FailureCause::NoOp
        );
        assert_eq!(
            classify_workspace_failure(&changed, &changed),
            FailureCause::WrongChangedFile
        );
        assert_eq!(
            classify_workspace_failure(
                &changed,
                &[DirDiffEntry {
                    path: "new.rs".into(),
                    kind: "missing".into()
                }]
            ),
            FailureCause::MissingExpectedChange
        );
        assert_eq!(
            classify_workspace_failure(
                &changed,
                &[DirDiffEntry {
                    path: "scratch.txt".into(),
                    kind: "unexpected".into()
                }]
            ),
            FailureCause::UnexpectedExtraFile
        );
        assert_eq!(
            classify_workspace_failure(
                &changed,
                &[
                    DirDiffEntry {
                        path: "a".into(),
                        kind: "missing".into()
                    },
                    DirDiffEntry {
                        path: "b".into(),
                        kind: "unexpected".into()
                    },
                ]
            ),
            FailureCause::WrongDiff
        );
        assert_eq!(
            classify_runtime_error("OpenRouter API error (429)"),
            FailureCause::ProviderApi
        );
        assert_eq!(
            classify_runtime_error("connection reset by peer"),
            FailureCause::Transport
        );
        assert_eq!(
            classify_runtime_error("tool execution failed"),
            FailureCause::Tool
        );
        assert_eq!(
            classify_runtime_error("permission denied"),
            FailureCause::Permission
        );
        assert_eq!(
            FailureCause::from(LimitReason::MaxTurns),
            FailureCause::MaxTurns
        );
        assert_eq!(
            FailureCause::from(LimitReason::TokenBudget),
            FailureCause::TokenBudget
        );
        assert_eq!(
            FailureCause::from(LimitReason::DoomLoop),
            FailureCause::DoomLoop
        );
        assert_eq!(FailureCause::Timeout.label(), "timeout");
        assert_eq!(
            FailureCause::SemanticVerificationFailed.label(),
            "semantic_verification_failed"
        );
    }

    #[test]
    fn compact_trace_is_bounded_and_records_terminal_evidence() {
        let tools = (0..(TRACE_MAX_TOOL_EVENTS + 2))
            .map(|_| "read_file".to_string())
            .collect::<Vec<_>>();
        let trace = build_compact_trace(CompactTraceInput {
            assistant_turns: 3,
            tool_sequence: &tools,
            tool_failures: &[ToolFailureTrace {
                name: "edit_file".into(),
                detail: "Error: denied".into(),
            }],
            finish_reasons: &["tool_calls".into()],
            terminal_cause: Some(FailureCause::Permission),
            routed_models: &[
                RoutedModelObservation {
                    assistant_turn: 1,
                    model: "a/model".into(),
                },
                RoutedModelObservation {
                    assistant_turn: 2,
                    model: "b/model".into(),
                },
            ],
            upstream_providers: &[
                UpstreamProviderObservation {
                    assistant_turn: 1,
                    provider: "provider-a".into(),
                },
                UpstreamProviderObservation {
                    assistant_turn: 2,
                    provider: "provider-b".into(),
                },
            ],
            tool_trace_truncated: false,
        });
        assert_eq!(trace.assistant_turns, 3);
        assert_eq!(trace.tool_sequence.len(), TRACE_MAX_TOOL_EVENTS);
        assert_eq!(
            trace.tool_counts["read_file"],
            (TRACE_MAX_TOOL_EVENTS + 2) as u32
        );
        assert_eq!(trace.terminal_cause, Some(FailureCause::Permission));
        assert_eq!(
            trace.routed_model_change.as_deref(),
            Some("a/model -> b/model")
        );
        assert_eq!(
            trace.upstream_provider_change.as_deref(),
            Some("provider-a -> provider-b")
        );
        assert!(trace.truncated);
    }

    #[test]
    fn summary_marks_runs_that_switched_routed_models() {
        let mut result = result(0, 0, Some("openai/gpt-oss-120b"));
        result.routed_models = vec![
            RoutedModelObservation {
                assistant_turn: 1,
                model: "openai/gpt-oss-120b".into(),
            },
            RoutedModelObservation {
                assistant_turn: 2,
                model: "qwen/qwen3-coder".into(),
            },
        ];

        assert!(format_summary(&[result]).contains("mixed (2 models; final qwen/qwen3-coder)"));
    }

    #[test]
    fn summary_omits_repeated_requested_and_routed_model_columns() {
        let summary = format_summary(&[result(0, 0, None)]);

        assert!(!summary.contains("| model |"));
        assert!(!summary.contains("| routed |"));
        assert!(summary.contains("| task"));
        assert!(summary.contains("\nmodel: openrouter/auto\n1 passed"));
    }

    #[test]
    fn summaries_label_execution_errors_separately_from_scored_failures() {
        let mut result = result(0, 0, None);
        result.scores.outcome.passed = false;
        result.scores.error = Some("OpenRouter API error (429 Too Many Requests)".into());

        let summary = format_summary(std::slice::from_ref(&result));
        assert!(summary.contains("ERROR"));
        assert!(summary.contains("0 failed, 0 limited, 1 errored"));
        let aggregated = format_aggregated_summary(std::slice::from_ref(&result), 1);
        assert!(aggregated.contains("0/1 pass; 1 ERROR"));
    }

    #[test]
    fn summaries_classify_max_turns_as_limit_and_preserve_legacy_artifacts() {
        let mut result = result(0, 0, None);
        result.scores.outcome.passed = false;
        result.scores.error = Some("Max iterations (8) reached — possible infinite loop".into());

        assert_eq!(result_status(&result), ResultStatus::Limit);
        assert!(format_summary(std::slice::from_ref(&result)).contains("LIMIT"));
        assert!(format_summary(std::slice::from_ref(&result))
            .contains("0 failed, 1 limited, 0 errored"));
        assert!(format_aggregated_summary(std::slice::from_ref(&result), 1)
            .contains("0/1 pass; 1 LIMIT"));

        result.scores.error = None;
        result.scores.limit = Some(LimitReason::MaxTurns);
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["scores"]["limit"], "max_turns");
    }

    #[test]
    fn transcript_artifact_preserves_model_facing_messages() {
        let dir = tempfile::tempdir().unwrap();
        let mut result = result(0, 0, None);
        result.task_id = "task".into();
        result.prompt_id = "prompt".into();
        result.generation_ids = vec!["gen-123".into()];
        result.transcript = vec![
            Message::System(SystemMessage {
                content: "system instructions".into(),
            }),
            Message::User(UserMessage {
                content: "complete the task".into(),
            }),
        ];

        write_transcript(dir.path(), &result, None).unwrap();

        let lines: Vec<serde_json::Value> =
            std::fs::read_to_string(dir.path().join("transcripts/task__prompt.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        assert_eq!(lines[0]["type"], "eval_transcript");
        assert_eq!(lines[0]["task_id"], "task");
        assert_eq!(lines[0]["generation_ids"], json!(["gen-123"]));
        assert_eq!(lines[1]["role"], "system");
        assert_eq!(lines[2]["content"], "complete the task");
    }

    #[test]
    fn execution_errors_write_a_readable_error_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let mut result = result(0, 0, Some("nvidia/nemotron:free"));
        result.scores.outcome.passed = false;
        result.scores.error = Some("error decoding provider JSON response".into());

        write_result_artifacts(dir.path(), &result).unwrap();

        let log = std::fs::read_to_string(dir.path().join("errors/task-1__prompt-1.log")).unwrap();
        assert!(log.contains("routed_models: nvidia/nemotron:free"));
        assert!(log.contains("error decoding provider JSON response"));
        assert!(log.contains("result: ../task-1__prompt-1.json"));
    }

    #[test]
    fn summaries_include_cache_metrics_and_zeroes_when_unreported() {
        let results = vec![
            result(75, 25, Some("anthropic/claude-sonnet-4")),
            result(0, 0, None),
        ];
        let summary = format_summary(&results);
        assert!(summary.contains("cache r/w"));
        assert!(summary.contains("75/25"));
        assert!(summary.contains("0/0"));
        assert!(summary.contains("cache tokens: 75 read, 25 written"));
        assert!(summary.contains("totals: 200 prompt + 40 completion = 240 tokens, $0.020000"));

        let aggregated = format_aggregated_summary(&results, 2);
        assert!(aggregated.contains("cache read (avg)"));
        assert!(aggregated.contains("cache write (avg)"));
        assert!(aggregated.contains("38"));
    }

    #[test]
    fn multi_run_summary_excludes_smoke_preflight_from_metrics_and_totals() {
        let mut smoke = result(80, 40, None);
        smoke.task_id = "smoke-task".into();

        let mut first = result(10, 5, None);
        first.task_id = "matrix-task".into();
        first.run_index = 1;

        let mut second = result(20, 15, None);
        second.task_id = "matrix-task".into();
        second.run_index = 2;
        second.scores.efficiency.tool_calls = 20;
        second.scores.efficiency.max_tool_calls = Some(12);
        second.scores.efficiency.exceeded_max_tool_calls = Some(true);

        let summary = format_aggregated_summary(&[smoke, first, second], 2);

        assert!(!summary.contains("smoke-task"));
        assert!(summary.contains("matrix-task"));
        assert!(summary.contains("model: openrouter/auto"));
        assert!(
            summary.contains("2 passed (0 over budget), 0 failed, 0 limited, 0 errored of 2 total")
        );
        assert!(summary.contains("1 exceeded maximum tool-call guidance (not a failure)"));
        assert!(summary.contains("cache tokens: 30 read, 20 written"));
        assert!(summary.contains("totals: 200 prompt + 40 completion = 240 tokens, $0.020000"));
    }

    #[test]
    fn retried_provider_errors_preserve_attempt_cost_and_quality_outcome() {
        let mut final_result = result(0, 0, None);
        final_result.retry_attempts.push(RetryAttempt {
            attempt: 1,
            cause: FailureCause::ProviderApi,
            error: "error decoding provider JSON response".into(),
            duration_ms: 12,
            cost: CostScore {
                tokens_in: 50,
                tokens_out: 5,
                cached_tokens: 4,
                cache_write_tokens: 3,
                usd: 0.005,
            },
            provider_telemetry: Vec::new(),
            debug_errors: Vec::new(),
            call_telemetry: Vec::new(),
            retry_reason: None,
            retry_delay_ms: None,
        });

        let summary = format_summary(std::slice::from_ref(&final_result));
        assert!(summary.contains("PASS (1 RETRY)"));
        assert!(summary.contains("1 passed (0 over budget), 0 failed, 0 limited, 0 errored of 1 total; 1 retry (1 errored attempt)"));
        assert!(summary.contains("failure causes: provider_api=1"));
        assert!(summary.contains("cache tokens: 4 read, 3 written"));
        assert!(summary.contains("totals: 150 prompt + 25 completion = 175 tokens, $0.015000"));

        let aggregated = format_aggregated_summary(std::slice::from_ref(&final_result), 1);
        assert!(aggregated.contains("1/1 pass; 1 RETRY"));
        assert!(aggregated.contains("1 retry (1 errored attempt)"));
        assert!(aggregated.contains("totals: 150 prompt + 25 completion = 175 tokens, $0.015000"));
        assert!(format_failure_details(&[final_result])
            .contains("retries (1):\n  task-1 | prompt-1 | attempt 1 | provider_api"));
    }

    #[test]
    fn retry_attempt_artifacts_use_a_distinct_stem() {
        let dir = tempfile::tempdir().unwrap();
        let mut failed = result(0, 0, None);
        failed.scores.outcome.passed = false;
        failed.scores.failure_cause = Some(FailureCause::ProviderApi);
        failed.scores.error = Some("error decoding provider JSON response".into());

        write_retry_attempt_artifacts(dir.path(), &failed, 1).unwrap();

        assert!(dir.path().join("task-1__prompt-1__attempt1.json").exists());
        assert!(dir
            .path()
            .join("transcripts/task-1__prompt-1__attempt1.jsonl")
            .exists());
        assert!(dir
            .path()
            .join("errors/task-1__prompt-1__attempt1.log")
            .exists());
    }

    #[test]
    fn summary_surfaces_exceeded_tool_call_guidance_without_failing_result() {
        let mut result = result(0, 0, None);
        result.scores.efficiency.tool_calls = 20;
        result.scores.efficiency.max_tool_calls = Some(12);
        result.scores.efficiency.tool_calls_in_range = false;

        let summary = format_summary(std::slice::from_ref(&result));

        assert!(summary.contains("20 (max 12)"));
        assert!(summary.contains("1 exceeded maximum tool-call guidance (not a failure)"));
        assert!(summary.contains("1 passed (0 over budget)"));
    }

    #[test]
    fn run_meta_persists_cache_token_totals() {
        let dir = tempfile::tempdir().unwrap();
        let results = vec![result(75, 25, None), result(5, 10, None)];
        let prewarm = CachePrewarmRun {
            session_id: "heddle-eval-cache-test".into(),
            ttl: "provider_default".into(),
            prewarms: vec![PrewarmResult {
                prompt_id: "prompt-1".into(),
                duration_ms: 20,
                routed_model: Some("anthropic/claude-sonnet-4".into()),
                tokens_in: 50,
                tokens_out: 1,
                cached_tokens: 0,
                cache_write_tokens: 50,
            }],
        };
        write_run_artifacts(
            dir.path(),
            dir.path(),
            "openrouter/auto",
            &["prompt-1".into()],
            &["task-1".into()],
            1_000,
            500,
            4,
            60,
            1.0,
            &results,
            "summary",
            "",
            Some(&prewarm),
            &StaticContextSelection {
                enabled: true,
                excluded_prompts: vec![StaticContextExclusion {
                    prompt_id: "dynamic".into(),
                    dynamic_features: vec!["cwd".into(), "file_tree".into()],
                }],
            },
            ComparisonConfig {
                prompts: vec!["prompt-1".into()],
                prompt_conditions: vec![PromptCondition {
                    id: "prompt-1".into(),
                    role: "verification".into(),
                    hypothesis: "Checks changed behavior.".into(),
                }],
                tasks: vec!["task-1".into()],
                max_tokens_per_task: 1_000,
                max_tokens_per_response: 500,
                max_turns: 4,
                task_timeout_secs: 60,
                static_context_only: true,
                excluded_dynamic_prompts: vec!["dynamic".into()],
                cache_prewarm: true,
                cache_ttl: Some("provider_default".into()),
                openrouter_routing: "balanced".into(),
                condition: Some(ResolvedEvalCondition {
                    declaration: EvalCondition {
                        id: "range-read-v1".into(),
                        hypothesis: "Bounded reads reduce retrieval cost.".into(),
                        baseline: "full-read-v1".into(),
                        variant: "range-read-v1".into(),
                        changed_factor: "read_file_contract".into(),
                        expected_signal: "fewer input tokens".into(),
                    },
                    fingerprint: "condition-fingerprint".into(),
                }),
            },
            test_comparison_identity("comparison-controls-test"),
            SuiteIdentity {
                fingerprint: "suite-fingerprint".into(),
                source: "test".into(),
            },
            false,
            2,
            1,
            &RunTiming {
                started_at: "2026-08-07T00:00:00Z".into(),
                finished_at: "2026-08-07T00:01:05Z".into(),
                duration_ms: 65_000,
                matrix_runs: vec![MatrixRunTiming {
                    run_index: 1,
                    duration_ms: 60_000,
                }],
            },
            &GitInfo {
                commit: "heddle-test".into(),
                dirty: Some(false),
            },
            &GitInfo {
                commit: "evals-test".into(),
                dirty: Some(false),
            },
        )
        .unwrap();

        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("run_meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["cache_tokens"]["cached_tokens"], 80);
        assert_eq!(meta["cache_tokens"]["cache_write_tokens"], 35);
        assert_eq!(meta["totals"]["prompt_tokens"], 200);
        assert_eq!(meta["totals"]["completion_tokens"], 40);
        assert_eq!(meta["totals"]["total_tokens"], 240);
        assert_eq!(meta["totals"]["usd"], 0.02);
        assert_eq!(meta["heddle_commit"], "heddle-test");
        assert_eq!(meta["heddle_dirty"], false);
        assert_eq!(meta["evals_commit"], "evals-test");
        assert_eq!(meta["evals_dirty"], false);
        assert_eq!(
            meta["cache_prewarm"]["session_id"],
            "heddle-eval-cache-test"
        );
        assert_eq!(
            meta["cache_prewarm"]["prewarms"][0]["cache_write_tokens"],
            50
        );
        assert_eq!(meta["static_context_selection"]["enabled"], true);
        assert_eq!(
            meta["static_context_selection"]["excluded_prompts"][0]["dynamic_features"],
            json!(["cwd", "file_tree"])
        );
        assert_eq!(meta["comparison"]["static_context_only"], true);
        assert_eq!(meta["condition"]["id"], "range-read-v1");
        assert_eq!(meta["prompt_conditions"][0]["role"], "verification");
        assert_eq!(
            meta["comparison_identity"]["fingerprint"],
            "comparison-controls-test"
        );
        assert_eq!(meta["condition"]["fingerprint"], "condition-fingerprint");
        assert_eq!(
            meta["comparison"]["condition"]["fingerprint"],
            "condition-fingerprint"
        );
        assert_eq!(meta["suite"]["fingerprint"], "suite-fingerprint");
        assert_eq!(meta["duration_ms"], 65_000);
        assert_eq!(meta["matrix_runs"][0]["duration_ms"], 60_000);
        let summary_md = std::fs::read_to_string(dir.path().join("summary.md")).unwrap();
        assert!(summary_md.contains("wall_time: `1m05s`"));
        assert!(summary_md.contains("matrix_run_wall_times: run 1=1m00s"));
    }

    #[test]
    fn human_duration_format_is_compact() {
        assert_eq!(format_duration_ms(999), "0s");
        assert_eq!(format_duration_ms(65_000), "1m05s");
        assert_eq!(format_duration_ms(3_661_000), "1h01m01s");
    }

    #[test]
    fn aggregate_writes_portable_snapshot_and_excludes_budget_stops() {
        let dir = tempfile::tempdir().unwrap();
        let results_root = dir.path().join("results");
        let comparison = ComparisonConfig {
            prompts: vec!["prompt-1".into()],
            prompt_conditions: Vec::new(),
            tasks: vec!["task-1".into()],
            max_tokens_per_task: 1_000,
            max_tokens_per_response: 500,
            max_turns: 4,
            task_timeout_secs: 60,
            static_context_only: false,
            excluded_dynamic_prompts: Vec::new(),
            cache_prewarm: false,
            cache_ttl: None,
            openrouter_routing: "balanced".into(),
            condition: None,
        };
        let suite = SuiteIdentity {
            fingerprint: "suite-fingerprint-for-test".into(),
            source: "test".into(),
        };
        for (name, model, budget_stopped) in
            [("first", "model/a", false), ("stopped", "model/b", true)]
        {
            let run_dir = results_root.join(name);
            let mut results = vec![result(0, 0, None)];
            if !budget_stopped {
                results[0].scores.efficiency.tool_calls = 20;
                results[0].scores.efficiency.max_tool_calls = Some(12);
                results[0].scores.efficiency.exceeded_max_tool_calls = Some(true);
                results[0].scores.efficiency.tool_calls_in_range = false;
            }
            write_run_artifacts(
                &run_dir,
                dir.path(),
                model,
                &["prompt-1".into()],
                &["task-1".into()],
                1_000,
                500,
                4,
                60,
                1.0,
                &results,
                "summary",
                "",
                None,
                &StaticContextSelection {
                    enabled: false,
                    excluded_prompts: Vec::new(),
                },
                comparison.clone(),
                test_comparison_identity("comparison-controls-test"),
                suite.clone(),
                budget_stopped,
                if budget_stopped { 2 } else { 1 },
                1,
                &RunTiming {
                    started_at: "2026-08-07T00:00:00Z".into(),
                    finished_at: "2026-08-07T00:01:05Z".into(),
                    duration_ms: 65_000,
                    matrix_runs: vec![MatrixRunTiming {
                        run_index: 1,
                        duration_ms: 60_000,
                    }],
                },
                &GitInfo {
                    commit: "heddle-test".into(),
                    dirty: Some(false),
                },
                &GitInfo {
                    commit: "evals-test".into(),
                    dirty: Some(false),
                },
            )
            .unwrap();
            if budget_stopped {
                let summary_path = run_dir.join("summary.json");
                let mut summary: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
                let efficiency = summary[0]["scores"]["efficiency"].as_object_mut().unwrap();
                efficiency.remove("max_tool_calls");
                efficiency.remove("exceeded_max_tool_calls");
                fs::write(
                    &summary_path,
                    serde_json::to_string_pretty(&summary).unwrap(),
                )
                .unwrap();
            }
        }

        cmd_aggregate(
            &results_root,
            &[],
            "fixture-suite",
            "quality",
            &dir.path().join("aggregates"),
            Some(&dir.path().join("aggregate-output")),
        )
        .unwrap();

        let snapshot_path = WalkDir::new(dir.path().join("aggregate-output"))
            .into_iter()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.into_path())
            .find(|path| path.file_name().is_some_and(|name| name == "runs.json"))
            .expect("aggregate runs.json");
        let snapshot: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&snapshot_path).unwrap()).unwrap();
        assert_eq!(snapshot["suite"]["label"], "fixture-suite");
        assert_eq!(snapshot["profile"]["label"], "quality");
        assert_eq!(
            snapshot["runs"][0]["results"][0]["scores"]["efficiency"]["max_tool_calls"],
            12
        );
        assert_eq!(
            snapshot["runs"][0]["results"][0]["scores"]["efficiency"]["exceeded_max_tool_calls"],
            true
        );
        assert_eq!(snapshot["runs"].as_array().unwrap().len(), 2);
        assert_eq!(
            snapshot["runs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|run| run["included_in_quality_metrics"] == true)
                .count(),
            1
        );
        let aggregate_dir = snapshot_path.parent().unwrap();
        assert!(aggregate_dir.join("profile.json").exists());
        assert!(aggregate_dir.join("by-model.md").exists());
        assert!(aggregate_dir.join("by-prompt.md").exists());
        assert!(aggregate_dir.join("by-heddle-revision.md").exists());
        let by_model = fs::read_to_string(aggregate_dir.join("by-model.md")).unwrap();
        assert!(by_model.contains("tools avg"));
        assert!(by_model.contains("max tools"));
        assert!(by_model.contains("over max"));
        assert!(by_model.contains("20.0"));
        assert!(by_model.contains("1/1"));
    }

    #[test]
    fn cache_mode_separates_stable_prompt_body_from_dynamic_context() {
        let dir = tempfile::tempdir().unwrap();
        let prompt = Prompt {
            id: "dynamic".into(),
            front: PromptFrontMatter {
                context: ContextConfig {
                    cwd: true,
                    ..ContextConfig::default()
                },
                ..PromptFrontMatter::default()
            },
            body: "Follow the project instructions.".into(),
        };

        let messages = compose_messages(&prompt, dir.path(), true);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[0],
            Message::System(SystemMessage { content }) if content == "Follow the project instructions."
        ));
        assert!(matches!(
            &messages[1],
            Message::System(SystemMessage { content }) if content.contains("Current Working Directory")
        ));
    }

    #[test]
    fn cache_mode_rejects_unstable_models_and_invalid_ttl() {
        assert!(validate_cache_model("openrouter/free", false).is_err());
        assert!(validate_cache_model("openrouter/auto", false).is_err());
        assert!(validate_cache_model("example/model:free", false).is_err());
        assert!(validate_cache_model("openai/gpt-5", false).is_err());
        assert!(validate_cache_model("anthropic/claude-sonnet-4", true).is_ok());
    }

    #[test]
    fn static_context_only_excludes_dynamic_prompts_from_matrix_selection() {
        let static_prompt = Prompt {
            id: "static".into(),
            front: PromptFrontMatter::default(),
            body: "instructions".into(),
        };
        let dynamic_prompt = Prompt {
            id: "dynamic".into(),
            front: PromptFrontMatter {
                context: ContextConfig {
                    cwd: true,
                    date: true,
                    ..ContextConfig::default()
                },
                ..PromptFrontMatter::default()
            },
            body: "instructions".into(),
        };
        let mut selected = vec![&static_prompt, &dynamic_prompt];

        let selection = apply_static_context_selection(&mut selected, "all", true).unwrap();

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "static");
        assert_eq!(selection.excluded_prompts.len(), 1);
        assert_eq!(selection.excluded_prompts[0].prompt_id, "dynamic");
        assert_eq!(
            selection.excluded_prompts[0].dynamic_features,
            vec!["cwd".to_string(), "date".to_string()]
        );
    }

    #[test]
    fn static_context_only_rejects_explicit_dynamic_prompt_selection() {
        let dynamic_prompt = Prompt {
            id: "dynamic".into(),
            front: PromptFrontMatter {
                context: ContextConfig {
                    git: Some(GitConfig {
                        branch: true,
                        status: false,
                    }),
                    ..ContextConfig::default()
                },
                ..PromptFrontMatter::default()
            },
            body: "instructions".into(),
        };
        let mut selected = vec![&dynamic_prompt];

        let error = apply_static_context_selection(&mut selected, "dynamic", true).unwrap_err();

        assert!(error.to_string().contains("dynamic (git)"));
    }

    #[test]
    fn task_tag_selection_filters_selected_tasks() {
        let task = |id: &str, tags: &[&str]| Task {
            dir: PathBuf::from(id),
            spec: TaskSpec {
                id: id.into(),
                prompt: String::new(),
                tags: tags.iter().map(|tag| (*tag).into()).collect(),
                tools: None,
                max_turns: None,
                task_timeout_secs: None,
                budget_tokens: None,
                smoke: false,
                score: TaskScoreSpec {
                    outcome: OutcomeSpec {
                        expected_dir: Some("after".into()),
                        ignore_globs: None,
                    },
                    semantic_verification: None,
                    efficiency: None,
                },
            },
        };
        let rename = task("rename", &["multi_file", "rename"]);
        let bugfix = task("bugfix", &["bugfix"]);

        let selected = select_task_tags(vec![&rename, &bugfix], "rename,bugfix").unwrap();
        assert_eq!(selected.len(), 2);
        assert!(select_task_tags(vec![&rename], "missing").is_err());
    }

    #[test]
    fn task_turn_limit_overrides_the_cli_fallback() {
        assert_eq!(task_max_turns(Some(12), 8), 12);
        assert_eq!(task_max_turns(None, 8), 8);
    }

    #[test]
    fn protected_path_diffs_only_reports_configured_paths() {
        let dir = tempfile::tempdir().unwrap();
        let before = dir.path().join("before");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&before).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(before.join("Cargo.toml"), "original").unwrap();
        fs::write(before.join("src.rs"), "original").unwrap();
        fs::write(workspace.join("Cargo.toml"), "changed").unwrap();
        fs::write(workspace.join("src.rs"), "allowed").unwrap();

        let diffs = protected_path_diffs(
            &before,
            &workspace,
            &["Cargo.toml".into(), "config/**".into()],
        )
        .unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "Cargo.toml");
    }

    #[tokio::test]
    async fn semantic_verifier_is_staged_after_the_agent_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir.path().join("task");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(task_dir.join("verify")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("api.txt"), "implemented").unwrap();
        fs::write(
            task_dir.join("verify/check.py"),
            "from pathlib import Path\nassert Path('api.txt').read_text() == 'implemented'\n",
        )
        .unwrap();
        let task = Task {
            dir: task_dir,
            spec: TaskSpec {
                id: "semantic".into(),
                prompt: "implement".into(),
                tags: vec!["semantic-verification".into()],
                tools: None,
                max_turns: None,
                task_timeout_secs: None,
                budget_tokens: None,
                smoke: false,
                score: TaskScoreSpec {
                    outcome: OutcomeSpec {
                        expected_dir: None,
                        ignore_globs: None,
                    },
                    semantic_verification: Some(SemanticVerificationSpec {
                        command: vec!["python3".into(), ".heddle-verification/check.py".into()],
                        timeout_secs: 5,
                        max_output_bytes: 128,
                        protected_globs: Vec::new(),
                    }),
                    efficiency: None,
                },
            },
        };

        let result = run_semantic_verification(
            &task,
            &workspace,
            task.spec.score.semantic_verification.as_ref().unwrap(),
        )
        .await
        .unwrap();
        assert!(result.passed);
        assert!(workspace.join(".heddle-verification/check.py").exists());
    }

    #[tokio::test]
    async fn semantic_verifier_records_a_bounded_failure_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir.path().join("task");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(task_dir.join("verify")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            task_dir.join("verify/check.py"),
            "raise AssertionError('expected public API behavior was missing')\n",
        )
        .unwrap();
        let task = Task {
            dir: task_dir,
            spec: TaskSpec {
                id: "semantic-failure".into(),
                prompt: "implement".into(),
                tags: Vec::new(),
                tools: None,
                max_turns: None,
                task_timeout_secs: None,
                budget_tokens: None,
                smoke: false,
                score: TaskScoreSpec {
                    outcome: OutcomeSpec {
                        expected_dir: None,
                        ignore_globs: None,
                    },
                    semantic_verification: Some(SemanticVerificationSpec {
                        command: vec!["python3".into(), ".heddle-verification/check.py".into()],
                        timeout_secs: 5,
                        max_output_bytes: 32,
                        protected_globs: Vec::new(),
                    }),
                    efficiency: None,
                },
            },
        };

        let result = run_semantic_verification(
            &task,
            &workspace,
            task.spec.score.semantic_verification.as_ref().unwrap(),
        )
        .await
        .unwrap();
        assert!(!result.passed);
        assert_eq!(result.exit_code, Some(1));
        assert!(result.output.contains("Traceback"));
        assert!(result.output.len() <= 35);
        assert!(result.output.ends_with('…'));
    }

    #[test]
    fn suite_fingerprint_ignores_task_tags_but_tracks_behavioral_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir.path().join("task");
        std::fs::create_dir_all(task_dir.join("before")).unwrap();
        std::fs::create_dir_all(task_dir.join("after")).unwrap();
        std::fs::write(task_dir.join("before/input.txt"), "before").unwrap();
        std::fs::write(task_dir.join("after/input.txt"), "after").unwrap();
        let prompt = Prompt {
            id: "default".into(),
            front: PromptFrontMatter::default(),
            body: "Do the task.".into(),
        };
        let mut task = Task {
            dir: task_dir,
            spec: TaskSpec {
                id: "task".into(),
                prompt: "Edit input.txt".into(),
                tags: vec!["simple-edit".into()],
                tools: None,
                max_turns: None,
                task_timeout_secs: None,
                budget_tokens: None,
                smoke: false,
                score: TaskScoreSpec {
                    outcome: OutcomeSpec {
                        expected_dir: Some("after".into()),
                        ignore_globs: None,
                    },
                    semantic_verification: None,
                    efficiency: None,
                },
            },
        };

        let original = suite_identity(&[&prompt], &[&task]).unwrap();
        task.spec.tags.push("bugfix".into());
        assert_eq!(
            original.fingerprint,
            suite_identity(&[&prompt], &[&task]).unwrap().fingerprint
        );

        task.spec.prompt.push_str(" Do not touch anything else.");
        assert_ne!(
            original.fingerprint,
            suite_identity(&[&prompt], &[&task]).unwrap().fingerprint
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    evals: &Path,
    prompts: &str,
    tasks: &str,
    tags: &str,
    model_flag: Option<&str>,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    task_timeout_secs: u64,
    budget_stop_usd_flag: Option<f64>,
    results_dir: Option<PathBuf>,
    tag: Option<&str>,
    suite_label_flag: Option<&str>,
    runs: u32,
    record_all_text: bool,
    cache_prewarm: bool,
    cache_ttl_1h: bool,
    static_context_only: bool,
    condition_path: Option<&Path>,
) -> Result<()> {
    let invocation_started_at = Utc::now().to_rfc3339();
    let invocation_start = Instant::now();
    let manifest = load_manifest(evals)?;
    let condition = condition_path.map(load_eval_condition).transpose()?;
    let model = model_flag
        .map(|s| s.to_string())
        .or_else(|| manifest.default_model.clone())
        .unwrap_or_else(|| "openrouter/free".into());
    let budget_stop_usd = budget_stop_usd_flag
        .or(manifest.defaults.budget_stop_usd)
        .unwrap_or(1.0);

    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow!("OPENROUTER_API_KEY not set (looked in env, .env.local, .env)"))?;

    let all_prompts = load_prompts(evals)?;
    let all_tasks = load_tasks(evals)?;
    let mut chosen_prompts = select(&all_prompts, prompts, |p| p.id.as_str())?;
    let chosen_tasks = select_task_tags(select(&all_tasks, tasks, |t| t.spec.id.as_str())?, tags)?;

    // When the user said `--prompts all`, drop prompts marked
    // `matrix_exclude` (retired-but-kept, known-failing baselines, etc).
    // Explicit `--prompts <list>` still includes them so they can be
    // re-tested intentionally.
    if prompts == "all" {
        let before = chosen_prompts.len();
        chosen_prompts.retain(|p| !p.front.matrix_exclude);
        let excluded = before - chosen_prompts.len();
        if excluded > 0 {
            println!("(excluded {excluded} prompt(s) marked matrix_exclude)");
        }
    }

    let static_context =
        apply_static_context_selection(&mut chosen_prompts, prompts, static_context_only)?;
    if !static_context.excluded_prompts.is_empty() {
        println!(
            "(excluded {} prompt(s) with dynamic context)",
            static_context.excluded_prompts.len()
        );
        for excluded in &static_context.excluded_prompts {
            println!(
                "  - {} ({})",
                excluded.prompt_id,
                excluded.dynamic_features.join(", ")
            );
        }
    }

    if chosen_prompts.is_empty() || chosen_tasks.is_empty() {
        bail!("nothing to run (no prompts or no tasks selected)");
    }
    // Snapshot provenance before prewarming, smoke checks, or any provider
    // work starts. A long-running eval must describe the code it began with,
    // not edits made while it was still running.
    let start_heddle_git = heddle_git_info();
    let start_evals_git = git_info(evals);

    if cache_ttl_1h && !cache_prewarm {
        bail!("--cache-ttl-1h requires --cache-prewarm");
    }
    let cache = if cache_prewarm {
        validate_cache_model(&model, cache_ttl_1h)?;
        Some(CachePrewarmConfig {
            session_id: format!("heddle-eval-cache-{}", Utc::now().format("%Y%m%dT%H%M%S%f")),
            ttl_1h: cache_ttl_1h,
        })
    } else {
        None
    };

    // Build the (task, prompt) pairs. Smoke tasks only run against the
    // `default` prompt when matrix mode (>1 chosen prompt), so they don't
    // pollute the comparison data. When user explicitly selects a single
    // prompt, smoke tasks run normally.
    let is_matrix = chosen_prompts.len() > 1;
    let default_prompt = chosen_prompts
        .iter()
        .find(|p| p.id == "default")
        .copied()
        .or_else(|| chosen_prompts.first().copied());
    // Smoke pairs first — if any smoke task fails we abort before burning
    // budget on the matrix. Non-smoke pairs are run after smoke passes.
    let mut smoke_pairs: Vec<(&Task, &Prompt)> = Vec::new();
    let mut matrix_pairs: Vec<(&Task, &Prompt)> = Vec::new();
    for task in &chosen_tasks {
        if task.spec.smoke {
            if let Some(p) = default_prompt {
                if is_matrix {
                    smoke_pairs.push((task, p));
                } else {
                    // Single-prompt run — smoke still goes through default
                    // (or whatever the user's single chosen prompt was).
                    for prompt in &chosen_prompts {
                        smoke_pairs.push((task, prompt));
                    }
                }
            }
        } else {
            for prompt in &chosen_prompts {
                matrix_pairs.push((task, prompt));
            }
        }
    }
    let smoke_count = chosen_tasks.iter().filter(|t| t.spec.smoke).count();

    let suite = suite_identity(&chosen_prompts, &chosen_tasks)?;
    let ts = Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let (results_dir, suite_root_name) = match results_dir {
        Some(path) => (path, None),
        None => {
            let is_canonical_full_suite =
                prompts == "all" && tasks == "all" && tags == "all" && !static_context_only;
            let uses_manifest_label = is_canonical_full_suite || tags == "smoke";
            let requested_label = suite_label_flag.or_else(|| {
                uses_manifest_label
                    .then_some(manifest.suite_label.as_deref())
                    .flatten()
            });
            let suite_root = resolve_suite_root(
                evals,
                &suite,
                requested_label,
                is_canonical_full_suite && suite_label_flag.is_none(),
            )?;
            let suite_root_name = suite_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            (
                suite_root
                    .join(model_result_dir_name(&model))
                    .join(default_result_dir_name(&ts, &model, tag)),
                suite_root_name,
            )
        }
    };
    let suite_provenance = format!(
        "{} (fingerprint: {}; source: {})",
        suite_root_name
            .as_deref()
            .unwrap_or("<explicit results-dir>"),
        suite.fingerprint,
        suite.source,
    );
    let total_pairs = smoke_pairs.len() + matrix_pairs.len();
    println!("Running {total_pairs} (task, prompt) pairs against {model}");
    println!("Suite -> {suite_provenance}");
    let cache_prewarm_run = if let Some(cache) = &cache {
        println!(
            "Prewarming {} stable prompt prefix(es) with session {}",
            chosen_prompts.len(),
            cache.session_id
        );
        let mut prewarms = Vec::with_capacity(chosen_prompts.len());
        for prompt in &chosen_prompts {
            let result = prewarm_prompt(prompt, &model, &api_key, cache).await?;
            println!(
                "  prewarm {}: {}/{} tokens, cache {}/{} r/w, {}ms",
                result.prompt_id,
                result.tokens_in,
                result.tokens_out,
                result.cached_tokens,
                result.cache_write_tokens,
                result.duration_ms,
            );
            prewarms.push(result);
        }
        Some(CachePrewarmRun {
            session_id: cache.session_id.clone(),
            ttl: if cache.ttl_1h {
                "1h"
            } else {
                "provider_default"
            }
            .to_string(),
            prewarms,
        })
    } else {
        None
    };
    if is_matrix && smoke_count > 0 {
        println!(
            "  ({} smoke run(s) up front; {} matrix run(s) after — matrix aborts if any smoke fails)",
            smoke_pairs.len(),
            matrix_pairs.len()
        );
    }
    println!("Saving -> {}", compact_results_location(&results_dir));

    let mut results: Vec<TaskResult> = Vec::new();
    let mut smoke_failed = false;
    let mut budget_stopped = false;
    let mut cumulative_usd = 0.0f64;
    let mut matrix_runs = Vec::new();
    let run_options = RunOneOptions {
        max_turns,
        max_tokens_per_task,
        max_tokens_per_response,
        task_timeout_secs,
        record_all_text,
        cache,
    };

    // Pass 1: smoke
    let smoke_total = smoke_pairs.len();
    for (i, (task, prompt)) in smoke_pairs.iter().enumerate() {
        let idx = i + 1;
        println!(
            "[smoke {idx}/{smoke_total}] {} | {}",
            task.spec.id, prompt.id
        );
        let (r, retry_attempt) =
            run_with_provider_retry(task, prompt, &model, &api_key, &run_options).await;
        if let Some(retry_attempt) = retry_attempt.as_ref() {
            write_retry_attempt_artifacts(&results_dir, retry_attempt, 1)?;
        }
        let outcome = outcome_label(&r);
        println!(
            "      {outcome} (tools={}, turns={}, tokens={}/{}, cache={}/{} r/w, usd=${:.6}, {}ms)",
            r.scores.efficiency.tool_calls,
            r.scores.efficiency.turns,
            r.scores.cost.tokens_in,
            r.scores.cost.tokens_out,
            r.scores.cost.cached_tokens,
            r.scores.cost.cache_write_tokens,
            r.scores.cost.usd,
            r.duration_ms,
        );
        if !r.scores.outcome.passed {
            smoke_failed = true;
        }
        cumulative_usd += attempt_cost(&r).usd;
        if cumulative_usd > budget_stop_usd {
            budget_stopped = true;
        }
        results.push(r);
        if budget_stopped {
            eprintln!(
                "budget_stop_usd exceeded (${cumulative_usd:.4} > ${budget_stop_usd:.4}); aborting remaining runs."
            );
            break;
        }
    }

    if smoke_failed && !matrix_pairs.is_empty() {
        eprintln!();
        eprintln!(
            "❌ smoke failed — aborting before {} matrix run(s) to save budget.",
            matrix_pairs.len()
        );
        eprintln!("Investigate the smoke failures above before re-running the matrix.");
        eprintln!();

        let summary = if runs > 1 {
            format_aggregated_summary(&results, runs)
        } else {
            format_summary(&results)
        };
        let failures = format_failure_details(&results);
        print!("{summary}");
        print!("{failures}");
        println!(
            "Smoke failed before matrix execution; not writing results under {}",
            results_dir.display()
        );
        return Ok(());
    }

    for r in &results {
        write_result_artifacts(&results_dir, r)?;
    }

    if budget_stopped {
        eprintln!();
        eprintln!(
            "Budget stop hit before {} pending matrix run(s).",
            matrix_pairs.len() * runs as usize
        );
        eprintln!();
    } else {
        // Pass 2: matrix, repeated `runs` times to average out variance.
        let matrix_total = matrix_pairs.len();
        for run_n in 1..=runs {
            let matrix_run_start = Instant::now();
            if runs > 1 {
                println!();
                println!("=== run {run_n}/{runs} ===");
            }
            for (i, (task, prompt)) in matrix_pairs.iter().enumerate() {
                let idx = i + 1;
                let prefix = if runs > 1 {
                    format!("[run {run_n}/{runs}, matrix {idx}/{matrix_total}]")
                } else {
                    format!("[matrix {idx}/{matrix_total}]")
                };
                println!("{prefix} {} | {}", task.spec.id, prompt.id);
                let (mut r, mut retry_attempt) =
                    run_with_provider_retry(task, prompt, &model, &api_key, &run_options).await;
                if runs > 1 {
                    r.run_index = run_n;
                    if let Some(attempt) = retry_attempt.as_mut() {
                        attempt.run_index = run_n;
                    }
                }
                if let Some(retry_attempt) = retry_attempt.as_ref() {
                    write_retry_attempt_artifacts(&results_dir, retry_attempt, 1)?;
                }
                let outcome = outcome_label(&r);
                println!(
                    "      {outcome} (tools={}, turns={}, tokens={}/{}, cache={}/{} r/w, usd=${:.6}, {}ms)",
                    r.scores.efficiency.tool_calls,
                    r.scores.efficiency.turns,
                    r.scores.cost.tokens_in,
                    r.scores.cost.tokens_out,
                    r.scores.cost.cached_tokens,
                    r.scores.cost.cache_write_tokens,
                    r.scores.cost.usd,
                    r.duration_ms,
                );
                write_result_artifacts(&results_dir, &r)?;
                cumulative_usd += attempt_cost(&r).usd;
                if cumulative_usd > budget_stop_usd {
                    budget_stopped = true;
                }
                results.push(r);
                if budget_stopped {
                    eprintln!(
                        "budget_stop_usd exceeded (${cumulative_usd:.4} > ${budget_stop_usd:.4}); aborting remaining runs."
                    );
                    break;
                }
            }
            if budget_stopped {
                matrix_runs.push(MatrixRunTiming {
                    run_index: run_n,
                    duration_ms: matrix_run_start.elapsed().as_millis(),
                });
                break;
            }
            matrix_runs.push(MatrixRunTiming {
                run_index: run_n,
                duration_ms: matrix_run_start.elapsed().as_millis(),
            });
        }
    }
    let summary = if runs > 1 {
        format_aggregated_summary(&results, runs)
    } else {
        format_summary(&results)
    };
    let failures = format_failure_details(&results);
    print!("{summary}");
    print!("{failures}");
    let run_timing = RunTiming {
        started_at: invocation_started_at,
        finished_at: Utc::now().to_rfc3339(),
        duration_ms: invocation_start.elapsed().as_millis(),
        matrix_runs,
    };
    println!("wall time: {}", format_duration_ms(run_timing.duration_ms));

    let comparison = comparison_config(
        &chosen_prompts,
        &chosen_tasks,
        ComparisonSettings {
            max_tokens_per_task,
            max_tokens_per_response,
            max_turns,
            task_timeout_secs,
            static_context_selection: &static_context,
            cache_prewarm: cache_prewarm_run.as_ref(),
            openrouter_routing: "balanced",
            condition: condition.as_ref(),
        },
    );
    let comparison_identity = comparison_identity(&model, &chosen_tasks, &comparison, runs)?;
    write_run_artifacts(
        &results_dir,
        evals,
        &model,
        &chosen_prompts
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>(),
        &chosen_tasks
            .iter()
            .map(|t| t.spec.id.clone())
            .collect::<Vec<_>>(),
        max_tokens_per_task,
        max_tokens_per_response,
        max_turns,
        task_timeout_secs,
        budget_stop_usd,
        &results,
        &summary,
        &failures,
        cache_prewarm_run.as_ref(),
        &static_context,
        comparison,
        comparison_identity,
        suite,
        budget_stopped,
        // Smoke checks are one-time preflight cases; only matrix cases repeat.
        smoke_pairs.len() + matrix_pairs.len() * runs as usize,
        runs,
        &run_timing,
        &start_heddle_git,
        &start_evals_git,
    )?;
    println!("Saved -> {}", compact_results_location(&results_dir));
    println!("Suite -> {suite_provenance}");
    Ok(())
}

#[derive(Debug, Serialize)]
struct RunMeta {
    timestamp: String,
    started_at: String,
    finished_at: String,
    duration_ms: u128,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    matrix_runs: Vec<MatrixRunTiming>,
    heddle_commit: String,
    heddle_dirty: Option<bool>,
    evals_commit: String,
    evals_dirty: Option<bool>,
    evals_version: String,
    model: String,
    openrouter_routing: String,
    runs_per_case: u32,
    prompts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prompt_conditions: Vec<PromptCondition>,
    tasks: Vec<String>,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    task_timeout_secs: u64,
    budget_stop_usd: f64,
    free_model: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_pacing_ms: Option<u64>,
    counts: RunCounts,
    totals: RunTotals,
    cache_tokens: CacheTokenTotals,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_prewarm: Option<CachePrewarmRun>,
    static_context_selection: StaticContextSelection,
    comparison: ComparisonConfig,
    comparison_identity: ComparisonIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<ResolvedEvalCondition>,
    suite: SuiteIdentity,
    budget_stopped: bool,
    planned_results_version: u8,
    planned_results: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MatrixRunTiming {
    run_index: u32,
    duration_ms: u128,
}

#[derive(Debug, Clone)]
struct RunTiming {
    started_at: String,
    finished_at: String,
    duration_ms: u128,
    matrix_runs: Vec<MatrixRunTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComparisonConfig {
    prompts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prompt_conditions: Vec<PromptCondition>,
    tasks: Vec<String>,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    task_timeout_secs: u64,
    static_context_only: bool,
    excluded_dynamic_prompts: Vec<String>,
    cache_prewarm: bool,
    cache_ttl: Option<String>,
    openrouter_routing: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    condition: Option<ResolvedEvalCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PromptCondition {
    id: String,
    role: String,
    hypothesis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SuiteIdentity {
    fingerprint: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComparisonIdentity {
    fingerprint: String,
    source: String,
}

const SUITE_METADATA_FILE: &str = "suite.json";

#[derive(Debug, Serialize, Deserialize)]
struct SuiteDirectoryMetadata {
    schema_version: u32,
    label: String,
    fingerprint: String,
    fingerprint_source: String,
}

#[derive(Debug, Deserialize)]
struct StoredSuiteIdentity {
    #[serde(default)]
    suite: Option<SuiteIdentity>,
}

fn suite_directory_name(label: &str, fingerprint: &str) -> String {
    format!(
        "{}__s-{}",
        result_name_component(label, "suite"),
        short_hash(fingerprint)
    )
}

fn short_hash(value: &str) -> String {
    value.chars().take(8).collect()
}

fn suite_directory_metadata(label: &str, suite: &SuiteIdentity) -> SuiteDirectoryMetadata {
    SuiteDirectoryMetadata {
        schema_version: 1,
        label: label.to_string(),
        fingerprint: suite.fingerprint.clone(),
        fingerprint_source: suite.source.clone(),
    }
}

fn suite_from_root(root: &Path) -> Result<Option<SuiteIdentity>> {
    let metadata_path = root.join(SUITE_METADATA_FILE);
    if metadata_path.is_file() {
        let metadata: SuiteDirectoryMetadata = serde_json::from_str(
            &fs::read_to_string(&metadata_path)
                .with_context(|| format!("reading {}", metadata_path.display()))?,
        )
        .with_context(|| format!("parsing {}", metadata_path.display()))?;
        return Ok(Some(SuiteIdentity {
            fingerprint: metadata.fingerprint,
            source: metadata.fingerprint_source,
        }));
    }

    // Roots migrated before suite.json existed can establish their identity
    // from one immutable run artifact, then receive suite.json on reuse.
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "run_meta.json" {
            let stored: StoredSuiteIdentity = serde_json::from_str(
                &fs::read_to_string(entry.path())
                    .with_context(|| format!("reading {}", entry.path().display()))?,
            )
            .with_context(|| format!("parsing {}", entry.path().display()))?;
            return Ok(stored.suite);
        }
    }
    Ok(None)
}

fn resolve_suite_root(
    evals: &Path,
    suite: &SuiteIdentity,
    requested_label: Option<&str>,
    require_new_family_label: bool,
) -> Result<PathBuf> {
    let results_link = evals.join("results");
    let link_metadata = fs::symlink_metadata(&results_link)
        .with_context(|| format!("reading results link {}", results_link.display()))?;
    if !link_metadata.file_type().is_symlink() {
        bail!(
            "{} must be a symlink to the canonical eval results repository; use --results-dir for an explicit temporary output path",
            results_link.display()
        );
    }
    let results_root = fs::canonicalize(&results_link).with_context(|| {
        format!(
            "resolving {} (expected a live canonical results repository)",
            results_link.display()
        )
    })?;
    if !results_root.join(".git").exists() {
        bail!(
            "{} does not resolve to a git results repository (missing .git)",
            results_link.display()
        );
    }

    let mut matches = Vec::new();
    for entry in fs::read_dir(&results_root)
        .with_context(|| format!("reading {}", results_root.display()))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() || !entry.file_name().to_string_lossy().contains("__s-") {
            continue;
        }
        if suite_from_root(&path)?.is_some_and(|existing| existing.fingerprint == suite.fingerprint)
        {
            matches.push(path);
        }
    }
    match matches.len() {
        1 => {
            let root = matches.pop().expect("one suite root");
            let metadata_path = root.join(SUITE_METADATA_FILE);
            if !metadata_path.exists() {
                let label = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.split_once("__s-").map(|(label, _)| label))
                    .unwrap_or("suite");
                fs::write(
                    &metadata_path,
                    serde_json::to_string_pretty(&suite_directory_metadata(label, suite))?,
                )?;
            }
            Ok(root)
        }
        0 => {
            let label = requested_label.ok_or_else(|| {
                anyhow!(
                    "no retained suite matches fingerprint {}. Re-run with --suite-label <name> to create <name>__s-{}",
                    suite.fingerprint,
                    short_hash(&suite.fingerprint)
                )
            })?;
            let label_component = result_name_component(label, "suite");
            if require_new_family_label
                && fs::read_dir(&results_root)?
                    .flatten()
                    .any(|entry| {
                        entry.path().is_dir()
                            && entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with(&format!("{label_component}__s-"))
                    })
            {
                bail!(
                    "manifest suite_label {label:?} already has retained suite roots but none matches fingerprint {}; bump suite_label (for example, a new base-evals version) before creating a new full-suite root",
                    suite.fingerprint
                );
            }
            let root = results_root.join(suite_directory_name(label, &suite.fingerprint));
            if root.exists() {
                bail!(
                    "suite directory {} already exists but does not match fingerprint {}",
                    root.display(),
                    suite.fingerprint
                );
            }
            fs::create_dir_all(&root)
                .with_context(|| format!("creating suite root {}", root.display()))?;
            fs::write(
                root.join(SUITE_METADATA_FILE),
                serde_json::to_string_pretty(&suite_directory_metadata(label, suite))?,
            )?;
            Ok(root)
        }
        _ => bail!(
            "multiple retained suite directories match fingerprint {}; resolve the duplicate roots before running",
            suite.fingerprint
        ),
    }
}

fn hash_file(hasher: &mut Sha256, label: &str, path: &Path) -> Result<()> {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update(fs::read(path).with_context(|| format!("reading {}", path.display()))?);
    hasher.update([0]);
    Ok(())
}

fn hash_value<T: Serialize>(hasher: &mut Sha256, label: &str, value: &T) -> Result<()> {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(value)?);
    hasher.update([0]);
    Ok(())
}

fn hash_fixture_tree(hasher: &mut Sha256, label: &str, root: &Path) -> Result<()> {
    if !root.is_dir() {
        bail!("fixture directory does not exist: {}", root.display());
    }
    let mut files: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    files.sort();
    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        hash_file(hasher, &format!("{label}/{relative}"), &path)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct SuitePrompt<'a> {
    id: &'a str,
    context: &'a ContextConfig,
    body: &'a str,
}

#[derive(Serialize)]
struct SuiteTask<'a> {
    id: &'a str,
    prompt: &'a str,
    tools: &'a Option<Vec<String>>,
    max_turns: Option<u32>,
    task_timeout_secs: Option<u64>,
    budget_tokens: Option<u64>,
    smoke: bool,
    score: &'a TaskScoreSpec,
}

fn suite_identity(prompts: &[&Prompt], tasks: &[&Task]) -> Result<SuiteIdentity> {
    let mut hasher = Sha256::new();
    let mut sorted_prompts: Vec<&Prompt> = prompts.to_vec();
    sorted_prompts.sort_by(|a, b| a.id.cmp(&b.id));
    for prompt in sorted_prompts {
        hash_value(
            &mut hasher,
            &format!("prompt/{}", prompt.id),
            &SuitePrompt {
                id: &prompt.id,
                context: &prompt.front.context,
                body: &prompt.body,
            },
        )?;
    }

    let mut sorted_tasks: Vec<&Task> = tasks.to_vec();
    sorted_tasks.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));
    for task in sorted_tasks {
        let config = SuiteTask {
            id: &task.spec.id,
            prompt: &task.spec.prompt,
            tools: &task.spec.tools,
            max_turns: task.spec.max_turns,
            task_timeout_secs: task.spec.task_timeout_secs,
            budget_tokens: task.spec.budget_tokens,
            smoke: task.spec.smoke,
            score: &task.spec.score,
        };
        hash_value(&mut hasher, &format!("task/{}", task.spec.id), &config)?;
        hash_fixture_tree(
            &mut hasher,
            &format!("task/{}/before", task.spec.id),
            &task.dir.join("before"),
        )?;
        if let Some(expected_dir) = &task.spec.score.outcome.expected_dir {
            hash_fixture_tree(
                &mut hasher,
                &format!("task/{}/expected", task.spec.id),
                &task.dir.join(expected_dir),
            )?;
        }
        if task.spec.score.semantic_verification.is_some() {
            hash_fixture_tree(
                &mut hasher,
                &format!("task/{}/verify", task.spec.id),
                &task.dir.join("verify"),
            )?;
        }
    }

    Ok(SuiteIdentity {
        fingerprint: hex::encode(hasher.finalize()),
        source: "selected_eval_behavior_v2".into(),
    })
}

fn task_input_fingerprint(tasks: &[&Task]) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut sorted_tasks: Vec<&Task> = tasks.to_vec();
    sorted_tasks.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));
    for task in sorted_tasks {
        let config = SuiteTask {
            id: &task.spec.id,
            prompt: &task.spec.prompt,
            tools: &task.spec.tools,
            max_turns: task.spec.max_turns,
            task_timeout_secs: task.spec.task_timeout_secs,
            budget_tokens: task.spec.budget_tokens,
            smoke: task.spec.smoke,
            score: &task.spec.score,
        };
        hash_value(&mut hasher, &format!("task/{}", task.spec.id), &config)?;
        hash_fixture_tree(
            &mut hasher,
            &format!("task/{}/before", task.spec.id),
            &task.dir.join("before"),
        )?;
        if let Some(expected_dir) = &task.spec.score.outcome.expected_dir {
            hash_fixture_tree(
                &mut hasher,
                &format!("task/{}/expected", task.spec.id),
                &task.dir.join(expected_dir),
            )?;
        }
        if task.spec.score.semantic_verification.is_some() {
            hash_fixture_tree(
                &mut hasher,
                &format!("task/{}/verify", task.spec.id),
                &task.dir.join("verify"),
            )?;
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

fn comparison_identity(
    model: &str,
    tasks: &[&Task],
    comparison: &ComparisonConfig,
    runs_per_case: u32,
) -> Result<ComparisonIdentity> {
    let mut controls = comparison.clone();
    controls.condition = None;
    controls.prompts.clear();
    controls.prompt_conditions.clear();
    #[derive(Serialize)]
    struct Inputs<'a> {
        model: &'a str,
        runs_per_case: u32,
        controls: &'a ComparisonConfig,
        task_input_fingerprint: String,
    }
    let inputs = Inputs {
        model,
        runs_per_case,
        controls: &controls,
        task_input_fingerprint: task_input_fingerprint(tasks)?,
    };
    Ok(ComparisonIdentity {
        fingerprint: hex::encode(Sha256::digest(serde_json::to_vec(&inputs)?)),
        source: "eval_comparison_controls_v1".into(),
    })
}

struct ComparisonSettings<'a> {
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    task_timeout_secs: u64,
    static_context_selection: &'a StaticContextSelection,
    cache_prewarm: Option<&'a CachePrewarmRun>,
    openrouter_routing: &'a str,
    condition: Option<&'a ResolvedEvalCondition>,
}

fn comparison_config(
    prompts: &[&Prompt],
    tasks: &[&Task],
    settings: ComparisonSettings<'_>,
) -> ComparisonConfig {
    let mut prompt_ids: Vec<String> = prompts.iter().map(|prompt| prompt.id.clone()).collect();
    let mut prompt_conditions: Vec<PromptCondition> = prompts
        .iter()
        .map(|prompt| PromptCondition {
            id: prompt.id.clone(),
            role: prompt
                .front
                .role
                .clone()
                .unwrap_or_else(|| "unclassified".into()),
            hypothesis: prompt
                .front
                .hypothesis
                .clone()
                .or_else(|| prompt.front.description.clone())
                .unwrap_or_default(),
        })
        .collect();
    let mut task_ids: Vec<String> = tasks.iter().map(|task| task.spec.id.clone()).collect();
    let mut excluded_dynamic_prompts: Vec<String> = settings
        .static_context_selection
        .excluded_prompts
        .iter()
        .map(|excluded| excluded.prompt_id.clone())
        .collect();
    prompt_ids.sort();
    prompt_conditions.sort_by(|a, b| a.id.cmp(&b.id));
    task_ids.sort();
    excluded_dynamic_prompts.sort();
    ComparisonConfig {
        prompts: prompt_ids,
        prompt_conditions,
        tasks: task_ids,
        max_tokens_per_task: settings.max_tokens_per_task,
        max_tokens_per_response: settings.max_tokens_per_response,
        max_turns: settings.max_turns,
        task_timeout_secs: settings.task_timeout_secs,
        static_context_only: settings.static_context_selection.enabled,
        excluded_dynamic_prompts,
        cache_prewarm: settings.cache_prewarm.is_some(),
        cache_ttl: settings.cache_prewarm.map(|cache| cache.ttl.clone()),
        openrouter_routing: settings.openrouter_routing.to_string(),
        condition: settings.condition.cloned(),
    }
}

#[derive(Debug, Serialize)]
struct RunCounts {
    total: usize,
    passed: usize,
    passed_over_budget: usize,
    failed: usize,
    limited: usize,
    errored: usize,
}

#[derive(Debug, Serialize)]
struct RunTotals {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    usd: f64,
}

#[derive(Debug, Serialize)]
struct CacheTokenTotals {
    cached_tokens: u64,
    cache_write_tokens: u64,
}

fn format_dirty(dirty: Option<bool>) -> &'static str {
    match dirty {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn format_duration_ms(duration_ms: u128) -> String {
    let total_seconds = duration_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

#[allow(clippy::too_many_arguments)]
fn write_run_artifacts(
    results_dir: &Path,
    _evals_dir: &Path,
    model: &str,
    prompts: &[String],
    tasks: &[String],
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    task_timeout_secs: u64,
    budget_stop_usd: f64,
    results: &[TaskResult],
    summary_md: &str,
    failures_md: &str,
    cache_prewarm: Option<&CachePrewarmRun>,
    static_context_selection: &StaticContextSelection,
    comparison: ComparisonConfig,
    comparison_identity: ComparisonIdentity,
    suite: SuiteIdentity,
    budget_stopped: bool,
    planned_results: usize,
    runs_per_case: u32,
    run_timing: &RunTiming,
    start_heddle_git: &GitInfo,
    start_evals_git: &GitInfo,
) -> Result<()> {
    fs::create_dir_all(results_dir)?;
    write_debug_error_log(results_dir, results)?;
    write_call_telemetry_log(results_dir, results)?;
    let reported = reported_results(results);

    let passed = reported
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Pass)
        .count();
    let passed_over_budget = reported
        .iter()
        .filter(|r| r.scores.outcome.passed && !r.scores.efficiency.tokens_in_budget)
        .count();
    let limited = reported
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Limit)
        .count();
    let errored = reported
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Error)
        .count();
    let failed = reported
        .iter()
        .filter(|r| result_status(r) == ResultStatus::Fail)
        .count();
    let cache_tokens = CacheTokenTotals {
        cached_tokens: reported.iter().map(|r| attempt_cost(r).cached_tokens).sum(),
        cache_write_tokens: reported
            .iter()
            .map(|r| attempt_cost(r).cache_write_tokens)
            .sum(),
    };
    let totals = run_totals_from_refs(reported.iter().copied());
    let meta = RunMeta {
        timestamp: Utc::now().to_rfc3339(),
        started_at: run_timing.started_at.clone(),
        finished_at: run_timing.finished_at.clone(),
        duration_ms: run_timing.duration_ms,
        matrix_runs: run_timing.matrix_runs.clone(),
        heddle_commit: start_heddle_git.commit.clone(),
        heddle_dirty: start_heddle_git.dirty,
        evals_commit: start_evals_git.commit.clone(),
        evals_dirty: start_evals_git.dirty,
        evals_version: "0.1.0".into(),
        model: model.to_string(),
        openrouter_routing: "balanced".into(),
        runs_per_case,
        prompts: prompts.to_vec(),
        prompt_conditions: comparison.prompt_conditions.clone(),
        tasks: tasks.to_vec(),
        max_tokens_per_task,
        max_tokens_per_response,
        max_turns,
        task_timeout_secs,
        budget_stop_usd,
        free_model: is_free_model(model),
        request_pacing_ms: is_free_model(model)
            .then_some(FREE_MODEL_REQUEST_INTERVAL.as_millis() as u64),
        counts: RunCounts {
            total: reported.len(),
            passed,
            passed_over_budget,
            failed,
            limited,
            errored,
        },
        totals,
        cache_tokens,
        cache_prewarm: cache_prewarm.cloned(),
        static_context_selection: static_context_selection.clone(),
        condition: comparison.condition.clone(),
        comparison_identity,
        comparison,
        suite,
        budget_stopped,
        planned_results_version: 2,
        planned_results,
    };

    fs::write(
        results_dir.join("run_meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    fs::write(
        results_dir.join("summary.json"),
        serde_json::to_string_pretty(results)?,
    )?;

    // summary.md: meta header + table + failures, paste-ready.
    let mut md = String::new();
    md.push_str(&format!("# Eval run — {}\n\n", meta.timestamp));
    md.push_str(&format!(
        "- wall_time: `{}` (started `{}`, finished `{}`)\n",
        format_duration_ms(meta.duration_ms),
        meta.started_at,
        meta.finished_at
    ));
    if !meta.matrix_runs.is_empty() {
        md.push_str(&format!(
            "- matrix_run_wall_times: {}\n",
            meta.matrix_runs
                .iter()
                .map(|run| format!(
                    "run {}={}",
                    run.run_index,
                    format_duration_ms(run.duration_ms)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    md.push_str(&format!("- model: `{}`\n", meta.model));
    md.push_str(&format!(
        "- openrouter_routing: `{}`\n",
        meta.openrouter_routing
    ));
    md.push_str(&format!(
        "- heddle: `{}` (dirty: `{}`)\n",
        meta.heddle_commit,
        format_dirty(meta.heddle_dirty)
    ));
    md.push_str(&format!(
        "- evals: `{}` (dirty: `{}`)\n",
        meta.evals_commit,
        format_dirty(meta.evals_dirty)
    ));
    md.push_str(&format!("- evals_version: `{}`\n", meta.evals_version));
    md.push_str(&format!("- prompts: {}\n", meta.prompts.join(", ")));
    md.push_str(&format!("- tasks: {}\n", meta.tasks.join(", ")));
    md.push_str(&format!(
        "- caps: max_tokens_per_task={}, max_tokens_per_response={}, max_turns={}, task_timeout_secs={}\n",
        meta.max_tokens_per_task,
        meta.max_tokens_per_response,
        meta.max_turns,
        meta.task_timeout_secs
    ));
    md.push_str(&format!(
        "- budget_stop_usd: `${:.4}`\n",
        meta.budget_stop_usd
    ));
    if meta.free_model {
        md.push_str(&format!(
            "- free_model: `true`; request_pacing_ms: `{}`\n",
            meta.request_pacing_ms.unwrap_or_default()
        ));
    }
    md.push_str(&format!(
        "- totals: {} prompt + {} completion = {} tokens, `${:.6}`\n",
        meta.totals.prompt_tokens,
        meta.totals.completion_tokens,
        meta.totals.total_tokens,
        meta.totals.usd
    ));
    if let Some(cache) = &meta.cache_prewarm {
        md.push_str(&format!(
            "- cache_prewarm: session=`{}`, ttl=`{}`, prefixes={}\n",
            cache.session_id,
            cache.ttl,
            cache.prewarms.len()
        ));
    }
    md.push_str(&format!(
        "- static_context_only: `{}`; excluded_prompts: {}\n",
        meta.static_context_selection.enabled,
        if meta.static_context_selection.excluded_prompts.is_empty() {
            "none".to_string()
        } else {
            meta.static_context_selection
                .excluded_prompts
                .iter()
                .map(|excluded| {
                    format!(
                        "{} ({})",
                        excluded.prompt_id,
                        excluded.dynamic_features.join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    md.push_str(summary_md);
    md.push_str(failures_md);
    fs::write(results_dir.join("summary.md"), md)?;
    Ok(())
}

fn write_debug_error_log(results_dir: &Path, results: &[TaskResult]) -> Result<()> {
    let entries = results
        .iter()
        .flat_map(|result| {
            result.debug_errors.iter().chain(
                result
                    .retry_attempts
                    .iter()
                    .flat_map(|attempt| attempt.debug_errors.iter()),
            )
        })
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    if !entries.is_empty() {
        fs::write(
            results_dir.join("debug-errors.jsonl"),
            format!("{}\n", entries.join("\n")),
        )?;
    }
    Ok(())
}

fn write_call_telemetry_log(results_dir: &Path, results: &[TaskResult]) -> Result<()> {
    let mut entries = Vec::new();
    for result in results {
        for retry in &result.retry_attempts {
            for call in &retry.call_telemetry {
                let mut call = call.clone();
                call.run_index = result.run_index;
                call.attempt = retry.attempt;
                entries.push(serde_json::to_string(&call)?);
            }
        }
        let attempt = result.retry_attempts.len() as u32 + 1;
        for call in &result.call_telemetry {
            let mut call = call.clone();
            call.run_index = result.run_index;
            call.attempt = attempt;
            entries.push(serde_json::to_string(&call)?);
        }
    }
    if !entries.is_empty() {
        fs::write(
            results_dir.join("call-telemetry.jsonl"),
            format!("{}\n", entries.join("\n")),
        )?;
    }
    Ok(())
}
