//! glob tool.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use globset::Glob;
use serde_json::{json, Value};
use walkdir::WalkDir;

use super::types::{ExecOptions, HeddleTool};

pub struct GlobTool;

pub fn create_glob_tool() -> Arc<dyn HeddleTool> {
    Arc::new(GlobTool)
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
        let mut results = Vec::new();
        for entry in WalkDir::new(base).into_iter().filter_map(|e| e.ok()) {
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
        if results.is_empty() {
            "No files matched the pattern.".to_string()
        } else {
            results.join("\n")
        }
    }
}
