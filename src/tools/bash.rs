//! bash tool — runs a shell command, honors cancellation.

use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::SharedWorkspaceBoundary;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::confined_bash_command;

pub struct BashTool;
pub struct WorkspaceBashTool {
    boundary: SharedWorkspaceBoundary,
    additional_deny_paths: Vec<std::path::PathBuf>,
}

/// Agent-facing capabilities for the OS-confined Bash tool. Provider traffic
/// is not part of this capability: it never runs inside the Bash child.
pub const WORKSPACE_BASH_CAPABILITY_CONTEXT: &str = "## Sandbox Capability Context\n\n- `network_mode: open`\n- Outbound network is available to Bash commands in this experimental developer sandbox.\n- The normal developer environment is available, except Heddle and recognised credential variables are removed before Bash starts.\n- Host credential files such as `.ssh`, `.gnupg`, `.aws`, and `.netrc` remain blocked by the OS sandbox. SSH-agent-backed signing can still use `SSH_AUTH_SOCK` without reading private key files.";

const WORKSPACE_BASH_DESCRIPTION: &str = "Run a shell command and return its stdout and stderr. Bash runs in an experimental developer sandbox: workspace/runtime writes are confined, outbound network is available, and recognised credential environment variables are removed.";

pub fn create_bash_tool() -> Arc<dyn HeddleTool> {
    Arc::new(BashTool)
}

/// Creates a Bash tool that is confined by the OS, rather than by parsing shell
/// text. Platforms without the supported confinement backend deny execution.
pub fn create_workspace_bash_tool(boundary: SharedWorkspaceBoundary) -> Arc<dyn HeddleTool> {
    create_workspace_bash_tool_with_deny_paths(boundary, Vec::new())
}

/// Like [`create_workspace_bash_tool`], with caller-configured host paths that
/// receive additional read/write deny rules in the OS sandbox.
pub fn create_workspace_bash_tool_with_deny_paths(
    boundary: SharedWorkspaceBoundary,
    additional_deny_paths: Vec<std::path::PathBuf>,
) -> Arc<dyn HeddleTool> {
    Arc::new(WorkspaceBashTool {
        boundary,
        additional_deny_paths,
    })
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
        WORKSPACE_BASH_DESCRIPTION
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
        let (roots, runtime_root) = {
            let boundary = self.boundary.read();
            let roots: Vec<std::path::PathBuf> =
                boundary.roots().map(std::path::Path::to_path_buf).collect();
            (roots, boundary.runtime_root().to_path_buf())
        };
        let mut cmd = match confined_bash_command(
            &roots,
            &runtime_root,
            &self.additional_deny_paths,
            command,
        ) {
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

/// Remove ambient credentials from the Bash child while preserving ordinary
/// developer configuration and agent sockets such as `SSH_AUTH_SOCK`.
#[cfg(target_os = "macos")]
fn scrub_sensitive_environment(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if is_sensitive_environment_variable(&key) {
            cmd.env_remove(key);
        }
    }
}

#[cfg(target_os = "macos")]
fn is_sensitive_environment_variable(key: &std::ffi::OsStr) -> bool {
    let key = key.to_string_lossy().to_ascii_uppercase();
    key.starts_with("HEDDLE_")
        || key == "OPENROUTER_API_KEY"
        || key == "GH_TOKEN"
        || key == "GITHUB_TOKEN"
        || key == "GITLAB_TOKEN"
        || key == "GLAB_TOKEN"
        || key == "HF_TOKEN"
        || key == "HUGGING_FACE_HUB_TOKEN"
        || key == "DOCKER_AUTH_CONFIG"
        || key.ends_with("_TOKEN")
        || key.ends_with("_KEY")
        || key.ends_with("_API_KEY")
        || key.ends_with("_ACCESS_KEY")
        || key.ends_with("_SECRET")
        || key.ends_with("_PASSWORD")
        || key.ends_with("_CREDENTIAL")
        || key.ends_with("_CREDENTIALS")
}

#[cfg(target_os = "macos")]
struct RustToolchainRuntime {
    cargo_bin: String,
    cargo_home: String,
    rustup_home: String,
    name: String,
    toolchain_root: String,
}

#[cfg(target_os = "macos")]
struct CuratedRuntime {
    bin_dir: String,
    root: String,
}

#[cfg(target_os = "macos")]
fn curated_runtimes() -> Result<Vec<CuratedRuntime>, String> {
    const COMMANDS: [&str; 5] = ["node", "npx", "tsc", "bun", "go"];
    let mut candidates = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    candidates.extend([
        std::path::PathBuf::from("/opt/homebrew/bin"),
        std::path::PathBuf::from("/usr/local/bin"),
    ]);
    let mut runtimes = Vec::new();
    for command in COMMANDS {
        let Some(executable) = candidates
            .iter()
            .map(|directory| directory.join(command))
            .find(|candidate| candidate.is_file())
        else {
            continue;
        };
        let Ok(canonical) = executable.canonicalize() else {
            continue;
        };
        let Some(root) = canonical
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
        else {
            continue;
        };
        let runtime = CuratedRuntime {
            bin_dir: sandbox_string(
                executable
                    .parent()
                    .ok_or_else(|| "Error: runtime command has no bin directory".to_string())?,
            )?,
            root: sandbox_string(root)?,
        };
        if !runtimes.iter().any(|existing: &CuratedRuntime| {
            existing.bin_dir == runtime.bin_dir && existing.root == runtime.root
        }) {
            runtimes.push(runtime);
        }
    }
    Ok(runtimes)
}

#[cfg(target_os = "macos")]
fn runtime_path(runtimes: &[CuratedRuntime], cargo_bin: Option<&str>) -> String {
    let mut entries = cargo_bin.into_iter().map(str::to_owned).collect::<Vec<_>>();
    for runtime in runtimes {
        if !entries.contains(&runtime.bin_dir) {
            entries.push(runtime.bin_dir.clone());
        }
    }
    entries.extend(["/usr/bin".to_string(), "/bin".to_string()]);
    entries.join(":")
}

#[cfg(target_os = "macos")]
fn rust_toolchain_runtime() -> Result<Option<RustToolchainRuntime>, String> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Ok(None);
    };
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".cargo"));
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
    let name = toolchain_root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Error: Rustup resolved a toolchain without a name".to_string())?
        .to_string();
    Ok(Some(RustToolchainRuntime {
        cargo_bin: sandbox_string(
            cargo.parent().ok_or_else(|| {
                "Error: Rustup resolved Cargo without a bin directory".to_string()
            })?,
        )?,
        cargo_home: sandbox_string(&cargo_home)?,
        rustup_home: sandbox_string(&rustup_home)?,
        name,
        toolchain_root: sandbox_string(toolchain_root)?,
    }))
}

#[cfg(target_os = "macos")]
fn sandbox_profile(
    root: &str,
    additional: &[String],
    runtime_root: &str,
    additional_deny_paths: &[String],
    toolchain: Option<&RustToolchainRuntime>,
    runtimes: &[CuratedRuntime],
) -> String {
    // Read-only host access is needed by dynamically linked developer tools.
    // Keep writes limited to the workspace/runtime allowances below.
    let sensitive_rules = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .into_iter()
        .flat_map(|home| {
            [
                home.join(".ssh"),
                home.join(".aws"),
                home.join(".gnupg"),
                home.join(".config/gcloud"),
                home.join(".config/gh"),
                home.join(".npmrc"),
                home.join(".netrc"),
            ]
        })
        .filter_map(|path| sandbox_string(&path).ok())
        .chain(additional_deny_paths.iter().cloned())
        .chain([
            "/etc/master.passwd".to_string(),
            "/etc/passwd".to_string(),
            "/etc/shadow".to_string(),
            "/private/var/db/dslocal".to_string(),
        ])
        .map(|path| {
            format!(
                "(deny file-read* (subpath \"{path}\"))\n(deny file-write* (subpath \"{path}\"))\n"
            )
        })
        .collect::<String>();
    let additional_rules = additional
        .iter()
        .map(|root| {
            format!(
                "(allow file-read* file-map-executable (literal \"{root}\") (subpath \"{root}\"))\n(allow file-write* (literal \"{root}\") (subpath \"{root}\"))\n(allow file-read-metadata file-test-existence (path-ancestors \"{root}\"))\n"
            )
        })
        .collect::<String>();
    let toolchain_rules = toolchain.map_or_else(String::new, |toolchain| {
        format!(
            "(allow file-read* file-map-executable (subpath \"{}\"))\n(allow file-read* (literal \"{}\") (literal \"{}/config.toml\") (subpath \"{}/registry\") (subpath \"{}/git\"))\n(allow file-read* (literal \"{}\") (literal \"{}/settings.toml\"))\n(allow file-read-metadata file-test-existence (literal \"{}\"))\n(allow file-read* file-map-executable (subpath \"{}\"))\n",
            toolchain.cargo_bin,
            toolchain.cargo_home,
            toolchain.cargo_home,
            toolchain.cargo_home,
            toolchain.cargo_home,
            toolchain.rustup_home,
            toolchain.rustup_home,
            toolchain.rustup_home,
            toolchain.toolchain_root,
        )
    });
    let runtime_rules = runtimes
        .iter()
        .map(|runtime| {
            format!(
                "(allow file-read* file-map-executable (literal \"{}\") (subpath \"{}\") (literal \"{}\") (subpath \"{}\"))\n(allow file-read-metadata file-test-existence (path-ancestors \"{}\") (path-ancestors \"{}\"))\n",
                runtime.bin_dir,
                runtime.bin_dir,
                runtime.root,
                runtime.root,
                runtime.bin_dir,
                runtime.root
            )
        })
        .collect::<String>();
    format!(
        "(version 1)\n(deny default)\n(allow network-outbound)\n(allow process-exec)\n(allow process-fork)\n(allow signal (target same-sandbox))\n(allow process-info* (target same-sandbox))\n(allow sysctl-read)\n(allow mach-lookup (global-name \"com.apple.system.opendirectoryd.libinfo\"))\n(allow pseudo-tty)\n(allow file-read* file-write-data (literal \"/dev/null\"))\n(allow file-read* file-write-data (literal \"/dev/zero\"))\n(allow file-read-data file-write-data (subpath \"/dev/fd\"))\n(allow file-read* file-map-executable (subpath \"/\"))\n(allow file-read-metadata file-test-existence (subpath \"/\"))\n(allow file-read* file-test-existence (literal \"/\") (literal \"/dev/random\") (literal \"/dev/urandom\") (subpath \"/Library/Apple\") (subpath \"/Library/Preferences\") (subpath \"/System/Library\") (subpath \"/System/Volumes/Data/Library/Preferences\") (subpath \"/usr/lib\") (subpath \"/usr/share\") (subpath \"/private/etc\"))\n(allow file-map-executable (subpath \"/Library/Apple/System/Library/Frameworks\") (subpath \"/Library/Apple/System/Library/PrivateFrameworks\") (subpath \"/System/Library/Frameworks\") (subpath \"/System/Library/PrivateFrameworks\") (subpath \"/usr/lib\"))\n(allow file-read-data file-read-metadata (subpath \"/bin\") (subpath \"/usr/bin\"))\n(allow file-read* file-map-executable (literal \"/var/db/xcode_select_link\") (literal \"/private/var/db/xcode_select_link\") (subpath \"/private/var/select\") (literal \"/Applications/Xcode.app\") (literal \"/Applications/Xcode.app/Contents/Developer\") (subpath \"/Applications/Xcode.app\") (literal \"/Library/Developer/CommandLineTools\") (subpath \"/Library/Developer/CommandLineTools\") (literal \"/System/Volumes/Data/Applications/Xcode.app\") (subpath \"/System/Volumes/Data/Applications/Xcode.app\") (literal \"/System/Volumes/Data/Library/Developer/CommandLineTools\") (subpath \"/System/Volumes/Data/Library/Developer/CommandLineTools\"))\n(allow file-read-metadata file-test-existence (literal \"/etc\") (literal \"/tmp\") (literal \"/var\") (literal \"/Applications\") (literal \"/Library\") (literal \"/Library/Developer\") (path-ancestors \"/System/Volumes/Data/private\") (path-ancestors \"{root}\"))\n(allow file-read* file-map-executable (literal \"{root}\") (subpath \"{root}\"))\n(allow file-write* (literal \"{root}\") (subpath \"{root}\"))\n(allow file-read* file-map-executable (literal \"{runtime_root}\") (subpath \"{runtime_root}\"))\n(allow file-write* (literal \"{runtime_root}\") (subpath \"{runtime_root}\"))\n(allow file-read-metadata file-test-existence (path-ancestors \"{runtime_root}\"))\n{sensitive_rules}{additional_rules}{toolchain_rules}{runtime_rules}"
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
    use super::{is_sensitive_environment_variable, sandbox_profile, RustToolchainRuntime};

    #[test]
    fn sandbox_profile_uses_sandbox_string_literals_for_workspace_paths() {
        let profile = sandbox_profile(
            "/private/tmp/workspace",
            &[],
            "/private/tmp/heddle-runtime",
            &[],
            None,
            &[],
        );

        assert!(profile.contains("(subpath \"/private/tmp/workspace\")"));
        assert!(!profile.contains("\\\\\"/private/tmp/workspace\\\\\""));
    }

    #[test]
    fn sandbox_profile_allows_only_the_rust_runtime_paths_needed_by_cargo() {
        let profile = sandbox_profile(
            "/private/tmp/workspace",
            &[],
            "/private/tmp/heddle-runtime",
            &[],
            Some(&RustToolchainRuntime {
                cargo_bin: "/Users/test/.cargo/bin".to_string(),
                cargo_home: "/Users/test/.cargo".to_string(),
                rustup_home: "/Users/test/.rustup".to_string(),
                name: "stable-aarch64-apple-darwin".to_string(),
                toolchain_root: "/Users/test/.rustup/toolchains/stable-aarch64-apple-darwin"
                    .to_string(),
            }),
            &[],
        );

        assert!(profile.contains("(subpath \"/Users/test/.cargo/bin\")"));
        assert!(profile.contains("(subpath \"/Users/test/.cargo/registry\")"));
        assert!(profile.contains("(literal \"/Users/test/.rustup/settings.toml\")"));
        assert!(profile
            .contains("(subpath \"/Users/test/.rustup/toolchains/stable-aarch64-apple-darwin\")"));
        assert!(!profile.contains("(allow file-write* (subpath \"/Users/test/.cargo\"))"));
        assert!(!profile.contains("(allow file-write* (subpath \"/Users/test/.rustup\"))"));
    }

    #[test]
    fn sandbox_profile_allows_the_macos_xcode_runtime_needed_by_cargo() {
        let profile = sandbox_profile(
            "/private/tmp/workspace",
            &[],
            "/private/tmp/heddle-runtime",
            &["/private/tmp/user-private".to_string()],
            None,
            &[],
        );

        assert!(profile.contains("(literal \"/var/db/xcode_select_link\")"));
        assert!(profile.contains("(literal \"/private/var/db/xcode_select_link\")"));
        assert!(profile.contains("(subpath \"/private/var/select\")"));
        assert!(profile.contains("(subpath \"/Applications/Xcode.app\")"));
        assert!(profile.contains("(subpath \"/System/Volumes/Data/Applications/Xcode.app\")"));
        assert!(profile.contains("(subpath \"/Library/Developer/CommandLineTools\")"));
        assert!(!profile.contains("(allow file-write* (subpath \"/Library/Developer"));
        assert!(profile.contains("(deny file-read* (subpath \"/private/tmp/user-private\"))"));
        assert!(profile.contains("(deny file-write* (subpath \"/private/tmp/user-private\"))"));
    }

    #[test]
    fn sensitive_environment_variable_detection_keeps_ssh_agent_but_removes_credentials() {
        for key in [
            "OPENROUTER_API_KEY",
            "HEDDLE_HOME",
            "GH_TOKEN",
            "AWS_SESSION_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "CUSTOM_SERVICE_KEY",
            "MY_SERVICE_PASSWORD",
            "DOCKER_AUTH_CONFIG",
        ] {
            assert!(is_sensitive_environment_variable(key.as_ref()), "{key}");
        }
        for key in [
            "HOME",
            "PATH",
            "SSH_AUTH_SOCK",
            "GPG_TTY",
            "GIT_CONFIG_GLOBAL",
        ] {
            assert!(!is_sensitive_environment_variable(key.as_ref()), "{key}");
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn confined_bash_command(
    _roots: &[std::path::PathBuf],
    _runtime_root: &std::path::Path,
    _additional_deny_paths: &[std::path::PathBuf],
    _command: &str,
) -> Result<Command, String> {
    Err(
        "Error: workspace boundary denied bash: no supported filesystem sandbox on this platform"
            .to_string(),
    )
}
