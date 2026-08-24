//! bash tool — runs a shell command, honors cancellation.

use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::SharedWorkspaceBoundary;

pub struct BashTool;
pub struct WorkspaceBashTool(SharedWorkspaceBoundary);

pub fn create_bash_tool() -> Arc<dyn HeddleTool> {
    Arc::new(BashTool)
}

/// Creates a Bash tool that is confined by the OS, rather than by parsing shell
/// text. Platforms without the supported confinement backend deny execution.
pub fn create_workspace_bash_tool(boundary: SharedWorkspaceBoundary) -> Arc<dyn HeddleTool> {
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
        let roots: Vec<_> = self
            .0
            .read()
            .roots()
            .map(std::path::Path::to_path_buf)
            .collect();
        let mut cmd = match confined_bash_command(&roots, command) {
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
fn confined_bash_command(roots: &[std::path::PathBuf], command: &str) -> Result<Command, String> {
    // `sandbox-exec` is a process/filesystem boundary: expansion, redirects,
    // children, and inherited cwd are all subject to this profile.
    let root = roots
        .first()
        .ok_or_else(|| "Error: workspace boundary denied empty workspace".to_string())?;
    let root = sandbox_string(root)?;
    let additional = roots
        .iter()
        .skip(1)
        .map(|root| sandbox_string(root))
        .collect::<Result<Vec<_>, _>>()?;
    let toolchain = rust_toolchain_runtime()?;
    let profile = sandbox_profile(&root, &additional, toolchain.as_ref());
    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    cmd.args(["-p", &profile, "/bin/bash", "-c", command])
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        // Developer toolchains expect a real, writable home directory. Point
        // it at the workspace, never the user's actual home directory.
        .env("HOME", &root)
        // Keep temporary build files inside the writable workspace rather
        // than accidentally requiring access to the host's temp directory.
        .env("TMPDIR", root);
    if let Some(toolchain) = toolchain {
        cmd.env("PATH", format!("{}:/usr/bin:/bin", toolchain.cargo_bin));
        cmd.env("CARGO_HOME", toolchain.cargo_home);
    }
    Ok(cmd)
}

#[cfg(target_os = "macos")]
struct RustToolchainRuntime {
    cargo_bin: String,
    cargo_home: String,
    toolchain_root: String,
}

#[cfg(target_os = "macos")]
fn rust_toolchain_runtime() -> Result<Option<RustToolchainRuntime>, String> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Ok(None);
    };
    let cargo_home = home.join(".cargo");
    let cargo_bin = cargo_home.join("bin");
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".rustup"));
    if !cargo_bin.is_dir() || !rustup_home.is_dir() {
        return Ok(None);
    }
    // `~/.cargo/bin/cargo` is a Rustup shim. Running it inside the sandbox
    // makes Rustup update bookkeeping in the host home directory, which is
    // outside the boundary. Resolve the installed Cargo binary once, then
    // give the sandbox only that toolchain directory.
    let output = std::process::Command::new(cargo_bin.join("rustup"))
        .args(["which", "cargo"])
        .output()
        .map_err(|error| format!("Error: could not resolve Rust toolchain: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let cargo = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let toolchain_root = cargo
        .parent()
        .and_then(std::path::Path::parent)
        .filter(|root| root.starts_with(rustup_home.join("toolchains")))
        .ok_or_else(|| "Error: Rustup resolved an unsafe Cargo path".to_string())?;
    Ok(Some(RustToolchainRuntime {
        cargo_bin: sandbox_string(
            cargo.parent().ok_or_else(|| {
                "Error: Rustup resolved Cargo without a bin directory".to_string()
            })?,
        )?,
        cargo_home: sandbox_string(&cargo_home)?,
        toolchain_root: sandbox_string(toolchain_root)?,
    }))
}

#[cfg(target_os = "macos")]
fn sandbox_profile(
    root: &str,
    additional: &[String],
    toolchain: Option<&RustToolchainRuntime>,
) -> String {
    let additional_rules = additional
        .iter()
        .map(|root| {
            format!(
                "(allow file-read* file-map-executable (subpath \"{root}\"))\n(allow file-write* (subpath \"{root}\"))\n"
            )
        })
        .collect::<String>();
    let toolchain_rules = toolchain.map_or_else(String::new, |toolchain| {
        format!(
            "(allow file-read* file-map-executable (subpath \"{}\"))\n(allow file-read* (literal \"{}\") (subpath \"{}/registry\") (subpath \"{}/git\"))\n(allow file-read* file-map-executable (subpath \"{}\"))\n",
            toolchain.cargo_bin,
            toolchain.cargo_home,
            toolchain.cargo_home,
            toolchain.cargo_home,
            toolchain.toolchain_root
        )
    });
    format!(
        "(version 1)\n(deny default)\n(allow process-exec)\n(allow process-fork)\n(allow signal (target same-sandbox))\n(allow process-info* (target same-sandbox))\n(allow sysctl-read)\n(allow mach-lookup (global-name \"com.apple.system.opendirectoryd.libinfo\"))\n(allow pseudo-tty)\n(allow file-read* file-write-data (literal \"/dev/null\"))\n(allow file-read* file-write-data (literal \"/dev/zero\"))\n(allow file-read-data file-write-data (subpath \"/dev/fd\"))\n(allow file-read* file-test-existence (literal \"/\") (literal \"/dev/random\") (literal \"/dev/urandom\") (subpath \"/Library/Apple\") (subpath \"/System/Library\") (subpath \"/usr/lib\") (subpath \"/usr/share\") (subpath \"/private/etc\"))\n(allow file-map-executable (subpath \"/Library/Apple/System/Library/Frameworks\") (subpath \"/Library/Apple/System/Library/PrivateFrameworks\") (subpath \"/System/Library/Frameworks\") (subpath \"/System/Library/PrivateFrameworks\") (subpath \"/usr/lib\"))\n(allow file-read-data file-read-metadata (subpath \"/bin\") (subpath \"/usr/bin\"))\n(allow file-read-metadata file-test-existence (literal \"/etc\") (literal \"/tmp\") (literal \"/var\") (path-ancestors \"/System/Volumes/Data/private\"))\n(allow file-read* file-map-executable (subpath \"{root}\"))\n(allow file-write* (subpath \"{root}\"))\n{additional_rules}{toolchain_rules}"
    )
}

#[cfg(target_os = "macos")]
fn sandbox_string(path: &std::path::Path) -> Result<String, String> {
    // macOS exposes aliases such as `/tmp` and `/var`; Seatbelt evaluates the
    // physical path. Canonicalizing prevents a workspace allowance from
    // missing files reached through one of those aliases.
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let value = canonical.to_string_lossy();
    if value.contains(['"', '\\']) {
        return Err("Error: workspace boundary denied unsafe workspace path".to_string());
    }
    Ok(value.into_owned())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{sandbox_profile, RustToolchainRuntime};

    #[test]
    fn sandbox_profile_uses_sandbox_string_literals_for_workspace_paths() {
        let profile = sandbox_profile("/private/tmp/workspace", &[], None);

        assert!(profile.contains("(subpath \"/private/tmp/workspace\")"));
        assert!(!profile.contains("\\\\\"/private/tmp/workspace\\\\\""));
    }

    #[test]
    fn sandbox_profile_allows_only_the_rust_runtime_paths_needed_by_cargo() {
        let profile = sandbox_profile(
            "/private/tmp/workspace",
            &[],
            Some(&RustToolchainRuntime {
                cargo_bin: "/Users/test/.cargo/bin".to_string(),
                cargo_home: "/Users/test/.cargo".to_string(),
                toolchain_root: "/Users/test/.rustup/toolchains/stable-aarch64-apple-darwin"
                    .to_string(),
            }),
        );

        assert!(profile.contains("(subpath \"/Users/test/.cargo/bin\")"));
        assert!(profile.contains("(subpath \"/Users/test/.cargo/registry\")"));
        assert!(profile
            .contains("(subpath \"/Users/test/.rustup/toolchains/stable-aarch64-apple-darwin\")"));
        assert!(!profile.contains("(allow file-write* (subpath \"/Users/test/.cargo\"))"));
    }
}

#[cfg(not(target_os = "macos"))]
fn confined_bash_command(_roots: &[std::path::PathBuf], _command: &str) -> Result<Command, String> {
    Err(
        "Error: workspace boundary denied bash: no supported filesystem sandbox on this platform"
            .to_string(),
    )
}
