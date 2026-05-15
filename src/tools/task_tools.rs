//! Task tracking tools (create_task / update_task / list_tasks).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};
use crate::tasks::storage::{
    create_task, format_tasks_summary, load_tasks, update_task, TaskUpdate,
};
use crate::tasks::types::TaskStatus;

pub struct CreateTaskTool {
    session_id: String,
    project_path: Option<String>,
}

pub fn create_create_task_tool(
    session_id: String,
    project_path: Option<String>,
) -> Arc<dyn HeddleTool> {
    Arc::new(CreateTaskTool {
        session_id,
        project_path,
    })
}

#[async_trait]
impl HeddleTool for CreateTaskTool {
    fn name(&self) -> &str {
        "create_task"
    }
    fn description(&self) -> &str {
        "Create a new task to track work across sessions. Tasks persist through context compaction."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title":   { "type": "string", "description": "Title of the task" },
                "details": { "type": "string", "description": "Additional details about the task" }
            },
            "required": ["title"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let title = match params.get("title").and_then(Value::as_str) {
            Some(t) => t.to_string(),
            None => return "Error: missing title".to_string(),
        };
        let details = params
            .get("details")
            .and_then(Value::as_str)
            .map(String::from);
        match create_task(
            &title,
            &self.session_id,
            self.project_path.as_deref(),
            details.as_deref(),
        ) {
            Ok(task) => format!(
                "Created task: {:?} (id: {}, status: {})",
                task.title,
                task.id,
                task.status.as_str()
            ),
            Err(e) => format!("Error: {e}"),
        }
    }
}

pub struct UpdateTaskTool {
    project_path: Option<String>,
}

pub fn create_update_task_tool(project_path: Option<String>) -> Arc<dyn HeddleTool> {
    Arc::new(UpdateTaskTool { project_path })
}

#[async_trait]
impl HeddleTool for UpdateTaskTool {
    fn name(&self) -> &str {
        "update_task"
    }
    fn description(&self) -> &str {
        "Update an existing task's status, title, or details."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id":      { "type": "string", "description": "ID of the task to update" },
                "status":  { "type": "string", "enum": ["pending", "in_progress", "done", "blocked"] },
                "title":   { "type": "string" },
                "details": { "type": "string" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let id = match params.get("id").and_then(Value::as_str) {
            Some(i) => i.to_string(),
            None => return "Error: missing id".to_string(),
        };
        let status = params
            .get("status")
            .and_then(Value::as_str)
            .and_then(TaskStatus::from_str);
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .map(String::from);
        let details = params
            .get("details")
            .and_then(Value::as_str)
            .map(String::from);
        match update_task(
            &id,
            TaskUpdate {
                status,
                title,
                details,
            },
            self.project_path.as_deref(),
        ) {
            Ok(task) => format!(
                "Updated task: {:?} (id: {}, status: {})",
                task.title,
                task.id,
                task.status.as_str()
            ),
            Err(e) => format!("Error: {e}"),
        }
    }
}

pub struct ListTasksTool {
    session_id: String,
    project_path: Option<String>,
}

pub fn create_list_tasks_tool(
    session_id: String,
    project_path: Option<String>,
) -> Arc<dyn HeddleTool> {
    Arc::new(ListTasksTool {
        session_id,
        project_path,
    })
}

#[async_trait]
impl HeddleTool for ListTasksTool {
    fn name(&self) -> &str {
        "list_tasks"
    }
    fn description(&self) -> &str {
        "List all tracked tasks, grouped by status."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, _options: ExecOptions) -> String {
        let tasks = load_tasks(self.project_path.as_deref());
        format_tasks_summary(&tasks, &self.session_id)
    }
}
