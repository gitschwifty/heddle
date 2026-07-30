use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::{result_name_component, ComparisonConfig, SuiteIdentity, TaskResult};

#[derive(Debug, Deserialize)]
struct StoredRunMeta {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    heddle_commit: String,
    #[serde(default)]
    heddle_dirty: Option<bool>,
    #[serde(default)]
    evals_commit: String,
    #[serde(default)]
    evals_dirty: Option<bool>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    prompts: Vec<String>,
    #[serde(default)]
    tasks: Vec<String>,
    #[serde(default)]
    max_tokens_per_task: u64,
    #[serde(default)]
    max_tokens_per_response: u32,
    #[serde(default)]
    max_turns: u32,
    #[serde(default)]
    task_timeout_secs: u64,
    #[serde(default)]
    comparison: Option<ComparisonConfig>,
    #[serde(default)]
    suite: Option<SuiteIdentity>,
    #[serde(default)]
    budget_stopped: bool,
    #[serde(default)]
    planned_results: Option<usize>,
}

#[derive(Debug, Serialize)]
struct AggregateSuite {
    label: String,
    fingerprint: String,
    fingerprint_source: String,
}
#[derive(Debug, Serialize)]
struct AggregateProfile {
    label: String,
    fingerprint: String,
    comparison: ComparisonConfig,
}
#[derive(Debug, Serialize)]
struct AggregateRun {
    source_dir: String,
    timestamp: String,
    model: String,
    heddle_commit: String,
    heddle_dirty: Option<bool>,
    evals_commit: String,
    evals_dirty: Option<bool>,
    included_in_quality_metrics: bool,
    exclusion_reason: Option<String>,
    results: Vec<TaskResult>,
}
#[derive(Debug, Serialize)]
struct AggregateSnapshot {
    schema_version: u32,
    generated_at: String,
    suite: AggregateSuite,
    profile: AggregateProfile,
    runs: Vec<AggregateRun>,
}

fn fingerprint<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn short_hash(value: &str) -> String {
    value.chars().take(8).collect()
}

fn legacy_suite(meta: &StoredRunMeta) -> Result<SuiteIdentity> {
    #[derive(Serialize)]
    struct Legacy<'a> {
        evals_commit: &'a str,
        prompts: &'a [String],
        tasks: &'a [String],
    }
    Ok(SuiteIdentity {
        fingerprint: fingerprint(&Legacy {
            evals_commit: &meta.evals_commit,
            prompts: &meta.prompts,
            tasks: &meta.tasks,
        })?,
        source: "legacy_run_metadata_v1".into(),
    })
}

fn legacy_comparison(meta: &StoredRunMeta) -> ComparisonConfig {
    let mut prompts = meta.prompts.clone();
    let mut tasks = meta.tasks.clone();
    prompts.sort();
    tasks.sort();
    ComparisonConfig {
        prompts,
        tasks,
        runs_per_case: 1,
        max_tokens_per_task: meta.max_tokens_per_task,
        max_tokens_per_response: meta.max_tokens_per_response,
        max_turns: meta.max_turns,
        task_timeout_secs: meta.task_timeout_secs,
        static_context_only: false,
        excluded_dynamic_prompts: Vec::new(),
        cache_prewarm: false,
        cache_ttl: None,
        openrouter_routing: "balanced".into(),
    }
}

fn find_run_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        bail!("results root does not exist: {}", root.display());
    }
    let mut dirs = BTreeSet::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "run_meta.json" {
            let path = entry.path();
            if !path
                .components()
                .any(|part| part.as_os_str() == "aggregate")
            {
                if let Some(parent) = path.parent() {
                    dirs.insert(parent.to_path_buf());
                }
            }
        }
    }
    Ok(dirs.into_iter().collect())
}

fn markdown_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut output = String::new();
    output.push('|');
    for header in headers {
        output.push_str(&format!(" {header} |"));
    }
    output.push_str("\n|");
    for _ in headers {
        output.push_str(" --- |");
    }
    output.push('\n');
    for row in rows {
        output.push('|');
        for value in row {
            output.push_str(&format!(" {} |", value.replace('|', "\\|")));
        }
        output.push('\n');
    }
    output
}

fn report_rows(runs: &[AggregateRun], by_prompt: bool, by_heddle: bool) -> Vec<Vec<String>> {
    #[derive(Default)]
    struct Totals {
        cases: usize,
        passed: usize,
        failed: usize,
        errored: usize,
        tokens: u64,
        usd: f64,
    }
    let mut groups: BTreeMap<(String, String), Totals> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.included_in_quality_metrics) {
        for result in &run.results {
            let first = if by_heddle {
                run.heddle_commit.clone()
            } else {
                run.model.clone()
            };
            let second = if by_prompt {
                result.prompt_id.clone()
            } else {
                result.task_id.clone()
            };
            let totals = groups.entry((first, second)).or_default();
            totals.cases += 1;
            totals.tokens += result.scores.cost.tokens_in + result.scores.cost.tokens_out;
            totals.usd += result.scores.cost.usd;
            if result.scores.error.is_some() {
                totals.errored += 1;
            } else if result.scores.outcome.passed {
                totals.passed += 1;
            } else {
                totals.failed += 1;
            }
        }
    }
    groups
        .into_iter()
        .map(|((first, second), totals)| {
            vec![
                first,
                second,
                totals.cases.to_string(),
                format!("{}/{}", totals.passed, totals.cases),
                totals.failed.to_string(),
                totals.errored.to_string(),
                totals.tokens.to_string(),
                format!("${:.6}", totals.usd),
            ]
        })
        .collect()
}

fn write_reports(output: &Path, snapshot: &AggregateSnapshot) -> Result<()> {
    fs::create_dir_all(output)?;
    fs::write(
        output.join("profile.json"),
        serde_json::to_string_pretty(&snapshot.profile)?,
    )?;
    fs::write(
        output.join("runs.json"),
        serde_json::to_string_pretty(snapshot)?,
    )?;
    let excluded = snapshot
        .runs
        .iter()
        .filter(|run| !run.included_in_quality_metrics)
        .count();
    let preamble = format!(
        "# Eval aggregate\n\n- suite: `{}` (`{}`)\n- profile: `{}` (`{}`)\n- source runs: {}; excluded from quality metrics: {}\n\n",
        snapshot.suite.label, short_hash(&snapshot.suite.fingerprint), snapshot.profile.label,
        short_hash(&snapshot.profile.fingerprint), snapshot.runs.len(), excluded,
    );
    let headers = [
        "group",
        "task/prompt",
        "cases",
        "pass",
        "fail",
        "error",
        "tokens",
        "cost",
    ];
    for (name, heading, rows) in [
        (
            "by-model.md",
            "Model by task",
            report_rows(&snapshot.runs, false, false),
        ),
        (
            "by-prompt.md",
            "Model by prompt",
            report_rows(&snapshot.runs, true, false),
        ),
        (
            "by-heddle-revision.md",
            "Heddle revision by task",
            report_rows(&snapshot.runs, false, true),
        ),
    ] {
        fs::write(
            output.join(name),
            format!(
                "{preamble}## {heading}\n\n{}",
                markdown_table(&headers, &rows)
            ),
        )?;
    }
    Ok(())
}

pub(super) fn cmd_aggregate(
    evals: &Path,
    results_root: Option<&Path>,
    explicit_runs: &[PathBuf],
    suite_label: &str,
    profile_label: &str,
    output_dir: Option<&Path>,
) -> Result<()> {
    let default_root = evals.join("results");
    let root = results_root.unwrap_or(&default_root);
    let run_dirs = if explicit_runs.is_empty() {
        find_run_dirs(root)?
    } else {
        explicit_runs.to_vec()
    };
    if run_dirs.is_empty() {
        bail!("no completed run directories found");
    }
    let mut groups: BTreeMap<
        (String, String),
        (SuiteIdentity, ComparisonConfig, Vec<AggregateRun>),
    > = BTreeMap::new();
    for run_dir in run_dirs {
        let meta_path = run_dir.join("run_meta.json");
        let meta: StoredRunMeta = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("reading {}", meta_path.display()))?,
        )
        .with_context(|| format!("parsing {}", meta_path.display()))?;
        let results_path = run_dir.join("summary.json");
        let results: Vec<TaskResult> = serde_json::from_str(
            &fs::read_to_string(&results_path)
                .with_context(|| format!("reading {}", results_path.display()))?,
        )
        .with_context(|| format!("parsing {}", results_path.display()))?;
        let suite = meta.suite.clone().unwrap_or(legacy_suite(&meta)?);
        let comparison = meta
            .comparison
            .clone()
            .unwrap_or_else(|| legacy_comparison(&meta));
        let profile_fingerprint = fingerprint(&comparison)?;
        let incomplete = meta.budget_stopped
            || meta
                .planned_results
                .is_some_and(|planned| results.len() < planned);
        let exclusion_reason = if meta.budget_stopped {
            Some("budget_stopped".into())
        } else if incomplete {
            Some("incomplete_results".into())
        } else {
            None
        };
        let run = AggregateRun {
            source_dir: run_dir.display().to_string(),
            timestamp: meta.timestamp,
            model: meta.model,
            heddle_commit: meta.heddle_commit,
            heddle_dirty: meta.heddle_dirty,
            evals_commit: meta.evals_commit,
            evals_dirty: meta.evals_dirty,
            included_in_quality_metrics: exclusion_reason.is_none(),
            exclusion_reason,
            results,
        };
        groups
            .entry((suite.fingerprint.clone(), profile_fingerprint))
            .or_insert_with(|| (suite, comparison, Vec::new()))
            .2
            .push(run);
    }
    if output_dir.is_some() && groups.len() > 1 {
        bail!("--output-dir requires runs from exactly one suite/profile group");
    }
    for ((suite_fingerprint, profile_fingerprint), (suite, comparison, runs)) in groups {
        let output = output_dir.map(Path::to_path_buf).unwrap_or_else(|| {
            root.join("aggregate")
                .join(format!(
                    "{}__s-{}",
                    result_name_component(suite_label, "suite"),
                    short_hash(&suite_fingerprint)
                ))
                .join(format!(
                    "{}__c-{}",
                    result_name_component(profile_label, "profile"),
                    short_hash(&profile_fingerprint)
                ))
        });
        let snapshot = AggregateSnapshot {
            schema_version: 1,
            generated_at: Utc::now().to_rfc3339(),
            suite: AggregateSuite {
                label: suite_label.into(),
                fingerprint: suite_fingerprint,
                fingerprint_source: suite.source,
            },
            profile: AggregateProfile {
                label: profile_label.into(),
                fingerprint: profile_fingerprint,
                comparison,
            },
            runs,
        };
        write_reports(&output, &snapshot)?;
        println!("Wrote aggregate -> {}", output.display());
    }
    Ok(())
}
