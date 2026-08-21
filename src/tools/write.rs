//! write_file tool — backs up before overwrite.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};
use super::workspace::WorkspaceBoundary;
use crate::file_history::backup::backup_file;

pub struct WriteTool;
pub struct WorkspaceWriteTool(WorkspaceBoundary);

pub fn create_write_tool() -> Arc<dyn HeddleTool> {
    Arc::new(WriteTool)
}
pub fn create_workspace_write_tool(boundary: WorkspaceBoundary) -> Arc<dyn HeddleTool> {
    Arc::new(WorkspaceWriteTool(boundary))
}

#[async_trait]
impl HeddleTool for WriteTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write content to a file, creating parent directories if needed. Overwrites existing files."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file (absolute or relative to cwd)" },
                "content":   { "type": "string", "description": "Content to write" }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let file_path = match params
            .get("file_path")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str)
        {
            Some(p) => p.to_string(),
            None => return "Error: missing file_path".to_string(),
        };
        let content = match params.get("content").and_then(Value::as_str) {
            Some(c) => c.to_string(),
            None => return "Error: missing content".to_string(),
        };

        if let Err(e) = backup_file(Path::new(&file_path), None) {
            return format!("Error: backup failed: {e}");
        }
        if let Some(parent) = Path::new(&file_path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return format!("Error: Could not create parent dir: {e}");
                }
            }
        }
        let len = content.len();
        match tokio::fs::write(&file_path, &content).await {
            Ok(_) => format!("Wrote {len} bytes to {file_path}"),
            Err(e) => format!("Error: Could not write file: {e}"),
        }
    }
}

#[async_trait]
impl HeddleTool for WorkspaceWriteTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        WriteTool.description()
    }
    fn parameters(&self) -> Value {
        WriteTool.parameters()
    }
    async fn execute(&self, mut params: Value, options: ExecOptions) -> String {
        let raw = match params
            .get("file_path")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str)
        {
            Some(path) => path,
            None => return "Error: missing file_path".into(),
        };
        let path = match self.0.resolve(raw) {
            Ok(path) => path,
            Err(error) => return error.to_string(),
        };
        if params.get("file_path").is_some() {
            params["file_path"] = json!(path);
        } else {
            params["path"] = json!(path);
        }
        WriteTool.execute(params, options).await
    }
}
