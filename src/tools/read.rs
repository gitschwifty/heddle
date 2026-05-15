//! read_file tool.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};

pub struct ReadTool;

pub fn create_read_tool() -> Arc<dyn HeddleTool> {
    Arc::new(ReadTool)
}

#[async_trait]
impl HeddleTool for ReadTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file (absolute or relative to cwd)" }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let file_path = match params.get("file_path").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => return "Error: missing file_path".to_string(),
        };
        match tokio::fs::read_to_string(&file_path).await {
            Ok(content) => content,
            Err(_) => format!("Error: Could not read file: {file_path}"),
        }
    }
}
