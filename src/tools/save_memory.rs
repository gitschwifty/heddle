//! save_memory tool — appends to MEMORY.md (project or global).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};
use crate::config::paths::get_global_memory_dir;

pub struct SaveMemoryTool {
    memory_dir: PathBuf,
}

pub fn create_save_memory_tool(memory_dir: PathBuf) -> Arc<dyn HeddleTool> {
    Arc::new(SaveMemoryTool { memory_dir })
}

#[async_trait]
impl HeddleTool for SaveMemoryTool {
    fn name(&self) -> &str {
        "save_memory"
    }
    fn description(&self) -> &str {
        "Save a memory note to MEMORY.md. Use scope='project' for project-specific notes or scope='global' for cross-project notes."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "The memory content to save" },
                "scope":   { "type": "string", "enum": ["project", "global"], "default": "project" }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let content = match params.get("content").and_then(Value::as_str) {
            Some(c) => c.to_string(),
            None => return "Error: missing content".to_string(),
        };
        let scope = params
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("project");
        let target_dir = if scope == "global" {
            get_global_memory_dir()
        } else {
            self.memory_dir.clone()
        };
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            return format!("Error: {e}");
        }
        let file_path = target_dir.join("MEMORY.md");
        let entry = format!("\n## {}\n\n{content}\n", Utc::now().to_rfc3339());
        let res = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .and_then(|mut f| f.write_all(entry.as_bytes()));
        match res {
            Ok(_) => format!("Saved memory to {scope} MEMORY.md"),
            Err(e) => format!("Error: {e}"),
        }
    }
}
