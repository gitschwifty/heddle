//! Search tool — prefers ripgrep, with a native Rust-regex fallback.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::{sinks::Lossy, BinaryDetection, SearcherBuilder};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use walkdir::WalkDir;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::SharedWorkspaceBoundary;

pub struct GrepTool;
pub struct WorkspaceGrepTool(SharedWorkspaceBoundary);

// Keep search results comfortably below the agent loop's generic 24 KiB tool
// history bound, while leaving room for an actionable truncation notice.
const MAX_SEARCH_RESULT_BYTES: usize = 20 * 1024;
const SEARCH_TRUNCATION_NOTICE: &str =
    "[search output truncated; refine the path, glob, or pattern to inspect fewer matches]";

struct SearchResult {
    output: String,
    truncated: bool,
}

enum RipgrepSearchError {
    Unavailable,
    Message(String),
}

impl SearchResult {
    fn render(mut self) -> String {
        if self.truncated {
            if !self.output.is_empty() {
                self.output.push_str("\n\n");
            }
            self.output.push_str(SEARCH_TRUNCATION_NOTICE);
        }
        if self.output.is_empty() {
            "No matches found.".to_string()
        } else {
            self.output
        }
    }
}

#[derive(Clone, Copy)]
enum SearchBackend {
    Ripgrep,
}

impl SearchBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Ripgrep => "rg",
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
    // `rg` otherwise accepts ambient RIPGREP_CONFIG_PATH options, which could
    // change the regex engine or file-discovery behavior behind this tool.
    command.env_remove("RIPGREP_CONFIG_PATH");
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
            // Credential-bearing dotenv files must not be surfaced through an
            // agent search. Keep this exclusion in sync with native_search.
            args.extend(["--glob".to_string(), "!.env*".to_string()]);
            if let Some(glob) = glob_filter {
                args.extend(["--glob".to_string(), glob.to_string()]);
            }
        }
    }
    args.extend(["-e".to_string(), pattern.to_string(), path.to_string()]);
    args
}

fn is_credential_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(".env"))
}

fn compile_glob(glob_filter: Option<&str>) -> Result<Option<globset::GlobMatcher>, String> {
    glob_filter
        .map(|glob| {
            Glob::new(glob)
                .map(|glob| glob.compile_matcher())
                .map_err(|error| format!("Error: invalid glob: {error}"))
        })
        .transpose()
}

#[cfg(test)]
fn native_search(pattern: &str, path: &str, glob_filter: Option<&str>) -> Result<String, String> {
    let matcher =
        RegexMatcher::new(pattern).map_err(|error| format!("Error: invalid regex: {error}"))?;
    native_search_with_matcher(&matcher, path, glob_filter).map(SearchResult::render)
}

fn native_search_with_matcher(
    matcher: &RegexMatcher,
    path: &str,
    glob_filter: Option<&str>,
) -> Result<SearchResult, String> {
    let glob = compile_glob(glob_filter)?;
    let root = Path::new(path);
    if !root.exists() {
        return Err(format!(
            "Error: search path does not exist: {}",
            root.display()
        ));
    }

    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\0'))
        .build();
    let mut results = String::new();
    let mut truncated = false;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || is_credential_path(entry.path()) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if glob.as_ref().is_some_and(|glob| !glob.is_match(&relative)) {
            continue;
        }

        let display_path = entry.path().display().to_string();
        searcher
            .search_path(
                matcher,
                entry.path(),
                Lossy(|line_number, line| {
                    let mut rendered = String::new();
                    write!(
                        rendered,
                        "{}:{}:{}",
                        display_path,
                        line_number,
                        line.trim_end_matches(['\n', '\r'])
                    )
                    .expect("writing into String cannot fail");
                    let separator_len = usize::from(!results.is_empty());
                    if results.len() + separator_len + rendered.len() > MAX_SEARCH_RESULT_BYTES {
                        truncated = true;
                        return Ok(false);
                    }
                    if separator_len > 0 {
                        results.push('\n');
                    }
                    results.push_str(&rendered);
                    Ok(true)
                }),
            )
            .map_err(|error| format!("Error: native search failed: {error}"))?;
        if truncated {
            break;
        }
    }

    Ok(SearchResult {
        output: results,
        truncated,
    })
}

async fn ripgrep_search(
    pattern: &str,
    path: &str,
    glob_filter: Option<&str>,
) -> Result<SearchResult, RipgrepSearchError> {
    let mut command = search_command(SearchBackend::Ripgrep, pattern, path, glob_filter);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(RipgrepSearchError::Unavailable)
        }
        Err(error) => return Err(RipgrepSearchError::Message(error.to_string())),
    };
    let mut stdout = child.stdout.take().ok_or_else(|| {
        RipgrepSearchError::Message("ripgrep stdout was not captured".to_string())
    })?;
    let mut output = Vec::with_capacity(MAX_SEARCH_RESULT_BYTES);
    let mut buffer = [0_u8; 8 * 1024];
    let mut truncated = false;

    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .map_err(|error| RipgrepSearchError::Message(error.to_string()))?;
        if read == 0 {
            break;
        }
        let remaining = MAX_SEARCH_RESULT_BYTES.saturating_sub(output.len());
        if read > remaining {
            output.extend_from_slice(&buffer[..remaining]);
            truncated = true;
            child
                .kill()
                .await
                .map_err(|error| RipgrepSearchError::Message(error.to_string()))?;
            break;
        }
        output.extend_from_slice(&buffer[..read]);
    }
    drop(stdout);
    let output_status = child
        .wait_with_output()
        .await
        .map_err(|error| RipgrepSearchError::Message(error.to_string()))?;
    if truncated {
        return Ok(SearchResult {
            output: String::from_utf8_lossy(&output).trim_end().to_string(),
            truncated: true,
        });
    }

    let exit = output_status.status.code().unwrap_or(-1);
    if exit > 1 {
        return Err(RipgrepSearchError::Message(format!(
            "rg exited with code {exit}: {}",
            String::from_utf8_lossy(&output_status.stderr)
        )));
    }
    Ok(SearchResult {
        output: String::from_utf8_lossy(&output).trim().to_string(),
        truncated: false,
    })
}

async fn search(pattern: String, path: String, glob_filter: Option<String>) -> String {
    // Validate once before choosing a backend. This gives callers one stable
    // error for syntax unsupported by the default Rust-regex contract.
    let matcher = match RegexMatcher::new(&pattern) {
        Ok(matcher) => matcher,
        Err(error) => return format!("Error: invalid regex: {error}"),
    };
    if let Err(error) = compile_glob(glob_filter.as_deref()) {
        return error;
    }

    match ripgrep_search(&pattern, &path, glob_filter.as_deref()).await {
        Ok(result) => result.render(),
        Err(RipgrepSearchError::Unavailable) => {
            match tokio::task::spawn_blocking(move || {
                native_search_with_matcher(&matcher, &path, glob_filter.as_deref())
            })
            .await
            {
                Ok(Ok(result)) => result.render(),
                Ok(Err(error)) => error,
                Err(error) => format!("Error: native search task failed: {error}"),
            }
        }
        Err(RipgrepSearchError::Message(error)) => format!("Error: {error}"),
    }
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
        "Search for a Rust-regex pattern in files with paths and line numbers. Uses ripgrep when available, otherwise a compatible native search."
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

        search(pattern, path, glob_filter).await
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
    use std::time::{Duration, Instant};

    fn benchmark_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let ordinary_line = "fn ordinary_function(value: usize) -> usize { value + 1 }\n";
        let needle_line = "fn needle_function(value: usize) -> usize { value + 1 }\n";
        // Search a meaningful amount of source text while keeping result
        // formatting out of the timing: real agent searches should return a
        // focused set of matches, not megabytes of repeated lines.
        let contents = format!("{}{}", ordinary_line.repeat(1_023), needle_line);
        for index in 0..128 {
            std::fs::write(dir.path().join(format!("source-{index:03}.rs")), &contents).unwrap();
        }
        dir
    }

    #[test]
    fn ripgrep_uses_default_regex_and_excludes_dotenv_files() {
        let args = search_args(SearchBackend::Ripgrep, "one|two", "src", Some("*.rs"));
        assert!(args.contains(&"!.env*".to_string()));
        assert!(args.contains(&"*.rs".to_string()));
        assert_eq!(args[args.len() - 3..], ["-e", "one|two", "src"]);
    }

    #[test]
    fn native_search_supports_rust_regex_syntax_and_excludes_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "id=42\nhello WORLD\n").unwrap();
        std::fs::write(dir.path().join(".env.local"), "id=999\n").unwrap();

        let result =
            native_search(r"id=\d+|(?i)world", &dir.path().to_string_lossy(), None).unwrap();

        assert!(result.contains("id=42"));
        assert!(result.contains("hello WORLD"));
        assert!(!result.contains("999"));
    }

    #[test]
    fn native_search_rejects_lookaround_with_a_stable_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = native_search(r"(?<=id=)\d+", &dir.path().to_string_lossy(), None).unwrap_err();
        assert!(error.starts_with("Error: invalid regex:"));
    }

    #[tokio::test]
    async fn ripgrep_and_native_search_share_the_default_regex_contract() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "id=42\nhello WORLD\n").unwrap();
        std::fs::write(dir.path().join(".env.local"), "id=999\n").unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let pattern = r"id=\d+|(?i)world";

        let native = native_search(pattern, &path, None).unwrap();
        let output = match search_command(SearchBackend::Ripgrep, pattern, &path, None)
            .output()
            .await
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("failed to start rg: {error}"),
        };
        assert!(output.status.success());
        let ripgrep = String::from_utf8(output.stdout).unwrap();

        assert_eq!(ripgrep.trim(), native);
        assert!(!ripgrep.contains("999"));
    }

    #[tokio::test]
    async fn ripgrep_and_native_search_share_file_discovery_contract() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/visible.rs"), "needle visible\n").unwrap();
        std::fs::write(root.join(".hidden.rs"), "needle hidden\n").unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
        std::fs::write(root.join("ignored.rs"), "needle ignored\n").unwrap();
        std::fs::write(root.join("binary.bin"), b"needle binary\0").unwrap();
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("needle utf16\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(root.join("utf16.txt"), utf16).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("src/visible.rs"), root.join("linked.rs")).unwrap();
        let path = root.to_string_lossy().into_owned();

        let native = native_search("needle", &path, None).unwrap();
        let output = match search_command(SearchBackend::Ripgrep, "needle", &path, None)
            .output()
            .await
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("failed to start rg: {error}"),
        };
        assert!(output.status.success());
        let ripgrep = String::from_utf8_lossy(&output.stdout);

        for expected in ["visible.rs", ".hidden.rs", "ignored.rs", "utf16.txt"] {
            assert!(
                native.contains(expected),
                "native result missing {expected}: {native}"
            );
            assert!(
                ripgrep.contains(expected),
                "ripgrep result missing {expected}: {ripgrep}"
            );
        }
        assert!(!native.contains("needle binary"));
        assert!(!ripgrep.contains("needle binary"));
        #[cfg(unix)]
        {
            assert!(!native.contains("linked.rs"));
            assert!(!ripgrep.contains("linked.rs"));
        }

        let native_glob = native_search("needle", &path, Some("*.rs")).unwrap();
        let output = search_command(SearchBackend::Ripgrep, "needle", &path, Some("*.rs"))
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let ripgrep_glob = String::from_utf8_lossy(&output.stdout);
        for expected in ["visible.rs", ".hidden.rs", "ignored.rs"] {
            assert!(
                native_glob.contains(expected),
                "native glob result missing {expected}: {native_glob}"
            );
            assert!(
                ripgrep_glob.contains(expected),
                "ripgrep glob result missing {expected}: {ripgrep_glob}"
            );
        }
        assert!(!native_glob.contains("utf16.txt"));
        assert!(!ripgrep_glob.contains("utf16.txt"));
    }

    #[tokio::test]
    async fn native_fallback_performance_guardrail() {
        let dir = benchmark_fixture();
        let path = dir.path().to_string_lossy().into_owned();
        let pattern = r"needle_function\(value: usize\)";

        let mut rg = search_command(SearchBackend::Ripgrep, pattern, &path, Some("*.rs"));
        let Ok(output) = rg.output().await else {
            eprintln!("rg is unavailable; native fallback benchmark skipped");
            return;
        };
        assert!(output.status.success());

        // Warm filesystem caches and regex compilation before measuring the
        // implementation work rather than cold-start effects.
        native_search(pattern, &path, Some("*.rs")).unwrap();
        search_command(SearchBackend::Ripgrep, pattern, &path, Some("*.rs"))
            .output()
            .await
            .unwrap();

        let native_started = Instant::now();
        let native = native_search(pattern, &path, Some("*.rs")).unwrap();
        let native_elapsed = native_started.elapsed();

        let rg_started = Instant::now();
        let output = search_command(SearchBackend::Ripgrep, pattern, &path, Some("*.rs"))
            .output()
            .await
            .unwrap();
        let rg_elapsed = rg_started.elapsed();
        assert!(output.status.success());
        // ripgrep's parallel traversal may order files differently, so check
        // equivalent result cardinality rather than serialized order.
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().lines().count(),
            native.lines().count()
        );

        eprintln!(
            "native fallback: {:?}; rg: {:?}; ratio: {:.2}x",
            native_elapsed,
            rg_elapsed,
            native_elapsed.as_secs_f64() / rg_elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
        );

        // This is intentionally forgiving: rg is expected to win on large
        // trees, but the fallback must remain usable when rg is not installed.
        let limit = rg_elapsed
            .saturating_mul(10)
            .max(Duration::from_millis(100));
        assert!(
            native_elapsed <= limit,
            "native fallback took {:?}; expected at most {:?} (rg: {:?})",
            native_elapsed,
            limit,
            rg_elapsed
        );
    }
}
