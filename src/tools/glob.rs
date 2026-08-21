//! glob tool.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use globset::Glob;
use serde_json::{json, Value};
use walkdir::WalkDir;

use super::types::{ExecOptions, HeddleTool};
use super::workspace::SharedWorkspaceBoundary;

pub struct GlobTool;
pub struct WorkspaceGlobTool(SharedWorkspaceBoundary);

const EXCLUDED_DIRS: &[&str] = &[".git", "target", "node_modules", "dist", "build"];

fn explicitly_targets_excluded(pattern: &str, path: &str) -> bool {
    let path = path.trim_matches('/');
    EXCLUDED_DIRS.iter().any(|dir| {
        path.split('/').any(|component| component == *dir)
            || pattern
                .trim_start_matches("./")
                .starts_with(&format!("{dir}/"))
    })
}

pub fn create_glob_tool() -> Arc<dyn HeddleTool> {
    Arc::new(GlobTool)
}
pub fn create_workspace_glob_tool(boundary: SharedWorkspaceBoundary) -> Arc<dyn HeddleTool> {
    Arc::new(WorkspaceGlobTool(boundary))
}

#[async_trait]
impl HeddleTool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern. Returns matching file paths, one per line."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern (e.g. 'src/**/*.ts')" },
                "path":    { "type": "string", "description": "Directory to search in (defaults to cwd)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let pattern = match params.get("pattern").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => return "Error: missing pattern".to_string(),
        };
        let path_arg = params
            .get("path")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| ".".to_string());

        let glob = match Glob::new(&pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => return format!("Error: invalid glob: {e}"),
        };
        let base = Path::new(&path_arg);
        let allow_excluded = explicitly_targets_excluded(&pattern, &path_arg);
        let mut results = Vec::new();
        let mut skipped_dirs = Vec::new();
        for entry in WalkDir::new(base)
            .into_iter()
            .filter_entry(|entry| {
                let excluded = !allow_excluded
                    && entry.file_type().is_dir()
                    && EXCLUDED_DIRS.iter().any(|dir| entry.file_name() == *dir);
                if excluded {
                    skipped_dirs.push(entry.path().to_string_lossy().into_owned());
                }
                !excluded
            })
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if glob.is_match(&rel) {
                let abs = entry
                    .path()
                    .canonicalize()
                    .unwrap_or_else(|_| entry.path().to_path_buf());
                results.push(abs.to_string_lossy().into_owned());
            }
        }
        if !skipped_dirs.is_empty() {
            results.extend(skipped_dirs.into_iter().map(|path| {
                format!(
                    "[skipped generated/VCS directory: {path}; search it explicitly to inspect it]"
                )
            }));
        }
        if results.is_empty() {
            "No files matched the pattern.".to_string()
        } else {
            results.join("\n")
        }
    }
}

#[async_trait]
impl HeddleTool for WorkspaceGlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        GlobTool.description()
    }
    fn parameters(&self) -> Value {
        GlobTool.parameters()
    }
    async fn execute(&self, mut params: Value, options: ExecOptions) -> String {
        let raw = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = match self.0.read().resolve(raw) {
            Ok(path) => path,
            Err(error) => return error.to_string(),
        };
        params["path"] = json!(path);
        GlobTool.execute(params, options).await
    }
}
