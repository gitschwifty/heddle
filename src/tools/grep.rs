//! Search tool — prefers ripgrep, with an extended-regex grep fallback.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::SharedWorkspaceBoundary;

pub struct GrepTool;
pub struct WorkspaceGrepTool(SharedWorkspaceBoundary);

#[derive(Clone, Copy)]
enum SearchBackend {
    Ripgrep,
    Grep,
}

impl SearchBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Ripgrep => "rg",
            Self::Grep => "grep",
        }
    }
}

fn search_command(
    backend: SearchBackend,
    pattern: &str,
    path: &str,
    glob_filter: Option<&str>,
) -> Command {
    let mut command = Command::new(backend.name());
    command.args(search_args(backend, pattern, path, glob_filter));
    command
}

fn search_args(
    backend: SearchBackend,
    pattern: &str,
    path: &str,
    glob_filter: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();
    match backend {
        SearchBackend::Ripgrep => {
            args.extend(
                [
                    "--line-number",
                    "--no-heading",
                    "--color=never",
                    "--hidden",
                    "--no-ignore",
                ]
                .map(String::from),
            );
            if let Some(glob) = glob_filter {
                args.extend(["--glob".to_string(), glob.to_string()]);
            }
        }
        SearchBackend::Grep => {
            args.extend(["-rEn".to_string(), "--color=never".to_string()]);
            if let Some(glob) = glob_filter {
                args.push(format!("--include={glob}"));
            }
        }
    }
    args.extend(["-e".to_string(), pattern.to_string(), path.to_string()]);
    args
}

pub fn create_grep_tool() -> Arc<dyn HeddleTool> {
    Arc::new(GrepTool)
}
pub fn create_workspace_grep_tool(boundary: SharedWorkspaceBoundary) -> Arc<dyn HeddleTool> {
    Arc::new(WorkspaceGrepTool(boundary))
}

#[async_trait]
impl HeddleTool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search for a regex pattern in files with paths and line numbers. Uses ripgrep when available, otherwise extended-regex grep."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path":    { "type": "string", "description": "File or directory to search in (defaults to cwd)" },
                "glob":    { "type": "string", "description": "Glob filter for files (e.g. '*.ts')" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let pattern = match params.get("pattern").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => return "Error: missing pattern".to_string(),
        };
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| ".".to_string());
        let glob_filter = params.get("glob").and_then(Value::as_str).map(String::from);

        let (output, backend) = match search_command(
            SearchBackend::Ripgrep,
            &pattern,
            &path,
            glob_filter.as_deref(),
        )
        .output()
        .await
        {
            Ok(output) => (output, SearchBackend::Ripgrep),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match search_command(SearchBackend::Grep, &pattern, &path, glob_filter.as_deref())
                    .output()
                    .await
                {
                    Ok(output) => (output, SearchBackend::Grep),
                    Err(error) => return format!("Error: {error}"),
                }
            }
            Err(error) => return format!("Error: {error}"),
        };
        let exit = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if exit > 1 {
            return format!(
                "Error: {} exited with code {exit}: {stderr}",
                backend.name()
            );
        }
        if exit == 1 || stdout.trim().is_empty() {
            return "No matches found.".to_string();
        }
        stdout.trim().to_string()
    }
}

#[async_trait]
impl HeddleTool for WorkspaceGrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        GrepTool.description()
    }
    fn parameters(&self) -> Value {
        GrepTool.parameters()
    }
    async fn execute(&self, mut params: Value, options: ExecOptions) -> String {
        let raw = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = match self.0.read().resolve(raw) {
            Ok(path) => path,
            Err(error) => return error.to_string(),
        };
        params["path"] = json!(path);
        GrepTool.execute(params, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_fallback_uses_extended_regex_and_the_same_pattern_argument() {
        let args = search_args(SearchBackend::Grep, "one|two", "src", Some("*.rs"));
        assert!(args.contains(&"-rEn".to_string()));
        assert!(args.contains(&"--include=*.rs".to_string()));
        assert_eq!(args[args.len() - 3..], ["-e", "one|two", "src"]);
    }
}
