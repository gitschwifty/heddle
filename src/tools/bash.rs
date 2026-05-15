//! bash tool — runs a shell command, honors cancellation.

use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};

pub struct BashTool;

pub fn create_bash_tool() -> Arc<dyn HeddleTool> {
    Arc::new(BashTool)
}

#[async_trait]
impl HeddleTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Run a shell command and return its stdout and stderr."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to execute" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: Value, options: ExecOptions) -> String {
        let command = match params.get("command").and_then(Value::as_str) {
            Some(c) => c.to_string(),
            None => return "Error: missing command".to_string(),
        };
        if let Some(tok) = &options.signal {
            if tok.is_cancelled() {
                return "Error: Aborted".to_string();
            }
        }

        let mut cmd = Command::new("bash");
        cmd.args(["-c", &command])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return format!("Error: {e}"),
        };

        let output_fut = async { child.wait_with_output().await };
        let output = if let Some(tok) = options.signal.clone() {
            tokio::select! {
                out = output_fut => out,
                _ = tok.cancelled() => return "Error: Aborted".to_string(),
            }
        } else {
            output_fut.await
        };
        let output = match output {
            Ok(o) => o,
            Err(e) => return format!("Error: {e}"),
        };

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit = output.status.code().unwrap_or(-1);

        let mut out = String::new();
        if !stdout.is_empty() {
            out.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("STDERR: {stderr}"));
        }
        if exit != 0 {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("Exit code: {exit}"));
        }
        if out.is_empty() {
            "(no output)".to_string()
        } else {
            out
        }
    }
}
