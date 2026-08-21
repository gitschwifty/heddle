use heddle::tools::{
    create_workspace_edit_tool, create_workspace_glob_tool, create_workspace_grep_tool,
    create_workspace_read_tool, create_workspace_write_tool, ExecOptions, HeddleTool,
    WorkspaceBoundary,
};
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

fn boundary() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    heddle::tools::SharedWorkspaceBoundary,
) {
    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(workspace.path().join("inside.txt"), "inside").unwrap();
    std::fs::write(outside.path().join("secret.txt"), "outside secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape")).unwrap();
    let boundary = WorkspaceBoundary::new(workspace.path()).unwrap();
    (
        workspace,
        outside,
        Arc::new(parking_lot::RwLock::new(boundary)),
    )
}

async fn assert_denied(tool: std::sync::Arc<dyn HeddleTool>, params: serde_json::Value) {
    let result = tool.execute(params, ExecOptions::default()).await;
    assert!(
        result.starts_with("Error: workspace boundary denied path"),
        "got: {result}"
    );
}

#[tokio::test]
async fn file_tools_deny_absolute_parent_and_symlink_escapes_without_disclosure() {
    let (workspace, outside, boundary) = boundary();
    let secret = outside.path().join("secret.txt");

    assert_denied(
        create_workspace_read_tool(boundary.clone()),
        json!({"file_path": secret}),
    )
    .await;
    assert_denied(
        create_workspace_read_tool(boundary.clone()),
        json!({"file_path": "escape/secret.txt"}),
    )
    .await;
    assert_denied(
        create_workspace_write_tool(boundary.clone()),
        json!({"file_path": "../outside.txt", "content": "no"}),
    )
    .await;
    assert_denied(
        create_workspace_write_tool(boundary.clone()),
        json!({"file_path": "escape/new.txt", "content": "no"}),
    )
    .await;
    assert_denied(
        create_workspace_edit_tool(boundary.clone()),
        json!({"file_path": "../secret.txt", "old_string": "outside", "new_string": "changed"}),
    )
    .await;
    assert_denied(
        create_workspace_glob_tool(boundary.clone()),
        json!({"pattern": "*", "path": "../"}),
    )
    .await;
    assert_denied(
        create_workspace_grep_tool(boundary),
        json!({"pattern": "secret", "path": "escape"}),
    )
    .await;

    assert_eq!(std::fs::read_to_string(secret).unwrap(), "outside secret");
    assert!(!workspace.path().join("escape/new.txt").exists());
}

#[tokio::test]
async fn explicitly_added_root_is_available_to_the_same_tool_registry() {
    let (_workspace, outside, boundary) = boundary();
    let path = outside.path().join("secret.txt");
    boundary.write().add_root(outside.path()).unwrap();
    let result = create_workspace_read_tool(boundary)
        .execute(json!({"file_path": path}), ExecOptions::default())
        .await;
    assert_eq!(result, "outside secret");
}
