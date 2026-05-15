//! save_plan tool — writes markdown plans with YAML frontmatter.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};
use crate::plans::storage::{save_plan, PlanMeta};

pub struct SavePlanTool {
    session_id: String,
    model: Option<String>,
}

pub fn create_save_plan_tool(session_id: String, model: Option<String>) -> Arc<dyn HeddleTool> {
    Arc::new(SavePlanTool { session_id, model })
}

#[async_trait]
impl HeddleTool for SavePlanTool {
    fn name(&self) -> &str {
        "save_plan"
    }
    fn description(&self) -> &str {
        "Save a plan to disk as a markdown file. Plans persist across sessions and can be loaded later."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name":    { "type": "string", "description": "Name for the plan (used as filename)" },
                "content": { "type": "string", "description": "The plan content in markdown" }
            },
            "required": ["name", "content"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let name = match params.get("name").and_then(Value::as_str) {
            Some(n) => n.to_string(),
            None => return "Error: missing name".to_string(),
        };
        let content = match params.get("content").and_then(Value::as_str) {
            Some(c) => c.to_string(),
            None => return "Error: missing content".to_string(),
        };
        match save_plan(
            &name,
            &content,
            PlanMeta {
                model: self.model.as_deref(),
                session_id: Some(&self.session_id),
            },
            None,
        ) {
            Ok(path) => format!("Saved plan {name:?} to {}", path.display()),
            Err(e) => format!("Error: {e}"),
        }
    }
}
