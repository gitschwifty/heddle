use heddle::tools::task_tools::{
    create_create_task_tool, create_list_tasks_tool, create_update_task_tool,
};
use heddle::tools::types::ExecOptions;
use serde_json::json;

mod common;
use common::Sandbox;

#[tokio::test]
async fn create_task_returns_confirmation() {
    let _sb = Sandbox::new("tt-create-confirm");
    let tool = create_create_task_tool(
        "session-1".to_string(),
        Some("/test/create-confirm".to_string()),
    );
    let result = tool
        .execute(json!({ "title": "My new task" }), ExecOptions::default())
        .await;
    assert!(result.contains("My new task"), "got: {result}");
    assert!(result.contains("pending"), "got: {result}");
}

#[tokio::test]
async fn create_task_with_details() {
    let _sb = Sandbox::new("tt-create-details");
    let tool = create_create_task_tool(
        "session-1".to_string(),
        Some("/test/create-details".to_string()),
    );
    let result = tool
        .execute(
            json!({ "title": "Task with details", "details": "Extra info" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Task with details"), "got: {result}");
}

#[tokio::test]
async fn create_task_has_correct_name() {
    let _sb = Sandbox::new("tt-create-name");
    let tool = create_create_task_tool("s1".to_string(), None);
    assert_eq!(tool.name(), "create_task");
}

#[tokio::test]
async fn update_task_changes_status() {
    let _sb = Sandbox::new("tt-update-status");
    let project = Some("/test/update-status".to_string());
    let create = create_create_task_tool("session-1".to_string(), project.clone());
    let update = create_update_task_tool(project);

    let create_result = create
        .execute(json!({ "title": "Update me" }), ExecOptions::default())
        .await;
    let id_re = regex::Regex::new(r"id:\s*([0-9a-f-]+)").unwrap();
    let caps = id_re
        .captures(&create_result)
        .expect("no id in create output");
    let task_id = caps.get(1).unwrap().as_str().to_string();

    let update_result = update
        .execute(
            json!({ "id": task_id, "status": "done" }),
            ExecOptions::default(),
        )
        .await;
    assert!(update_result.contains("done"), "got: {update_result}");
}

#[tokio::test]
async fn update_task_has_correct_name() {
    let _sb = Sandbox::new("tt-update-name");
    let tool = create_update_task_tool(None);
    assert_eq!(tool.name(), "update_task");
}

#[tokio::test]
async fn list_tasks_returns_formatted_output() {
    let _sb = Sandbox::new("tt-list-formatted");
    let project = Some("/test/list-formatted".to_string());
    let create = create_create_task_tool("session-1".to_string(), project.clone());
    let list = create_list_tasks_tool("session-1".to_string(), project);

    create
        .execute(json!({ "title": "Task Alpha" }), ExecOptions::default())
        .await;
    create
        .execute(json!({ "title": "Task Beta" }), ExecOptions::default())
        .await;

    let result = list.execute(json!({}), ExecOptions::default()).await;
    assert!(result.contains("Task Alpha"), "got: {result}");
    assert!(result.contains("Task Beta"), "got: {result}");
}

#[tokio::test]
async fn list_tasks_empty_message_when_no_tasks() {
    let _sb = Sandbox::new("tt-list-empty");
    let list = create_list_tasks_tool(
        "session-1".to_string(),
        Some("/test/empty-list".to_string()),
    );
    let result = list.execute(json!({}), ExecOptions::default()).await;
    assert!(result.contains("No tasks"), "got: {result}");
}

#[tokio::test]
async fn list_tasks_has_correct_name() {
    let _sb = Sandbox::new("tt-list-name");
    let tool = create_list_tasks_tool("s1".to_string(), None);
    assert_eq!(tool.name(), "list_tasks");
}

#[tokio::test]
async fn list_tasks_flags_stale_session() {
    let _sb = Sandbox::new("tt-list-stale");
    let project = Some("/test/stale-flag".to_string());
    let old = create_create_task_tool("old-session".to_string(), project.clone());
    let current = create_create_task_tool("current-session".to_string(), project.clone());
    let list = create_list_tasks_tool("current-session".to_string(), project);

    old.execute(json!({ "title": "Old task" }), ExecOptions::default())
        .await;
    current
        .execute(json!({ "title": "Current task" }), ExecOptions::default())
        .await;

    let result = list.execute(json!({}), ExecOptions::default()).await;
    let old_stale_re = regex::Regex::new(r"(?is)Old task.*stale|stale.*Old task").unwrap();
    let current_stale_re = regex::Regex::new(r"(?is)Current task.*stale").unwrap();
    assert!(
        old_stale_re.is_match(&result),
        "expected Old task to be stale: {result}"
    );
    assert!(
        !current_stale_re.is_match(&result),
        "Current task should not be stale: {result}"
    );
}
