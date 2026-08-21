//! bash tool — runs a shell command, honors cancellation.

use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::WorkspaceBoundary;

pub struct BashTool;
pub struct WorkspaceBashTool(WorkspaceBoundary);

pub fn create_bash_tool() -> Arc<dyn HeddleTool> {
    Arc::new(BashTool)
}

/// Creates a Bash tool that is confined by the OS, rather than by parsing shell
/// text. Platforms without the supported confinement backend deny execution.
pub fn create_workspace_bash_tool(boundary: WorkspaceBoundary) -> Arc<dyn HeddleTool> {
    Arc::new(WorkspaceBashTool(boundary))
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
            .stderr(Stdio::piped())
            .kill_on_drop(true);
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

#[async_trait]
impl HeddleTool for WorkspaceBashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        BashTool.description()
    }
    fn parameters(&self) -> Value {
        BashTool.parameters()
    }

    async fn execute(&self, params: Value, options: ExecOptions) -> String {
        let command = match params.get("command").and_then(Value::as_str) {
            Some(command) => command,
            None => return "Error: missing command".to_string(),
        };
        if options
            .signal
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
        {
            return "Error: Aborted".to_string();
        }
        let mut cmd = match confined_bash_command(self.0.root(), command) {
            Ok(command) => command,
            Err(error) => return error,
        };
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => return format!("Error: {error}"),
        };
        let output_fut = async { child.wait_with_output().await };
        let output = if let Some(token) = options.signal {
            tokio::select! { output = output_fut => output, _ = token.cancelled() => return "Error: Aborted".to_string() }
        } else {
            output_fut.await
        };
        format_output(match output {
            Ok(output) => output,
            Err(error) => return format!("Error: {error}"),
        })
    }
}

fn format_output(output: std::process::Output) -> String {
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

#[cfg(target_os = "macos")]
fn confined_bash_command(root: &std::path::Path, command: &str) -> Result<Command, String> {
    // `sandbox-exec` is a process/filesystem boundary: expansion, redirects,
    // children, and inherited cwd are all subject to this profile.
    let root = sandbox_string(root)?;
    let profile = format!(
        "(version 1)\n(deny default)\n(allow process-exec)\n(allow process-fork)\n(allow signal (target self))\n(allow file-read* (subpath \\\"{root}\\\"))\n(allow file-write* (subpath \\\"{root}\\\"))\n(allow file-read* (subpath \\\"/bin\\\"))\n(allow file-read* (subpath \\\"/usr/bin\\\"))\n(allow file-read* (subpath \\\"/usr/lib\\\"))\n(allow file-read* (subpath \\\"/System/Library\\\"))\n(allow file-read* (literal \\\"/dev/null\\\"))"
    );
    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    cmd.args(["-p", &profile, "/bin/bash", "-c", command])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent");
    Ok(cmd)
}

#[cfg(target_os = "macos")]
fn sandbox_string(path: &std::path::Path) -> Result<String, String> {
    let value = path.to_string_lossy();
    if value.contains(['"', '\\']) {
        return Err("Error: workspace boundary denied unsafe workspace path".to_string());
    }
    Ok(value.into_owned())
}

#[cfg(not(target_os = "macos"))]
fn confined_bash_command(_root: &std::path::Path, _command: &str) -> Result<Command, String> {
    Err(
        "Error: workspace boundary denied bash: no supported filesystem sandbox on this platform"
            .to_string(),
    )
}
