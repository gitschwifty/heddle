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
    #[allow(dead_code)]
    #[serde(default)]
    budget_tokens: Option<u64>,
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
    within_budget: bool,
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

    let mut tool_calls = 0u32;
    let mut turns = 0u32;
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut error: Option<String> = None;
    let mut tool_sequence: Vec<String> = Vec::new();

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
                if tokens_in + tokens_out > max_tokens_per_task {
                    error = Some(format!(
                        "token budget exceeded: {} > {max_tokens_per_task}",
                        tokens_in + tokens_out
                    ));
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
    let within_budget = eff_min.map(|m| tool_calls >= m).unwrap_or(true)
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
        tool_sequence,
        scores: Scores {
            outcome: OutcomeScore {
                passed,
                diff_files: diff,
            },
            efficiency: EfficiencyScore {
                tool_calls,
                turns,
                within_budget,
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
        tool_sequence: Vec::new(),
        scores: Scores {
            outcome: OutcomeScore {
                passed: false,
                diff_files: Vec::new(),
            },
            efficiency: EfficiencyScore {
                tool_calls: 0,
                turns: 0,
                within_budget: false,
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

fn print_summary(results: &[TaskResult]) {
    if results.is_empty() {
        return;
    }
    let header = [
        "task", "prompt", "model", "outcome", "tools", "turns", "tokens", "err",
    ];
    let mut rows: Vec<[String; 8]> = Vec::with_capacity(results.len() + 1);
    rows.push(header.map(String::from));
    for r in results {
        let outcome = if r.scores.outcome.passed {
            "pass"
        } else {
            "FAIL"
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
    println!();
    println!("{}", render(&rows[0]));
    println!("|-{}-|", sep.join("-|-"));
    for row in &rows[1..] {
        println!("{}", render(row));
    }
    println!();
}

fn print_failure_details(results: &[TaskResult]) {
    let fails: Vec<&TaskResult> = results
        .iter()
        .filter(|r| !r.scores.outcome.passed)
        .collect();
    if fails.is_empty() {
        return;
    }
    println!("failures ({}):", fails.len());
    for r in fails {
        println!("  {} | {}", r.task_id, r.prompt_id);
        if let Some(e) = &r.scores.error {
            println!("    error: {e}");
        }
        if !r.scores.outcome.diff_files.is_empty() {
            for d in &r.scores.outcome.diff_files {
                println!("    diff: {} ({})", d.path, d.kind);
            }
        }
        if !r.tool_sequence.is_empty() {
            println!("    tools: {}", r.tool_sequence.join(" -> "));
        }
    }
    println!();
}

fn write_result(results_dir: &Path, r: &TaskResult) -> Result<()> {
    fs::create_dir_all(results_dir)?;
    let name = format!("{}__{}.json", r.task_id, r.prompt_id);
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
    let chosen_prompts = select(&all_prompts, prompts, |p| p.id.as_str())?;
    let chosen_tasks = select(&all_tasks, tasks, |t| t.spec.id.as_str())?;

    if chosen_prompts.is_empty() || chosen_tasks.is_empty() {
        bail!("nothing to run (no prompts or no tasks selected)");
    }

    let ts = Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let results_dir = results_dir.unwrap_or_else(|| evals.join("results").join(ts));
    println!(
        "Running {} prompts × {} tasks against {model}",
        chosen_prompts.len(),
        chosen_tasks.len()
    );
    println!("Results -> {}", results_dir.display());

    let mut results: Vec<TaskResult> = Vec::new();
    let total = chosen_tasks.len() * chosen_prompts.len();
    let mut idx = 0;
    for task in &chosen_tasks {
        for prompt in &chosen_prompts {
            idx += 1;
            println!("[{idx}/{total}] {} | {}", task.spec.id, prompt.id);
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
            let outcome = if r.scores.outcome.passed {
                "pass"
            } else {
                "FAIL"
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
    print_summary(&results);
    print_failure_details(&results);
    Ok(())
}
