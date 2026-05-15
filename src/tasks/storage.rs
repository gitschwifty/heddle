//! Task persistence: read/write tasks.json under the project dir.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::Utc;
use uuid::Uuid;

use super::types::{Task, TaskStatus};
use crate::config::paths::get_project_dir;

pub fn get_tasks_path(project_path: Option<&str>) -> PathBuf {
    get_project_dir(project_path).join("tasks.json")
}

pub fn load_tasks(project_path: Option<&str>) -> Vec<Task> {
    let path = get_tasks_path(project_path);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_tasks(tasks: &[Task], project_path: Option<&str>) -> Result<()> {
    let path = get_tasks_path(project_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(tasks)?;
    std::fs::write(&path, serialized)?;
    Ok(())
}

pub fn create_task(
    title: &str,
    session_id: &str,
    project_path: Option<&str>,
    details: Option<&str>,
) -> Result<Task> {
    let mut tasks = load_tasks(project_path);
    let now = Utc::now().to_rfc3339();
    let task = Task {
        id: Uuid::new_v4().to_string(),
        title: title.to_string(),
        status: TaskStatus::Pending,
        created: now.clone(),
        updated: now,
        session_id: session_id.to_string(),
        details: details.map(String::from),
    };
    tasks.push(task.clone());
    save_tasks(&tasks, project_path)?;
    Ok(task)
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub title: Option<String>,
    pub details: Option<String>,
}

pub fn update_task(id: &str, update: TaskUpdate, project_path: Option<&str>) -> Result<Task> {
    let mut tasks = load_tasks(project_path);
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow!("Task not found: {id}"))?;
    if let Some(s) = update.status {
        task.status = s;
    }
    if let Some(t) = update.title {
        task.title = t;
    }
    if let Some(d) = update.details {
        task.details = Some(d);
    }
    task.updated = Utc::now().to_rfc3339();
    let updated = task.clone();
    save_tasks(&tasks, project_path)?;
    Ok(updated)
}

const STATUS_ORDER: &[TaskStatus] = &[
    TaskStatus::InProgress,
    TaskStatus::Blocked,
    TaskStatus::Pending,
    TaskStatus::Done,
];

pub fn format_tasks_summary(tasks: &[Task], current_session_id: &str) -> String {
    if tasks.is_empty() {
        return "No tasks tracked.".to_string();
    }
    let mut lines = Vec::new();
    for status in STATUS_ORDER {
        let group: Vec<&Task> = tasks.iter().filter(|t| t.status == *status).collect();
        if group.is_empty() {
            continue;
        }
        lines.push(format!(
            "## {}",
            status.as_str().replace('_', " ").to_uppercase()
        ));
        for task in group {
            let stale = if task.session_id != current_session_id {
                " [stale]"
            } else {
                ""
            };
            let details = task
                .details
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            lines.push(format!(
                "- [{}] {}{}{}",
                task.id, task.title, details, stale
            ));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}
