//! Heddle eval harness runner.
//!
//! Loads task + prompt fixtures from an eval directory (default
//! `evals-staging/` next to the worktree, or `--evals <path>`), runs each
//! (task, prompt) pair against the agent loop, and scores outcome +
//! efficiency + cost.
//!
//! See `evals-staging/README.md` for the prompt/task format.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::openrouter::create_openrouter_provider;
use heddle::provider::types::{Provider, ProviderConfig};
use heddle::tools::bash::create_bash_tool;
use heddle::tools::edit::create_edit_tool;
use heddle::tools::glob::create_glob_tool;
use heddle::tools::grep::create_grep_tool;
use heddle::tools::read::create_read_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::HeddleTool;
use heddle::tools::web_fetch::create_web_fetch_tool;
use heddle::tools::write::create_write_tool;
use heddle::types::{Message, SystemMessage, UserMessage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;

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
        #[arg(long, default_value = "evals-staging")]
        evals: PathBuf,
    },
    /// Run one or more (task, prompt) pairs.
    Run {
        #[arg(long, default_value = "evals-staging")]
        evals: PathBuf,
        /// Comma-separated prompt ids, or "all".
        #[arg(long, default_value = "all")]
        prompts: String,
        /// Comma-separated task ids, or "all".
        #[arg(long, default_value = "all")]
        tasks: String,
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
        /// Hard cap on turns per task (default 8).
        #[arg(long, default_value_t = 8)]
        max_turns: u32,
        /// Abort the sweep if cumulative cost crosses this USD value.
        #[arg(long, default_value_t = 1.0)]
        budget_stop_usd: f64,
        /// Write results under this directory (default <evals>/results/<ts>/).
        #[arg(long)]
        results_dir: Option<PathBuf>,
        /// Number of times to run each (task, prompt) pair. When >1, the
        /// summary aggregates with mean ± stddev per pair. Useful for
        /// averaging out LLM stochastic variance.
        #[arg(long, default_value_t = 1)]
        runs: u32,
    },
}

// ─── Manifest / prompt / task schemas ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default = "default_version")]
    version: String,
    #[serde(default)]
    default_model: Option<String>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    budget_stop_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PromptFrontMatter {
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    context: ContextConfig,
    /// When true, skip this prompt when running `--prompts all`. The prompt
    /// is still selectable by explicit name. Use for retired prompts kept
    /// for reference, or known-failing baselines.
    #[serde(default)]
    matrix_exclude: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
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

#[derive(Debug, Clone, Deserialize)]
struct GitConfig {
    #[serde(default)]
    branch: bool,
    #[serde(default)]
    status: bool,
}

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    max_turns: Option<u32>,
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

#[derive(Debug, Clone, Deserialize)]
struct TaskScoreSpec {
    outcome: OutcomeSpec,
    #[serde(default)]
    efficiency: Option<EfficiencySpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct OutcomeSpec {
    expected_dir: String,
    #[serde(default)]
    #[allow(dead_code)]
    ignore_globs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Serialize)]
struct TaskResult {
    task_id: String,
    prompt_id: String,
    model: String,
    heddle_commit: String,
    evals_version: String,
    timestamp: String,
    duration_ms: u128,
    scores: Scores,
    rendered_system_prompt_chars: usize,
    /// 1-indexed run number when --runs N. 0 if single-run.
    #[serde(default)]
    run_index: u32,
    /// Order of tool calls (names only). Useful for diagnosing why a task
    /// failed without re-reading the result JSON.
    tool_sequence: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Scores {
    outcome: OutcomeScore,
    efficiency: EfficiencyScore,
    cost: CostScore,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct OutcomeScore {
    passed: bool,
    diff_files: Vec<DirDiffEntry>,
}

#[derive(Debug, Serialize)]
struct DirDiffEntry {
    path: String,
    kind: String, // "missing" | "unexpected" | "differs"
}

#[derive(Debug, Serialize)]
struct EfficiencyScore {
    tool_calls: u32,
    turns: u32,
    /// Tool-call count fell within task.toml [min, max] range.
    tool_calls_in_range: bool,
    /// Total tokens stayed under the per-task budget (CLI default or task
    /// override). When false, the task was force-aborted but still scored
    /// on whatever workspace state existed at the time.
    tokens_in_budget: bool,
}

#[derive(Debug, Serialize)]
struct CostScore {
    tokens_in: u64,
    tokens_out: u64,
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
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(load_prompt(&path)?);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn load_task(dir: &Path) -> Result<Task> {
    let toml_path = dir.join("task.toml");
    let text = fs::read_to_string(&toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;
    let spec: TaskSpec =
        toml::from_str(&text).with_context(|| format!("parsing {}", toml_path.display()))?;
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
/// `0.2.0` vs `0.2.0\n` vs `0.2.0\r\n` is noise we don't want to score on.
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

// ─── Tool selection ──────────────────────────────────────────────────────

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

fn build_registry(names: &[String]) -> Result<ToolRegistry> {
    let mut r = ToolRegistry::new();
    for n in names {
        let tool = tool_by_name(n).ok_or_else(|| anyhow!("unknown tool: {n}"))?;
        r.register(tool)?;
    }
    Ok(r)
}

// ─── Runner ──────────────────────────────────────────────────────────────

const FREE_FALLBACK: &[&str] = &[
    "liquid/lfm-2.5-1.2b-instruct:free",
    "arcee-ai/trinity-large-preview:free",
    "arcee-ai/trinity-mini:free",
    "openrouter/free",
];

fn make_provider(model: &str, api_key: String, max_tokens_per_response: u32) -> Arc<dyn Provider> {
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
    create_openrouter_provider(ProviderConfig {
        api_key,
        model: model.to_string(),
        base_url: None,
        request_params: Some(serde_json::Value::Object(params)),
        retry: None,
    })
}

async fn run_one(
    task: &Task,
    prompt: &Prompt,
    model: &str,
    api_key: &str,
    max_turns: u32,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
) -> TaskResult {
    let start = Instant::now();
    let tempdir = tempfile::tempdir().expect("tempdir");
    let workspace = tempdir.path();
    if let Err(e) = copy_dir_recursive(&task.dir.join("before"), workspace) {
        return error_result(task, prompt, model, format!("copy before/: {e}"), start);
    }

    let composed = compose_system_prompt(prompt, workspace);
    let mut messages = Vec::new();
    if !composed.is_empty() {
        messages.push(Message::System(SystemMessage {
            content: composed.clone(),
        }));
    }
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
    let registry = match build_registry(&tool_names) {
        Ok(r) => r,
        Err(e) => return error_result(task, prompt, model, e.to_string(), start),
    };

    let provider = make_provider(model, api_key.to_string(), max_tokens_per_response);
    let effective_max_turns = task.spec.max_turns.unwrap_or(max_turns).min(max_turns);
    // task.toml budget wins when set; else CLI default.
    let effective_max_tokens = task.spec.budget_tokens.unwrap_or(max_tokens_per_task);

    let mut tool_calls = 0u32;
    let mut turns = 0u32;
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut error: Option<String> = None;
    let mut tool_sequence: Vec<String> = Vec::new();
    let mut budget_exceeded = false;

    let prev_cwd = std::env::current_dir().ok();
    if std::env::set_current_dir(workspace).is_err() {
        return error_result(task, prompt, model, "set_current_dir failed".into(), start);
    }

    let opts = AgentLoopOptions {
        max_iterations: Some(effective_max_turns),
        ..AgentLoopOptions::default()
    };
    let stream = run_agent_loop(provider, registry, &mut messages, opts);
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::ToolStart { name, .. } => {
                tool_calls += 1;
                println!("      -> {name}");
                std::io::Write::flush(&mut std::io::stdout()).ok();
                tool_sequence.push(name);
            }
            AgentEvent::AssistantMessage { .. } => turns += 1,
            AgentEvent::Usage { usage } => {
                tokens_in += usage.prompt_tokens;
                tokens_out += usage.completion_tokens;
                if tokens_in + tokens_out > effective_max_tokens {
                    // Cost-control kill switch, NOT a correctness failure.
                    // Break so we don't burn more tokens, but still attempt
                    // the dir diff below — the agent may have done the work
                    // and just emitted verbose tail-text after.
                    budget_exceeded = true;
                    break;
                }
            }
            AgentEvent::Error { message } => {
                error = Some(message);
                break;
            }
            _ => {}
        }
    }
    if let Some(prev) = prev_cwd {
        let _ = std::env::set_current_dir(prev);
    }

    let diff = diff_dirs(
        workspace,
        &task.dir.join(task.spec.score.outcome.expected_dir.as_str()),
    );
    let passed = diff.is_empty() && error.is_none();

    let (eff_min, eff_max) = match &task.spec.score.efficiency {
        Some(e) => (e.min_tool_calls, e.max_tool_calls),
        None => (None, None),
    };
    let tool_calls_in_range = eff_min.map(|m| tool_calls >= m).unwrap_or(true)
        && eff_max.map(|m| tool_calls <= m).unwrap_or(true);

    TaskResult {
        task_id: task.spec.id.clone(),
        prompt_id: prompt.id.clone(),
        model: model.to_string(),
        heddle_commit: git_short_sha().unwrap_or_else(|| "unknown".into()),
        evals_version: "0.1.0".into(),
        timestamp: Utc::now().to_rfc3339(),
        duration_ms: start.elapsed().as_millis(),
        rendered_system_prompt_chars: composed.chars().count(),
        run_index: 0,
        tool_sequence,
        scores: Scores {
            outcome: OutcomeScore {
                passed,
                diff_files: diff,
            },
            efficiency: EfficiencyScore {
                tool_calls,
                turns,
                tool_calls_in_range,
                tokens_in_budget: !budget_exceeded,
            },
            cost: CostScore {
                tokens_in,
                tokens_out,
                usd: 0.0,
            },
            error,
        },
    }
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
        model: model.to_string(),
        heddle_commit: git_short_sha().unwrap_or_else(|| "unknown".into()),
        evals_version: "0.1.0".into(),
        timestamp: Utc::now().to_rfc3339(),
        duration_ms: start.elapsed().as_millis(),
        rendered_system_prompt_chars: 0,
        run_index: 0,
        tool_sequence: Vec::new(),
        scores: Scores {
            outcome: OutcomeScore {
                passed: false,
                diff_files: Vec::new(),
            },
            efficiency: EfficiencyScore {
                tool_calls: 0,
                turns: 0,
                tool_calls_in_range: false,
                tokens_in_budget: true,
            },
            cost: CostScore {
                tokens_in: 0,
                tokens_out: 0,
                usd: 0.0,
            },
            error: Some(err),
        },
    }
}

fn git_short_sha() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ─── Output ──────────────────────────────────────────────────────────────

fn format_summary(results: &[TaskResult]) -> String {
    let mut out = String::new();
    if results.is_empty() {
        return out;
    }
    let header = [
        "task", "prompt", "model", "outcome", "tools", "turns", "tokens", "err",
    ];
    let mut rows: Vec<[String; 8]> = Vec::with_capacity(results.len() + 1);
    rows.push(header.map(String::from));
    for r in results {
        let outcome = match (
            r.scores.outcome.passed,
            r.scores.efficiency.tokens_in_budget,
        ) {
            (true, true) => "pass",
            (true, false) => "pass*",
            (false, _) => "FAIL",
        };
        let err = r.scores.error.as_deref().unwrap_or("");
        let err = if err.len() > 50 { &err[..50] } else { err };
        rows.push([
            r.task_id.clone(),
            r.prompt_id.clone(),
            r.model.clone(),
            outcome.to_string(),
            r.scores.efficiency.tool_calls.to_string(),
            r.scores.efficiency.turns.to_string(),
            format!("{}/{}", r.scores.cost.tokens_in, r.scores.cost.tokens_out),
            err.to_string(),
        ]);
    }
    let mut widths = [0usize; 8];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |row: &[String; 8]| -> String {
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
    let pass = results.iter().filter(|r| r.scores.outcome.passed).count();
    let over_budget = results
        .iter()
        .filter(|r| r.scores.outcome.passed && !r.scores.efficiency.tokens_in_budget)
        .count();
    let fail = results.len() - pass;
    out.push('\n');
    out.push_str(&format!(
        "{pass} passed ({over_budget} over budget), {fail} failed of {} total\n",
        results.len()
    ));
    if over_budget > 0 {
        out.push_str(
            "(`pass*` = correct outcome but token budget exceeded mid-run; not a failure)\n",
        );
    }
    out.push('\n');
    out
}

/// Aggregate per (task, prompt) over multiple runs. Reports pass rate
/// (X/N), mean tokens (in/out), mean tool_calls and turns, and a stddev
/// flag when tokens vary by >25% from mean (indicates noise).
fn format_aggregated_summary(results: &[TaskResult], runs: u32) -> String {
    use std::collections::BTreeMap;
    if results.is_empty() {
        return String::new();
    }
    // Group by (task_id, prompt_id).
    let mut groups: BTreeMap<(String, String), Vec<&TaskResult>> = BTreeMap::new();
    for r in results {
        groups
            .entry((r.task_id.clone(), r.prompt_id.clone()))
            .or_default()
            .push(r);
    }

    let header = [
        "task",
        "prompt",
        "outcome",
        "tools (avg)",
        "turns (avg)",
        "tokens in (avg±std)",
        "tokens out (avg)",
    ];
    let mut rows: Vec<[String; 7]> = Vec::with_capacity(groups.len() + 1);
    rows.push(header.map(String::from));

    for ((task_id, prompt_id), runs_of) in &groups {
        let n = runs_of.len() as f64;
        let passed = runs_of.iter().filter(|r| r.scores.outcome.passed).count();
        let pass_rate = format!("{}/{}", passed, runs_of.len());
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
        ]);
    }

    let mut widths = [0usize; 7];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |row: &[String; 7]| -> String {
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
    out
}

fn format_failure_details(results: &[TaskResult]) -> String {
    let mut out = String::new();
    let fails: Vec<&TaskResult> = results
        .iter()
        .filter(|r| !r.scores.outcome.passed)
        .collect();
    if fails.is_empty() {
        return out;
    }
    out.push_str(&format!("failures ({}):\n", fails.len()));
    for r in fails {
        out.push_str(&format!("  {} | {}\n", r.task_id, r.prompt_id));
        if let Some(e) = &r.scores.error {
            out.push_str(&format!("    error: {e}\n"));
        }
        if !r.scores.outcome.diff_files.is_empty() {
            for d in &r.scores.outcome.diff_files {
                out.push_str(&format!("    diff: {} ({})\n", d.path, d.kind));
            }
        }
        if !r.tool_sequence.is_empty() {
            out.push_str(&format!("    tools: {}\n", r.tool_sequence.join(" -> ")));
        }
    }
    out.push('\n');
    out
}

fn write_result(results_dir: &Path, r: &TaskResult) -> Result<()> {
    fs::create_dir_all(results_dir)?;
    let name = if r.run_index > 0 {
        format!("{}__{}__run{}.json", r.task_id, r.prompt_id, r.run_index)
    } else {
        format!("{}__{}.json", r.task_id, r.prompt_id)
    };
    let path = results_dir.join(name);
    fs::write(&path, serde_json::to_string_pretty(r)?)?;
    Ok(())
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
        Cmd::Run {
            evals,
            prompts,
            tasks,
            model,
            max_tokens_per_task,
            max_tokens_per_response,
            max_turns,
            budget_stop_usd: _,
            results_dir,
            runs,
        } => {
            cmd_run(
                &evals,
                &prompts,
                &tasks,
                model.as_deref(),
                max_tokens_per_task,
                max_tokens_per_response,
                max_turns,
                results_dir,
                runs.max(1),
            )
            .await
        }
    }
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
        println!(
            "  {:<28} body={:>5}c  cwd={} date={} git={} tree={}",
            p.id, chars, cwd, date, git, tree
        );
    }
    println!();
    println!("tasks ({}):", tasks.len());
    for t in &tasks {
        println!(
            "  {:<28} max_turns={}  tools={:?}",
            t.spec.id,
            t.spec.max_turns.unwrap_or(8),
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

#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    evals: &Path,
    prompts: &str,
    tasks: &str,
    model_flag: Option<&str>,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    results_dir: Option<PathBuf>,
    runs: u32,
) -> Result<()> {
    let manifest = load_manifest(evals)?;
    let model = model_flag
        .map(|s| s.to_string())
        .or_else(|| manifest.default_model.clone())
        .unwrap_or_else(|| "openrouter/free".into());

    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow!("OPENROUTER_API_KEY not set (looked in env, .env.local, .env)"))?;

    let all_prompts = load_prompts(evals)?;
    let all_tasks = load_tasks(evals)?;
    let mut chosen_prompts = select(&all_prompts, prompts, |p| p.id.as_str())?;
    let chosen_tasks = select(&all_tasks, tasks, |t| t.spec.id.as_str())?;

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

    if chosen_prompts.is_empty() || chosen_tasks.is_empty() {
        bail!("nothing to run (no prompts or no tasks selected)");
    }

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

    let ts = Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let results_dir = results_dir.unwrap_or_else(|| evals.join("results").join(ts));
    let total_pairs = smoke_pairs.len() + matrix_pairs.len();
    println!("Running {total_pairs} (task, prompt) pairs against {model}");
    if is_matrix && smoke_count > 0 {
        println!(
            "  ({} smoke run(s) up front; {} matrix run(s) after — matrix aborts if any smoke fails)",
            smoke_pairs.len(),
            matrix_pairs.len()
        );
    }
    println!("Results -> {}", results_dir.display());

    let mut results: Vec<TaskResult> = Vec::new();
    let mut smoke_failed = false;

    // Pass 1: smoke
    let smoke_total = smoke_pairs.len();
    for (i, (task, prompt)) in smoke_pairs.iter().enumerate() {
        let idx = i + 1;
        println!(
            "[smoke {idx}/{smoke_total}] {} | {}",
            task.spec.id, prompt.id
        );
        let r = run_one(
            task,
            prompt,
            &model,
            &api_key,
            max_turns,
            max_tokens_per_task,
            max_tokens_per_response,
        )
        .await;
        let outcome = match (
            r.scores.outcome.passed,
            r.scores.efficiency.tokens_in_budget,
        ) {
            (true, true) => "pass",
            (true, false) => "pass*",
            (false, _) => "FAIL",
        };
        println!(
            "      {outcome} (tools={}, turns={}, tokens={}/{}, {}ms)",
            r.scores.efficiency.tool_calls,
            r.scores.efficiency.turns,
            r.scores.cost.tokens_in,
            r.scores.cost.tokens_out,
            r.duration_ms,
        );
        if !r.scores.outcome.passed {
            smoke_failed = true;
        }
        write_result(&results_dir, &r)?;
        results.push(r);
    }

    if smoke_failed && !matrix_pairs.is_empty() {
        eprintln!();
        eprintln!(
            "❌ smoke failed — aborting before {} matrix run(s) to save budget.",
            matrix_pairs.len()
        );
        eprintln!("Investigate the smoke failures above before re-running the matrix.");
        eprintln!();
    } else {
        // Pass 2: matrix, repeated `runs` times to average out variance.
        let matrix_total = matrix_pairs.len();
        for run_n in 1..=runs {
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
                let mut r = run_one(
                    task,
                    prompt,
                    &model,
                    &api_key,
                    max_turns,
                    max_tokens_per_task,
                    max_tokens_per_response,
                )
                .await;
                if runs > 1 {
                    r.run_index = run_n;
                }
                let outcome = match (
                    r.scores.outcome.passed,
                    r.scores.efficiency.tokens_in_budget,
                ) {
                    (true, true) => "pass",
                    (true, false) => "pass*",
                    (false, _) => "FAIL",
                };
                println!(
                    "      {outcome} (tools={}, turns={}, tokens={}/{}, {}ms)",
                    r.scores.efficiency.tool_calls,
                    r.scores.efficiency.turns,
                    r.scores.cost.tokens_in,
                    r.scores.cost.tokens_out,
                    r.duration_ms,
                );
                write_result(&results_dir, &r)?;
                results.push(r);
            }
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

    write_run_artifacts(
        &results_dir,
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
        &results,
        &summary,
        &failures,
    )?;
    println!(
        "Wrote summary.md, summary.json, run_meta.json -> {}",
        results_dir.display()
    );
    Ok(())
}

#[derive(Debug, Serialize)]
struct RunMeta {
    timestamp: String,
    heddle_commit: String,
    evals_version: String,
    model: String,
    prompts: Vec<String>,
    tasks: Vec<String>,
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    counts: RunCounts,
}

#[derive(Debug, Serialize)]
struct RunCounts {
    total: usize,
    passed: usize,
    passed_over_budget: usize,
    failed: usize,
}

#[allow(clippy::too_many_arguments)]
fn write_run_artifacts(
    results_dir: &Path,
    model: &str,
    prompts: &[String],
    tasks: &[String],
    max_tokens_per_task: u64,
    max_tokens_per_response: u32,
    max_turns: u32,
    results: &[TaskResult],
    summary_md: &str,
    failures_md: &str,
) -> Result<()> {
    fs::create_dir_all(results_dir)?;

    let passed = results.iter().filter(|r| r.scores.outcome.passed).count();
    let passed_over_budget = results
        .iter()
        .filter(|r| r.scores.outcome.passed && !r.scores.efficiency.tokens_in_budget)
        .count();
    let meta = RunMeta {
        timestamp: Utc::now().to_rfc3339(),
        heddle_commit: git_short_sha().unwrap_or_else(|| "unknown".into()),
        evals_version: "0.1.0".into(),
        model: model.to_string(),
        prompts: prompts.to_vec(),
        tasks: tasks.to_vec(),
        max_tokens_per_task,
        max_tokens_per_response,
        max_turns,
        counts: RunCounts {
            total: results.len(),
            passed,
            passed_over_budget,
            failed: results.len() - passed,
        },
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
    md.push_str(&format!("- model: `{}`\n", meta.model));
    md.push_str(&format!("- heddle: `{}`\n", meta.heddle_commit));
    md.push_str(&format!("- evals_version: `{}`\n", meta.evals_version));
    md.push_str(&format!("- prompts: {}\n", meta.prompts.join(", ")));
    md.push_str(&format!("- tasks: {}\n", meta.tasks.join(", ")));
    md.push_str(&format!(
        "- caps: max_tokens_per_task={}, max_tokens_per_response={}, max_turns={}\n",
        meta.max_tokens_per_task, meta.max_tokens_per_response, meta.max_turns
    ));
    md.push_str(summary_md);
    md.push_str(failures_md);
    fs::write(results_dir.join("summary.md"), md)?;
    Ok(())
}
