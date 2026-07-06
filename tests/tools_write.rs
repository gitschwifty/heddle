use heddle::tools::types::ExecOptions;
use heddle::tools::write::create_write_tool;
use serde_json::json;
use tempfile::tempdir;

mod common;
use common::Sandbox;

#[tokio::test]
async fn returns_error_for_invalid_nested_path() {
    let _sb = Sandbox::new("write-bad-path");
    let tool = create_write_tool();
    let result = tool
        .execute(
            json!({ "file_path": "/dev/null/impossible/file.txt", "content": "hello" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn returns_error_for_empty_path() {
    let _sb = Sandbox::new("write-empty-path");
    let tool = create_write_tool();
    let result = tool
        .execute(
            json!({ "file_path": "", "content": "hello" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn writes_file_and_returns_success() {
    let _sb = Sandbox::new("write-ok");
    let dir = tempdir().unwrap();
    let path = dir.path().join("out.txt");
    let tool = create_write_tool();
    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy(), "content": "data" }),
            ExecOptions::default(),
        )
        .await;
    assert!(!result.contains("Error"), "got: {result}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "data");
}

#[tokio::test]
async fn writes_creates_parent_dirs() {
    let _sb = Sandbox::new("write-parents");
    let dir = tempdir().unwrap();
    let path = dir.path().join("a/b/c/file.txt");
    let tool = create_write_tool();
    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy(), "content": "deep" }),
            ExecOptions::default(),
        )
        .await;
    assert!(!result.contains("Error"), "got: {result}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "deep");
}
