//! grep tool — shells out to `grep -rn` like the TS port.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};

pub struct GrepTool;

pub fn create_grep_tool() -> Arc<dyn HeddleTool> {
    Arc::new(GrepTool)
}

#[async_trait]
impl HeddleTool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search for a regex pattern in files. Uses ripgrep-style output with file paths and line numbers."
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

        let mut cmd = Command::new("grep");
        cmd.args(["-rn", "--color=never"]);
        if let Some(g) = &glob_filter {
            cmd.arg(format!("--include={g}"));
        }
        cmd.arg(&pattern).arg(&path);

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => return format!("Error: {e}"),
        };
        let exit = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if exit > 1 {
            return format!("Error: grep exited with code {exit}: {stderr}");
        }
        if exit == 1 || stdout.trim().is_empty() {
            return "No matches found.".to_string();
        }
        stdout.trim().to_string()
    }
}
